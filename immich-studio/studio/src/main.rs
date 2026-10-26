//! Gimmich Studio, in Rust.
//!
//! One program that serves the Studio pages and its API: photos, file formats, editing, printing,
//! photo books, AI labels, faces, video speech-to-text, logo and device setup. Requests run
//! concurrently on tokio's threads; heavy picture and AI work runs on blocking worker threads.

mod ai;
mod brand;
mod clip;
mod editor;
mod faces;
mod formats;
mod immich;
mod jobs;
mod labels;
mod learn;
mod printing;
mod routes;
mod tools;
mod video;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::Router;
use tower_http::services::ServeDir;

pub struct Config {
    pub immich_url: String,
    pub api_key: Option<String>,
    pub data_dir: PathBuf,
    pub static_dir: PathBuf,
    pub graphite_dir: PathBuf,
    pub downloads_dir: PathBuf,
    pub brand_default_dir: PathBuf,
    pub model_dir: PathBuf,
    pub trained_dir: PathBuf,
    pub public_url: Option<String>,
    pub camera_album: String,
}

impl Config {
    fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let path = |k: &str, d: &str| PathBuf::from(var(k).unwrap_or_else(|| d.into()));
        Self {
            immich_url: var("IMMICH_URL").unwrap_or_else(|| "http://immich-server:2283".into()),
            api_key: var("IMMICH_API_KEY"),
            data_dir: path("STUDIO_DATA_DIR", "/data"),
            static_dir: path("STUDIO_STATIC_DIR", "/srv/static"),
            graphite_dir: path("STUDIO_GRAPHITE_DIR", "/graphite"),
            downloads_dir: path("STUDIO_DOWNLOADS_DIR", "/downloads"),
            brand_default_dir: path("STUDIO_BRAND_DEFAULT_DIR", "/srv/brand-default"),
            model_dir: path("STUDIO_MODEL_DIR", "/models"),
            trained_dir: path("STUDIO_TRAINED_DIR", "/srv/trained"),
            public_url: var("GIMMICH_PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string()),
            camera_album: var("STUDIO_CAMERA_ALBUM").unwrap_or_else(|| "Studio Camera".into()),
        }
    }
}

pub struct AppState {
    pub cfg: Config,
    pub http: reqwest::Client,
    pub jobs: jobs::Jobs,
    pub ai: ai::Ai,
}

pub type State = Arc<AppState>;

