//! Studio tools on single photos: import any format, edit, Graphite round-trip, print and photo book.

use axum::{
    body::Bytes,
    extract::{Multipart, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use image::RgbImage;
use serde_json::{json, Value};

use crate::{
    editor, formats,
    immich::Immich,
    printing,
    routes::{err, immich_for, ApiResult},
    State as AppState,
};

/// Picture work is slow and blocks: run it on a worker thread so other requests keep flowing.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(f).await?
}

fn stem(name: &str) -> String {
    std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "photo".into())
}

/// The uploaded "file" field of a form: (file name, bytes).
async fn form_file(mut form: Multipart, default_name: &str) -> ApiResult<(String, Bytes)> {
    while let Some(field) = form.next_field().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))? {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or(default_name).to_string();
            let data = field.bytes().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
            return Ok((name, data));
        }
    }
    Err(err(StatusCode::BAD_REQUEST, "No file"))
}

/// A photo as pixels: the quick preview (for on-screen work) or the full original.
async fn load(im: &Immich, id: &str, full: bool) -> anyhow::Result<(RgbImage, Value)> {
    let a = im.asset(id).await?;
    let img = if full {
        let data = im.original(id).await?;
        let name = a["originalFileName"].as_str().unwrap_or("").to_string();
        blocking(move || formats::open_any(&data, &name)).await?
    } else {
        let data = im.thumbnail(id, "preview").await?;
        blocking(move || Ok(image::load_from_memory(&data)?.to_rgb8())).await?
    };
    Ok((img, a))
}

fn name_of(a: &Value) -> String {
    a["originalFileName"].as_str().unwrap_or("photo").to_string()
}

// ------------------------------------------------------------------ import ---
/// Upload any file. Formats Immich can't show are converted (TIFF keeps full quality).
pub async fn import(State(st): State<AppState>, h: HeaderMap, form: Multipart) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let (name, data) = form_file(form, "file").await?;
    if formats::immich_native(&name) {
        return Ok(Json(json!({ "asset": im.upload(data, &name, None).await?, "converted": false })));
    }
    let n = name.clone();
    let (out, ext, w, h) = blocking(move || {
        let img = formats::open_any(&data, &n)?;
        let (out, ext) = formats::save_as(&img, "tiff")?;
        Ok((out, ext, img.width(), img.height()))
    }).await.map_err(|e| err(StatusCode::UNSUPPORTED_MEDIA_TYPE, &format!("{e:#}")))?;
    let id = im.upload(out.into(), &format!("{}{ext}", stem(&name)), None).await?;
    Ok(Json(json!({ "asset": id, "converted": true, "size": [w, h] })))
}

// -------------------------------------------------------------------- edit ---
/// Preview edits on screen (fast, small) or save them as a new photo next to the original.
pub async fn edit(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<Response> {
    let im = immich_for(&st, &h)?;
    let preview_only = body["preview"].as_bool().unwrap_or(true);
    let steps = body["steps"].as_array().cloned().unwrap_or_default();
    let (img, a) = load(&im, &id, !preview_only).await?;
    if preview_only {
        let jpeg = blocking(move || formats::to_jpeg(&editor::apply(img, &steps)?, Some(1400), 90))
            .await.map_err(|e| err(StatusCode::BAD_REQUEST, &format!("{e:#}")))?;
        return Ok(([(header::CONTENT_TYPE, "image/jpeg")], jpeg).into_response());
    }
    let fmt = body["format"].as_str().unwrap_or("jpeg").to_string();
    let (data, ext) = blocking(move || formats::save_as(&editor::apply(img, &steps)?, &fmt))
        .await.map_err(|e| err(StatusCode::BAD_REQUEST, &format!("{e:#}")))?;
    let new_id = im.upload(data.into(), &format!("{}-edited{ext}", stem(&name_of(&a))), a["fileCreatedAt"].as_str()).await?;
    let replace = body["replace"].as_bool().unwrap_or(false);
    if replace {
        im.delete(&[id], false).await?;
    }
    Ok(Json(json!({ "new_asset": new_id, "original_in_trash": replace })).into_response())
}

/// Graphite (patched) posts its exported picture here; we store it in Immich next to the original.
pub async fn graphite_return(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>, form: Multipart) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let a = im.asset(&id).await?;
    let (name, data) = form_file(form, "graphite.png").await?;
    let ext = match formats::ext_of(&name) {
        e if e.is_empty() => "png".to_string(),
        e => e,
    };
    let new_id = im.upload(data, &format!("{}-graphite.{ext}", stem(&name_of(&a))), a["fileCreatedAt"].as_str()).await?;
    Ok(Json(json!({ "new_asset": new_id })))
}

