//! Small client for the Immich server API, acting as the logged-in visitor.

use anyhow::{anyhow, Result};
use bytes::Bytes;
use reqwest::{header, Client, Method, RequestBuilder};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Immich {
    http: Client,
    base: String,
    auth: Auth,
}

#[derive(Clone)]
enum Auth {
    Token(String),
    ApiKey(String),
}

impl Immich {
    pub fn with_token(http: Client, base: &str, token: &str) -> Self {
        Self { http, base: format!("{}/api", base.trim_end_matches('/')), auth: Auth::Token(token.into()) }
    }

    pub fn with_api_key(http: Client, base: &str, key: &str) -> Self {
        Self { http, base: format!("{}/api", base.trim_end_matches('/')), auth: Auth::ApiKey(key.into()) }
    }

    fn req(&self, method: Method, path: &str) -> RequestBuilder {
        let r = self.http.request(method, format!("{}{}", self.base, path));
        match &self.auth {
            Auth::Token(t) => r.header(header::AUTHORIZATION, format!("Bearer {t}")),
            Auth::ApiKey(k) => r.header("x-api-key", k),
        }
    }

    async fn send(&self, r: RequestBuilder, what: &str) -> Result<reqwest::Response> {
        let resp = r.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let text: String = text.chars().take(300).collect();
            return Err(ImmichError { status: status.as_u16(), message: format!("Immich {what} -> {status}: {text}") }.into());
        }
        Ok(resp)
    }

    pub async fn get_json(&self, path: &str) -> Result<Value> {
        Ok(self.send(self.req(Method::GET, path), path).await?.json().await?)
    }

    pub async fn send_json(&self, method: Method, path: &str, body: &Value) -> Result<Value> {
        let resp = self.send(self.req(method, path).json(body), path).await?;
        Ok(resp.json().await.unwrap_or(Value::Null))
    }

    pub async fn get_bytes(&self, path: &str) -> Result<Bytes> {
        Ok(self.send(self.req(Method::GET, path), path).await?.bytes().await?)
    }

    pub async fn me(&self) -> Result<Value> {
        self.get_json("/users/me").await
    }

    /// One page of photos/videos (Immich metadata search), plus the next page number.
    pub async fn assets(&self, page: u32, size: u32, filters: Value) -> Result<(Vec<Value>, Option<u32>)> {
        let mut body = json!({ "page": page, "size": size, "withExif": true });
        if let (Some(b), Some(f)) = (body.as_object_mut(), filters.as_object()) {
            b.extend(f.clone());
        }
        let d = self.send_json(Method::POST, "/search/metadata", &body).await?;
        let a = &d["assets"];
        let next = a["nextPage"].as_str().and_then(|s| s.parse().ok()).or(a["nextPage"].as_u64().map(|n| n as u32));
        Ok((a["items"].as_array().cloned().unwrap_or_default(), next))
    }

    pub async fn asset(&self, id: &str) -> Result<Value> {
        self.get_json(&format!("/assets/{id}")).await
    }

    pub async fn thumbnail(&self, id: &str, size: &str) -> Result<Bytes> {
        self.get_bytes(&format!("/assets/{id}/thumbnail?size={size}")).await
    }

    pub async fn original(&self, id: &str) -> Result<Bytes> {
        self.get_bytes(&format!("/assets/{id}/original")).await
    }

    pub async fn upload(&self, data: Bytes, filename: &str, created: Option<&str>) -> Result<String> {
        let now = chrono::Utc::now().to_rfc3339();
        let form = reqwest::multipart::Form::new()
            .part("assetData", reqwest::multipart::Part::stream(data).file_name(filename.to_string()))
            .text("deviceAssetId", format!("studio-{filename}-{now}"))
            .text("deviceId", "immich-studio")
            .text("fileCreatedAt", created.unwrap_or(&now).to_string())
            .text("fileModifiedAt", now.clone());
        let resp = self.send(self.req(Method::POST, "/assets").multipart(form), "/assets upload").await?;
        let v: Value = resp.json().await?;
        v["id"].as_str().map(str::to_string).ok_or_else(|| anyhow!("upload returned no id"))
    }

    pub async fn delete(&self, ids: &[String], forever: bool) -> Result<()> {
        self.send_json(Method::DELETE, "/assets", &json!({ "ids": ids, "force": forever })).await?;
        Ok(())
    }

    pub async fn albums(&self) -> Result<Vec<Value>> {
        Ok(self.get_json("/albums").await?.as_array().cloned().unwrap_or_default())
    }

    pub async fn ensure_album(&self, name: &str, asset_ids: &[String]) -> Result<String> {
        for a in self.albums().await? {
            if a["albumName"] == name {
                let id = a["id"].as_str().unwrap_or_default().to_string();
                if !asset_ids.is_empty() {
                    self.send_json(Method::PUT, &format!("/albums/{id}/assets"), &json!({ "ids": asset_ids })).await?;
                }
                return Ok(id);
            }
        }
        let v = self.send_json(Method::POST, "/albums", &json!({ "albumName": name, "assetIds": asset_ids })).await?;
        Ok(v["id"].as_str().unwrap_or_default().to_string())
    }
}

/// An error answer from Immich, keeping its HTTP status (e.g. 401 = not logged in).
#[derive(Debug)]
pub struct ImmichError {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for ImmichError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ImmichError {}

pub fn status_of(e: &anyhow::Error) -> Option<u16> {
    e.downcast_ref::<ImmichError>().map(|e| e.status)
}
