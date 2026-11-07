//! Video: turn speech into text with Whisper (whisper.cpp, MIT licence).
//!
//! Gives back subtitles (.srt) and a transcript we store in the Immich description,
//! so Immich's search can find a video by what people say in it.

use std::{path::Path, sync::OnceLock};

use anyhow::{anyhow, Result};
use axum::{
    extract::{Path as UrlPath, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{
    routes::{err, immich_for, ApiResult},
    tools::blocking,
    State as AppState,
};

fn model_files(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let size = std::env::var("STUDIO_WHISPER_MODEL").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "base".into());
    let d = dir.join("whisper-cpp");
    (d.join(format!("ggml-{size}.bin")), d.join("ggml-silero-v5.1.2.bin"))
}

/// The Whisper model, loaded the first time a video is transcribed.
fn whisper(dir: &Path) -> Result<&'static WhisperContext> {
    static CTX: OnceLock<WhisperContext> = OnceLock::new();
    if let Some(c) = CTX.get() {
        return Ok(c);
    }
    let (model, _) = model_files(dir);
    let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())
        .map_err(|e| anyhow!("Whisper model {}: {e}", model.display()))?;
    Ok(CTX.get_or_init(|| ctx))
}

fn ts(t: f64) -> String {
    let ms = (t * 1000.0) as u64;
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// Speech in a video file -> language, text, timed segments and .srt subtitles.
pub fn transcribe(video: &[u8], filename: &str, model_dir: &Path) -> Result<Value> {
    let dir = tempfile::tempdir()?;
    let ext = Path::new(filename).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "mp4".into());
    let src = dir.path().join(format!("video.{ext}"));
    std::fs::write(&src, video)?;
    // 16 kHz mono 16-bit sound, straight from ffmpeg
    let out = std::process::Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-i"]).arg(&src)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "s16le", "-"])
        .output()?;
    if !out.status.success() || out.stdout.len() < 3200 {
        return Ok(json!({ "language": null, "text": "", "srt": "", "segments": [], "note": "no audio track" }));
    }
    let audio: Vec<f32> = out.stdout.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();

    let ctx = whisper(model_dir)?;
    let mut state = ctx.create_state().map_err(|e| anyhow!("Whisper: {e}"))?;
    let mut params = FullParams::new(SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 });
    params.set_language(Some("auto"));
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    params.set_n_threads(std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4).min(8));
    // skip the silent parts (Silero voice detector), like the Python Studio did
    let (_, vad) = model_files(model_dir);
    let vad = vad.to_string_lossy().into_owned();
    if Path::new(&vad).exists() {
        params.set_vad_model_path(Some(&vad));
        params.enable_vad(true);
    }
    state.full(params, &audio).map_err(|e| anyhow!("Whisper: {e}"))?;
    let language = whisper_rs::get_lang_str(state.full_lang_id_from_state()).unwrap_or("en").to_string();
    let mut segments = vec![];
    for i in 0..state.full_n_segments() {
        let Some(seg) = state.get_segment(i) else { continue };
        let text = seg.to_str_lossy().map(|t| t.trim().to_string()).unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        segments.push(json!({ "start": seg.start_timestamp() as f64 / 100.0, "end": seg.end_timestamp() as f64 / 100.0, "text": text }));
    }
    let srt = segments.iter().enumerate().map(|(i, s)| {
        format!("{}\n{} --> {}\n{}\n", i + 1, ts(s["start"].as_f64().unwrap_or(0.0)), ts(s["end"].as_f64().unwrap_or(0.0)),
                s["text"].as_str().unwrap_or(""))
    }).collect::<Vec<_>>().join("\n");
    let text = segments.iter().filter_map(|s| s["text"].as_str()).collect::<Vec<_>>().join(" ");
    Ok(json!({ "language": language, "text": text, "srt": srt, "segments": segments }))
}

pub async fn transcribe_route(State(st): State<AppState>, h: HeaderMap, UrlPath(id): UrlPath<String>) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let s = st.clone();
    let job = st.jobs.start("transcribe", move |_job| async move {
        let a = im.asset(&id).await?;
        let data = im.original(&id).await?;
        let name = a["originalFileName"].as_str().unwrap_or("video.mp4").to_string();
        let dir = s.cfg.model_dir.clone();
        let res = blocking(move || transcribe(&data, &name, &dir)).await?;
        std::fs::write(s.cfg.data_dir.join(format!("{id}.srt")), res["srt"].as_str().unwrap_or(""))?;
        let text = res["text"].as_str().unwrap_or("");
        if !text.is_empty() {
            let old = a["exifInfo"]["description"].as_str().unwrap_or("").split("\n\n[Transcript]").next().unwrap_or("").to_string();
            let sep = if old.is_empty() { "" } else { "\n\n" };
            let short: String = text.chars().take(1900).collect();
            im.set_description(&id, &format!("{old}{sep}[Transcript] {short}")).await?;
            let tags = im.upsert_tags(&[format!("Video/Speech/{}", res["language"].as_str().unwrap_or("en"))]).await?;
            im.tag_assets(&tags.into_iter().map(|(_, t)| t).collect::<Vec<_>>(), &[id.clone()]).await?;
        }
        Ok(json!({ "language": res["language"], "text": res["text"], "segments": res["segments"] }))
    });
    Ok(Json(json!({ "job": job })))
}

pub async fn subtitles(State(st): State<AppState>, UrlPath(id): UrlPath<String>) -> ApiResult<Response> {
    if id.contains('/') || id.contains('\\') || id.starts_with('.') {
        return Err(err(StatusCode::NOT_FOUND, "Not found"));
    }
    let data = tokio::fs::read(st.cfg.data_dir.join(format!("{id}.srt"))).await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Transcribe the video first."))?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
         (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}.srt\""))], data).into_response())
}