// ---------------------------------------------------------------- printing ---
/// The full-quality picture for printing (RAW files are developed at full resolution).
async fn print_source(im: &Immich, id: &str) -> anyhow::Result<(RgbImage, Value)> {
    load(im, id, true).await
}

pub async fn print_check(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let a = im.asset(&id).await?;
    let exif = &a["exifInfo"];
    let (w, h) = match (exif["exifImageWidth"].as_u64(), exif["exifImageHeight"].as_u64()) {
        (Some(w), Some(h)) if w > 0 => (w as u32, h as u32),
        _ => print_source(&im, &id).await?.0.dimensions(),
    };
    Ok(Json(json!({
        "pixels": [w, h], "megapixels": ((w as f64 * h as f64 / 1e6) * 10.0).round() / 10.0,
        "raw": formats::is_raw(&name_of(&a)), "papers": printing::print_report(w, h),
    })))
}

/// Make a print-ready PDF in the background (AI upscale first when the photo is too small).
pub async fn print_make(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let paper = body["paper"].as_str().unwrap_or("A4").to_string();
    printing::paper_mm(&paper).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let job = st.jobs.start("print", move |job| async move {
        let (mut img, a) = print_source(&im, &id).await?;
        let name = name_of(&a);
        let (p, title) = (job.clone(), name.clone());
        let (pdf, result) = blocking(move || {
            if let Some(steps) = body["steps"].as_array().filter(|s| !s.is_empty()) {
                img = editor::apply(img, steps)?;
            }
            let up = match &body["upscale"] {
                Value::Number(n) => n.as_u64().unwrap_or(1) as u32,
                Value::String(s) if s != "auto" => s.parse().unwrap_or(1),
                _ => printing::print_report(img.width(), img.height()).iter()
                    .find(|r| r["paper"] == paper.as_str()).and_then(|r| r["upscale"].as_u64()).unwrap_or(1) as u32,
            };
            p.set(10);
            if up > 1 {
                img = printing::upscale(&img, up, |f| p.set(10 + (70.0 * f) as u32))?;
            }
            p.set(80);
            let pdf = printing::print_pdf(&img, &paper, body["fit"].as_str().unwrap_or("fill"),
                body["margin_mm"].as_f64().unwrap_or(0.0) as f32, body["crop_marks"].as_bool().unwrap_or(false), &title)?;
            let dpi = printing::dpi_on_paper(img.width(), img.height(), &paper)?;
            Ok((pdf, json!({ "paper": paper, "upscaled": up, "pixels": [img.width(), img.height()], "dpi": dpi.round() as i64 })))
        }).await?;
        job.file(pdf.into(), "application/pdf", &format!("{}-{}.pdf", stem(&name), result["paper"].as_str().unwrap_or("")));
        Ok(result)
    });
    Ok(Json(json!({ "job": job })))
}

/// A photo book PDF from chosen photos or a whole album, with dates and places as captions.
pub async fn book(State(st): State<AppState>, h: HeaderMap, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let job = st.jobs.start("book", move |job| async move {
        let ids: Vec<String> = match body["asset_ids"].as_array().filter(|a| !a.is_empty()) {
            Some(a) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            None => im.album_asset_ids(body["album_id"].as_str().unwrap_or_default()).await?,
        };
        let full = body["full_quality"].as_bool().unwrap_or(false);
        let mut photos = vec![];
        for (i, id) in ids.iter().enumerate() {
            let (img, a) = load(&im, id, full).await?;
            let exif = &a["exifInfo"];
            let date: String = exif["dateTimeOriginal"].as_str().or(a["fileCreatedAt"].as_str()).unwrap_or("").chars().take(10).collect();
            let place = [&exif["city"], &exif["country"]].iter().filter_map(|v| v.as_str()).filter(|s| !s.is_empty())
                .collect::<Vec<_>>().join(", ");
            let desc = exif["description"].as_str().unwrap_or("").split("\n\n[Immich Studio]").next().unwrap_or("").to_string();
            let caption = [desc, place, date].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            photos.push((img, caption));
            job.set((90 * (i + 1) / ids.len()) as u32);
        }
        let per_page = body["per_page"].as_u64().unwrap_or(2).clamp(1, 4) as usize;
        let title = body["title"].as_str().unwrap_or("Photo book").to_string();
        let (t, n) = (title.clone(), photos.len());
        let pdf = blocking(move || printing::book_pdf(&photos, body["paper"].as_str().unwrap_or("A4"), per_page, &t,
                                                       body["subtitle"].as_str().unwrap_or(""))).await?;
        job.file(pdf.into(), "application/pdf", &format!("{title}.pdf"));
        Ok(json!({ "pages": 1 + n.div_ceil(per_page) }))
    });
    Ok(Json(json!({ "job": job })))
}
