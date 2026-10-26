//! Studio API routes handled in Rust. Everything else under /api/ goes to the Python Studio.

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{brand, immich::{status_of, Immich}, proxy, State as AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/me", get(me))
        .route("/api/assets", get(assets))
        .route("/api/albums", get(albums))
        .route("/api/thumb/{id}", get(thumb))
        .route("/api/original/{id}", get(original))
        .route("/api/asset/{id}", delete(delete_asset))
        .route("/api/jobs/{id}", get(job))
        .route("/api/jobs/{id}/file", get(job_file))
        .route("/api/brand", get(brand_info).post(set_brand))
        .route("/api/brand/{name}", get(brand_file))
        .route("/api/connect", get(connect))
        .route("/api/downloads/{kind}/{name}", get(download))
        .route("/api/camera/upload", post(camera_upload))
        .route("/api/{*rest}", any(proxy::to_python))
}

// ------------------------------------------------------------------ errors ---
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "detail": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let code = status_of(&e).and_then(|s| StatusCode::from_u16(s).ok()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        ApiError(code, format!("{e:#}"))
    }
}

fn err(code: StatusCode, msg: &str) -> ApiError {
    ApiError(code, msg.into())
}

type ApiResult<T> = Result<T, ApiError>;

// ------------------------------------------------------------------- login ---
/// Act as the visitor: their Immich login cookie (same address as Immich), or an API key.
fn immich_for(st: &AppState, headers: &HeaderMap) -> ApiResult<Immich> {
    let cookie = headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok())
        .flat_map(|c| c.split(';')).map(str::trim)
        .find_map(|c| c.strip_prefix("immich_access_token=")).map(str::to_string);
    if let Some(t) = cookie {
        return Ok(Immich::with_token(st.http.clone(), &st.cfg.immich_url, &t));
    }
    let key = headers.get("x-api-key").and_then(|v| v.to_str().ok()).map(str::to_string).or(st.cfg.api_key.clone());
    key.map(|k| Immich::with_api_key(st.http.clone(), &st.cfg.immich_url, &k))
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Please log in to Immich first."))
}

// ------------------------------------------------------------------ photos ---
async fn health(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "ok": true, "engine": "rust", "graphite": st.cfg.graphite_dir.join("index.html").exists(),
                 "python_fallback": st.cfg.py_url.is_some() }))
}

async fn me(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    Ok(Json(immich_for(&st, &h)?.me().await?))
}

#[derive(Deserialize)]
struct AssetQuery {
    page: Option<u32>,
    size: Option<u32>,
    #[serde(rename = "type")]
    kind: Option<String>,
    album: Option<String>,
}

async fn assets(State(st): State<AppState>, h: HeaderMap, Query(q): Query<AssetQuery>) -> ApiResult<Json<Value>> {
    let mut filters = json!({});
    if let Some(k) = q.kind.filter(|k| !k.is_empty()) { filters["type"] = k.into(); }
    if let Some(a) = q.album.filter(|a| !a.is_empty()) { filters["albumIds"] = json!([a]); }
    let (items, next) = immich_for(&st, &h)?.assets(q.page.unwrap_or(1), q.size.unwrap_or(60), filters).await?;
    let items: Vec<Value> = items.iter().map(|a| json!({
        "id": a["id"], "name": a["originalFileName"], "type": a["type"],
        "tags": a["tags"].as_array().map(|t| t.iter().map(|x| x["value"].clone()).collect::<Vec<_>>()).unwrap_or_default(),
    })).collect();
    Ok(Json(json!({ "items": items, "next": next })))
}

async fn albums(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let list = immich_for(&st, &h)?.albums().await?;
    Ok(Json(list.iter().map(|a| json!({ "id": a["id"], "name": a["albumName"], "count": a["assetCount"] })).collect()))
}

#[derive(Deserialize)]
struct SizeQuery {
    size: Option<String>,
}

