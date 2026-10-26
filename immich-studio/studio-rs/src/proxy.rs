//! Pass-through to the Python Studio for routes that are not ported to Rust yet.

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::TryStreamExt;

use crate::State as AppState;

pub async fn to_python(State(st): State<AppState>, req: Request) -> Response {
    let Some(base) = st.cfg.py_url.clone() else {
        return (StatusCode::NOT_IMPLEMENTED, "This Studio tool is not available in this build.").into_response();
    };
    let path = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/").to_string();
    let method = req.method().clone();
    let mut headers = HeaderMap::new();
    for (k, v) in req.headers() {
        if k != "host" && k != "content-length" {
            headers.insert(k.clone(), v.clone());
        }
    }
    let body = reqwest::Body::wrap_stream(req.into_body().into_data_stream());
    let upstream = st.http.request(method, format!("{}{}", base.trim_end_matches('/'), path)).headers(headers).body(body);
    match upstream.send().await {
        Ok(resp) => {
            let mut out = Response::builder().status(resp.status().as_u16());
            for (k, v) in resp.headers() {
                if k != "transfer-encoding" && k != "connection" {
                    out = out.header(k, v);
                }
            }
            out.body(Body::from_stream(resp.bytes_stream().map_err(std::io::Error::other)))
                .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
        }
        Err(e) => (StatusCode::BAD_GATEWAY, format!("Python Studio unreachable: {e}")).into_response(),
    }
}
