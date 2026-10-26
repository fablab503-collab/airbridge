//! Gimmich Studio, in Rust.
//!
//! One program that serves the Studio page and its API. Requests run concurrently on tokio's
//! threads; heavy picture work runs on blocking worker threads. Routes not ported to Rust yet are
//! passed through unchanged to the Python Studio (STUDIO_PY_URL), so everything keeps working
//! while pieces move over one at a time.

mod brand;
mod immich;
mod jobs;
mod proxy;
mod routes;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::Router;
use tower_http::services::ServeDir;

pub struct Config {
    pub immich_url: String,
    pub api_key: Option<String>,
    pub py_url: Option<String>,
    pub data_dir: PathBuf,
    pub static_dir: PathBuf,
    pub graphite_dir: PathBuf,
    pub downloads_dir: PathBuf,
    pub brand_default_dir: PathBuf,
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
            py_url: var("STUDIO_PY_URL"),
            data_dir: path("STUDIO_DATA_DIR", "/data"),
            static_dir: path("STUDIO_STATIC_DIR", "/srv/app/static"),
            graphite_dir: path("STUDIO_GRAPHITE_DIR", "/graphite"),
            downloads_dir: path("STUDIO_DOWNLOADS_DIR", "/downloads"),
            brand_default_dir: path("STUDIO_BRAND_DEFAULT_DIR", "/srv/brand-default"),
            public_url: var("GIMMICH_PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string()),
            camera_album: var("STUDIO_CAMERA_ALBUM").unwrap_or_else(|| "Studio Camera".into()),
        }
    }
}

pub struct AppState {
    pub cfg: Config,
    pub http: reqwest::Client,
    pub jobs: jobs::Jobs,
}

pub type State = Arc<AppState>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
    ).init();
    let cfg = Config::from_env();
    std::fs::create_dir_all(&cfg.data_dir)?;
    brand::seed_default(&cfg)?;

    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(600)).build()?;
    let state: State = Arc::new(AppState { cfg, http, jobs: jobs::Jobs::default() });

    let app = Router::new()
        .merge(routes::router())
        .nest_service("/graphite", ServeDir::new(&state.cfg.graphite_dir).append_index_html_on_directories(true))
        .fallback_service(ServeDir::new(&state.cfg.static_dir).append_index_html_on_directories(true))
        .with_state(state.clone())
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024 * 1024));

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(
        "Gimmich Studio (Rust) on {addr}; not-yet-ported routes -> {}",
        state.cfg.py_url.as_deref().unwrap_or("(none)")
    );
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}