/// `gimmich-studio check-formats DIR...`: open every file and check a real picture comes out.
fn check_formats(dirs: &[String]) -> anyhow::Result<()> {
    let (mut ok, mut bad) = (0, 0);
    for d in dirs {
        let mut files: Vec<_> = std::fs::read_dir(d)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            let t = std::time::Instant::now();
            let res = std::fs::read(&f).map_err(anyhow::Error::from).and_then(|data| formats::open_any(&data, &name));
            match res {
                Ok(img) => {
                    let px = img.as_raw();
                    let mean = px.iter().map(|&v| v as f64).sum::<f64>() / px.len() as f64;
                    let sd = (px.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / px.len() as f64).sqrt();
                    if img.width() > 10 && img.height() > 10 && sd > 3.0 {
                        println!("  OK   {name:45} {}x{}  {:.1}s", img.width(), img.height(), t.elapsed().as_secs_f32());
                        ok += 1;
                    } else {
                        println!("  FAIL {name:45} picture is blank or tiny");
                        bad += 1;
                    }
                }
                Err(e) => { println!("  FAIL {name:45} {e:#}"); bad += 1; }
            }
        }
    }
    println!("\n{ok} opened, {bad} failed");
    if bad > 0 { std::process::exit(1) }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("check-formats") {
        return check_formats(&args[2..]);
    }
    if args.get(1).map(String::as_str) == Some("convert") && (4..=5).contains(&args.len()) {
        // `gimmich-studio convert IN OUT.jpg [QUALITY]`: read any format, write a JPEG (for checking by eye)
        let img = formats::open_any(&std::fs::read(&args[2])?, &args[2])?;
        let q = args.get(4).and_then(|q| q.parse().ok());
        std::fs::write(&args[3], formats::to_jpeg(&img, if q.is_some() { None } else { Some(2000) }, q.unwrap_or(90))?)?;
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("edit") && args.len() == 5 {
        // `gimmich-studio edit IN OUT.png '[steps]'`: run edit steps (for comparing with the Python Studio)
        let img = formats::open_any(&std::fs::read(&args[2])?, &args[2])?;
        let steps: Vec<serde_json::Value> = serde_json::from_str(&args[4])?;
        editor::apply(img, &steps)?.save(&args[3])?;
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("upscale") && args.len() == 5 {
        // `gimmich-studio upscale IN OUT.png 2|4`: AI upscale (for comparing with the Python Studio)
        let img = formats::open_any(&std::fs::read(&args[2])?, &args[2])?;
        printing::upscale(&img, args[4].parse()?, |_| {})?.save(&args[3])?;
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("faces") && args.len() >= 3 {
        // `gimmich-studio faces IMG...`: face results as JSON lines (for comparing with the Python Studio)
        let dir = Config::from_env().model_dir;
        let p = faces::FacePipeline::load(&dir)?;
        for f in &args[2..] {
            let img = formats::open_any(&std::fs::read(f)?, f)?;
            let found: Vec<_> = p.analyze(&img, 0.6)?.into_iter().map(|f| serde_json::json!({
                "box": f.bbox, "kps": f.kps, "age": f.age_insightface, "sex": f.sex, "emb": f.embedding, "raw": f.raw_embedding,
                "p106": f.points106, "mesh": f.mesh.map(|m| m.iter().map(|q| [q[0], q[1]]).collect::<Vec<_>>()),
            })).collect();
            println!("{}", serde_json::json!({ "file": f, "faces": found }));
        }
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("train-age-invariant") && args.len() == 4 {
        // `gimmich-studio train-age-invariant TRAINED_DIR OUT.json`: the layer from FG-NET only (for checking)
        let dir = std::path::Path::new(&args[2]);
        let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("fgnet_antelopev2.json"))?)?;
        let dim = meta["dim"].as_u64().unwrap_or(512) as usize;
        let raw = std::fs::read(dir.join("fgnet_antelopev2.f32"))?;
        let all: Vec<f32> = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let x: Vec<Vec<f32>> = all.chunks(dim).map(|c| c.to_vec()).collect();
        let y: Vec<String> = serde_json::from_value(meta["people"].clone())?;
        std::fs::write(&args[3], serde_json::to_vec(&learn::age_invariant(&x, &y, 200, 0.3)?)?)?;
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("train-logistic") && args.len() == 5 {
        // `gimmich-studio train-logistic X.json Y.json OUT.json`: label training on given numbers (for checking)
        let x: Vec<Vec<f32>> = serde_json::from_slice(&std::fs::read(&args[2])?)?;
        let y: Vec<String> = serde_json::from_slice(&std::fs::read(&args[3])?)?;
        std::fs::write(&args[4], serde_json::to_vec(&learn::logistic_regression(&x, &y, 5.0)?)?)?;
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("transcribe") && args.len() == 3 {
        // `gimmich-studio transcribe VIDEO`: speech to text (for checking)
        let res = video::transcribe(&std::fs::read(&args[2])?, &args[2], &Config::from_env().model_dir)?;
        println!("{}", serde_json::to_string_pretty(&res)?);
        return Ok(());
    }
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
    ).init();
    let cfg = Config::from_env();
    std::fs::create_dir_all(&cfg.data_dir)?;
    brand::seed_default(&cfg)?;

    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(600)).build()?;
    let ai = ai::Ai::new(http.clone(), cfg.model_dir.clone(), cfg.trained_dir.clone());
    let state: State = Arc::new(AppState { cfg, http, jobs: jobs::Jobs::default(), ai });

    let app = Router::new()
        .merge(routes::router())
        .nest_service("/graphite", ServeDir::new(&state.cfg.graphite_dir).append_index_html_on_directories(true))
        .fallback_service(ServeDir::new(&state.cfg.static_dir).append_index_html_on_directories(true))
        .with_state(state.clone())
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024 * 1024));

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Gimmich Studio on {addr}");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}
