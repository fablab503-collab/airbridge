//! Talk to Immich's machine-learning server to turn images and text into numbers.
//!
//! Images and sentences that mean the same thing get similar numbers, which lets us
//! label a photo by comparing it with sentences like "a photo of a beach".

use std::{collections::HashMap, sync::Mutex};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub struct Clip {
    http: reqwest::Client,
    url: String,
    pub model: String,
    texts: Mutex<HashMap<(String, String), Vec<f32>>>,
}

pub fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

impl Clip {
    pub fn new(http: reqwest::Client) -> Self {
        let var = |k: &str, d: &str| std::env::var(k).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| d.into());
        Self {
            http,
            url: var("IMMICH_ML_URL", "http://immich-machine-learning:3003"),
            model: var("STUDIO_CLIP_MODEL", "ViT-B-16-SigLIP2__webli"),
            texts: Mutex::default(),
        }
    }

    async fn predict(&self, form: reqwest::multipart::Form) -> Result<Vec<f32>> {
        let resp = self.http.post(format!("{}/predict", self.url)).multipart(form).send().await
            .map_err(|e| anyhow!("AI server unreachable: {e}"))?;
        if !resp.status().is_success() {
            return Err(anyhow!("AI server -> {}: {}", resp.status(), resp.text().await.unwrap_or_default()));
        }
        let v: Value = resp.json().await?;
        // Immich answers {"clip": "[0.1, 0.2, ...]"} (the numbers as a JSON string)
        let nums: Vec<f32> = match &v["clip"] {
            Value::String(s) => serde_json::from_str(s)?,
            other => serde_json::from_value(other.clone())?,
        };
        Ok(normalize(nums))
    }

    pub async fn text(&self, text: &str, model: Option<&str>) -> Result<Vec<f32>> {
        let model = model.unwrap_or(&self.model).to_string();
        let key = (text.to_string(), model.clone());
        if let Some(v) = self.texts.lock().unwrap().get(&key) {
            return Ok(v.clone());
        }
        let entries = json!({ "clip": { "textual": { "modelName": model } } });
        let form = reqwest::multipart::Form::new().text("entries", entries.to_string()).text("text", text.to_string());
        let v = self.predict(form).await?;
        self.texts.lock().unwrap().insert(key, v.clone());
        Ok(v)
    }

    pub async fn image(&self, jpeg: Vec<u8>, model: Option<&str>) -> Result<Vec<f32>> {
        let entries = json!({ "clip": { "visual": { "modelName": model.unwrap_or(&self.model) } } });
        let part = reqwest::multipart::Part::bytes(jpeg).file_name("image.jpg").mime_str("image/jpeg")?;
        self.predict(reqwest::multipart::Form::new().text("entries", entries.to_string()).part("image", part)).await
    }
}