async fn thumb(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>, Query(q): Query<SizeQuery>) -> ApiResult<Response> {
    let size = q.size.filter(|s| s == "preview" || s == "fullsize").unwrap_or_else(|| "thumbnail".into());
    let data = immich_for(&st, &h)?.thumbnail(&id, &size).await?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "private, max-age=3600")], data).into_response())
}

async fn original(State(st): State<AppState>, Path(id): Path<String>, req: Request) -> ApiResult<Response> {
    let im = immich_for(&st, req.headers())?;
    let a = im.asset(&id).await?;
    let name = a["originalFileName"].as_str().unwrap_or("").to_lowercase();
    let simple = [".jpg", ".jpeg", ".png", ".webp"].iter().any(|e| name.ends_with(e));
    if !simple {
        // other formats (RAW, HEIC, PSD ...) are converted by the Python Studio until ported
        return Ok(proxy::to_python(State(st), req).await);
    }
    let media = a["originalMimeType"].as_str().unwrap_or("image/jpeg").to_string();
    Ok(([(header::CONTENT_TYPE, media)], im.original(&id).await?).into_response())
}

#[derive(Deserialize)]
struct DeleteQuery {
    forever: Option<bool>,
}

async fn delete_asset(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>, Query(q): Query<DeleteQuery>) -> ApiResult<Json<Value>> {
    let forever = q.forever.unwrap_or(false);
    immich_for(&st, &h)?.delete(&[id], forever).await?;
    Ok(Json(json!({ "ok": true, "trash": !forever })))
}

// -------------------------------------------------------------------- jobs ---
async fn job(State(st): State<AppState>, Path(id): Path<String>, req: Request) -> Response {
    match st.jobs.describe(&id) {
        Some(v) => Json(v).into_response(),
        None => proxy::to_python(State(st), req).await, // a job started by the Python Studio
    }
}

async fn job_file(State(st): State<AppState>, Path(id): Path<String>, req: Request) -> Response {
    match st.jobs.get(&id) {
        Some(j) => match (j.status, j.file) {
            ("done", Some((data, media, name))) => ([(header::CONTENT_TYPE, media),
                (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], data).into_response(),
            _ => StatusCode::NOT_FOUND.into_response(),
        },
        None => proxy::to_python(State(st), req).await,
    }
}

// ------------------------------------------------------------------- brand ---
async fn brand_info(State(st): State<AppState>) -> Json<Value> {
    Json(brand::info(&st.cfg)) // public: the login page shows the logo before anyone logs in
}

async fn brand_file(State(st): State<AppState>, Path(name): Path<String>) -> ApiResult<Response> {
    let media = brand::FILES.iter().find(|(f, _)| *f == name).map(|(_, m)| *m).ok_or_else(|| err(StatusCode::NOT_FOUND, "Not found"))?;
    let data = tokio::fs::read(brand::dir(&st.cfg).join(&name)).await.map_err(|_| err(StatusCode::NOT_FOUND, "Not found"))?;
    Ok(([(header::CONTENT_TYPE, media), (header::CACHE_CONTROL, "public, max-age=300")], data).into_response())
}

async fn set_brand(State(st): State<AppState>, h: HeaderMap, mut form: Multipart) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    if im.me().await?["isAdmin"] != true {
        return Err(err(StatusCode::FORBIDDEN, "Only the Immich admin can change the logo."));
    }
    let (mut file, mut name, mut reset) = (None, String::new(), false);
    while let Some(field) = form.next_field().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))? {
        match field.name() {
            Some("file") => {
                let fname = field.file_name().unwrap_or("logo.png").to_lowercase();
                file = Some((fname, field.bytes().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?));
            }
            Some("name") => name = field.text().await.unwrap_or_default(),
            Some("reset") => reset = true,
            _ => {}
        }
    }
    let d = brand::dir(&st.cfg);
    if reset {
        brand::reset(&d)?;
        return Ok(Json(json!({ "logo": null })));
    }
    std::fs::create_dir_all(&d).map_err(anyhow::Error::from)?;
    if let Some((fname, data)) = file {
        for old in ["logo.svg", "app-icon.svg"] {
            let _ = std::fs::remove_file(d.join(old));
        }
        let d2 = d.clone();
        tokio::task::spawn_blocking(move || if fname.ends_with(".svg") {
            brand::save_svg_logo(&d2, &data, None)
        } else {
            brand::save_raster_logo(&d2, &data)
        }).await.map_err(anyhow::Error::from)?.map_err(|e| err(StatusCode::UNSUPPORTED_MEDIA_TYPE, &format!("{e:#}")))?;
    }
    let name: String = name.trim().chars().take(40).collect();
    std::fs::write(d.join("brand.json"), json!({ "name": if name.is_empty() { Value::Null } else { name.into() } }).to_string())
        .map_err(anyhow::Error::from)?;
    Ok(Json(brand::info(&st.cfg)))
}

// ----------------------------------------------------------------- connect ---
const GITHUB_BUILDS: &str = "https://github.com/fablab503-collab/airbridge/actions/workflows/gimmich-desktop.yml";

async fn connect(State(st): State<AppState>, h: HeaderMap) -> Json<Value> {
    let address = st.cfg.public_url.clone().unwrap_or_else(|| {
        let proto = h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).unwrap_or("http");
        let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
        format!("{proto}://{host}")
    });
    let mut files = serde_json::Map::new();
    for kind in ["android", "desktop"] {
        if let Ok(rd) = std::fs::read_dir(st.cfg.downloads_dir.join(kind)) {
            let mut names: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.')).collect();
            names.sort();
            files.insert(kind.into(), names.into());
        }
    }
    Json(json!({ "address": address, "secure": address.starts_with("https://"), "files": files, "desktop_builds": GITHUB_BUILDS }))
}

async fn download(State(st): State<AppState>, Path((kind, name)): Path<(String, String)>) -> ApiResult<Response> {
    let bad = || err(StatusCode::NOT_FOUND, "Not found");
    if !["android", "desktop"].contains(&kind.as_str()) || name.contains('/') || name.contains('\\') || name.starts_with('.') {
        return Err(bad());
    }
    let path = st.cfg.downloads_dir.join(&kind).join(&name);
    let file = tokio::fs::File::open(&path).await.map_err(|_| bad())?;
    let media = if name.ends_with(".apk") { "application/vnd.android.package-archive" } else { "application/octet-stream" };
    let body = Body::from_stream(tokio_util_stream(file));
    Ok(([(header::CONTENT_TYPE, media.to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], body).into_response())
}

fn tokio_util_stream(file: tokio::fs::File) -> impl futures_util::Stream<Item = std::io::Result<bytes::Bytes>> {
    futures_util::stream::unfold(file, |mut f| async move {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 256 * 1024];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => { buf.truncate(n); Some((Ok(bytes::Bytes::from(buf)), f)) }
            Err(e) => Some((Err(e), f)),
        }
    })
}

// ------------------------------------------------------------------ camera ---
/// A photo or video straight from the Studio camera: into Immich, never kept on the phone.
async fn camera_upload(State(st): State<AppState>, h: HeaderMap, mut form: Multipart) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let (mut file, mut taken) = (None, None);
    while let Some(field) = form.next_field().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))? {
        match field.name() {
            Some("file") => {
                let name = field.file_name().unwrap_or("camera.jpg").to_string();
                file = Some((name, field.bytes().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?));
            }
            Some("taken") => taken = field.text().await.ok(),
            _ => {}
        }
    }
    let (name, data) = file.ok_or_else(|| err(StatusCode::BAD_REQUEST, "No file"))?;
    if data.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Empty file"));
    }
    let bytes = data.len();
    let id = im.upload(data, &name, taken.as_deref()).await?;
    im.ensure_album(&st.cfg.camera_album, &[id.clone()]).await?;
    Ok(Json(json!({ "asset": id, "album": st.cfg.camera_album, "bytes": bytes })))
}
