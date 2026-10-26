//! Label photos: environment, people, era (year), age, clothing.
//!
//! Each label group works in one of two ways:
//!   * zero-shot: compare the photo with describing sentences (works with no training)
//!   * trained:   a small model learned from examples (Places365, or your own
//!                "Train: <Group>/<Label>" albums in Immich) - used when available

use std::{collections::HashMap, path::PathBuf, sync::RwLock};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::clip::{dot, normalize, Clip};

/// label -> describing sentences (same words as the Python Studio).
const ENVIRONMENT: &[(&str, &[&str])] = &[
    ("Beach", &["a photo of a sandy beach", "a beach by the sea"]),
    ("Sea", &["a photo of the open ocean", "ocean waves"]),
    ("Mountains", &["a photo of mountains", "a mountain landscape", "snowy mountain peaks"]),
    ("Forest", &["a photo of a forest", "trees in a forest"]),
    ("Lake or river", &["a photo of a lake", "a photo of a river"]),
    ("Waterfall", &["a photo of a waterfall"]),
    ("Desert", &["a photo of a desert"]),
    ("Snow", &["a snowy landscape", "a ski slope"]),
    ("Countryside", &["a photo of farmland and fields", "countryside fields"]),
    ("Park or garden", &["a photo of a park", "a garden"]),
    ("City street", &["a photo of a city street", "downtown buildings"]),
    ("Road", &["a photo of a highway", "an open road"]),
    ("Bridge", &["a photo of a bridge"]),
    ("Living room", &["a photo of a living room"]),
    ("Bedroom", &["a photo of a bedroom"]),
    ("Kitchen", &["a photo of a kitchen"]),
    ("Bathroom", &["a photo of a bathroom"]),
    ("Dining room", &["a photo of a dining room"]),
    ("Office", &["a photo of an office"]),
    ("Classroom", &["a photo of a classroom"]),
    ("Restaurant or cafe", &["a photo of a restaurant", "a cafe"]),
    ("Bar or pub", &["a photo of a bar", "a pub"]),
    ("Shop or mall", &["a photo of a shop", "a shopping mall", "a supermarket"]),
    ("Place of worship", &["a photo of a church", "a temple", "a mosque"]),
    ("Castle or palace", &["a photo of a castle", "a palace"]),
    ("Museum or gallery", &["a photo of a museum", "an art gallery"]),
    ("Hospital", &["a photo of a hospital room"]),
    ("Prison", &["a photo of a prison cell", "a jail cell with bars"]),
    ("Airport or plane", &["a photo of an airport", "inside an airplane"]),
    ("Train or station", &["a photo of a train station", "inside a train"]),
    ("Inside a car", &["a photo taken inside a car"]),
    ("Stadium or sports field", &["a photo of a stadium", "a sports field"]),
    ("Gym", &["a photo of a gym"]),
    ("Swimming pool", &["a photo of a swimming pool"]),
    ("Concert or stage", &["a photo of a stage", "a concert"]),
    ("Playground", &["a photo of a playground"]),
    ("Amusement park", &["a photo of an amusement park"]),
    ("Harbor or boat", &["a photo of a harbor", "on a boat"]),
    ("Campsite", &["a photo of a campsite with tents"]),
    ("Cemetery", &["a photo of a cemetery"]),
    ("Construction site", &["a photo of a construction site"]),
    ("Library", &["a photo of a library", "a bookstore"]),
];

const ERAS: &[&str] = &["1900s", "1910s", "1920s", "1930s", "1940s", "1950s", "1960s", "1970s", "1980s", "1990s",
    "2000s", "2010s", "2020s"];
const CLOTHING: &[(&str, &str)] = &[
    ("T-shirt", "a t-shirt"), ("Shirt", "a button-up shirt"), ("Suit", "a suit and tie"), ("Dress", "a dress"),
    ("Jacket", "a jacket"), ("Coat", "a winter coat"), ("Hoodie", "a hoodie"), ("Sweater", "a sweater"),
    ("Jeans", "jeans"), ("Shorts", "shorts"), ("Skirt", "a skirt"), ("Swimwear", "swimwear"),
    ("Uniform", "a uniform"), ("Sportswear", "sportswear"), ("Traditional dress", "traditional clothing"),
    ("Wedding dress", "a wedding dress"), ("Hat", "a hat"), ("Glasses", "glasses"), ("Sunglasses", "sunglasses"),
    ("Scarf", "a scarf"),
];
const COLORS: &[&str] = &["Black", "White", "Grey", "Red", "Blue", "Green", "Yellow", "Orange", "Pink", "Purple",
    "Brown", "Beige"];

/// Every label group, in order, with each label's describing sentences.
pub fn groups() -> Vec<(&'static str, Vec<(String, Vec<String>)>)> {
    let own = |l: &[(&str, &[&str])]| l.iter().map(|(n, p)| (n.to_string(), p.iter().map(|s| s.to_string()).collect())).collect();
    vec![
        ("Environment", own(ENVIRONMENT)),
        ("Setting", own(&[("Indoors", &["a photo taken indoors"]), ("Outdoors", &["a photo taken outdoors"])])),
        ("Light", own(&[("Daytime", &["a photo taken in daylight"]), ("Night", &["a photo taken at night"]),
                        ("Sunset", &["a photo of a sunset or sunrise"])])),
        ("People", own(&[("No people", &["a photo with no people in it", "an empty scene"]),
                         ("One person", &["a photo of one person", "a portrait of a single person"]),
                         ("Two people", &["a photo of two people", "a couple"]),
                         ("Group", &["a photo of a small group of people", "a family photo"]),
                         ("Crowd", &["a photo of a big crowd of people"])])),
        ("Era", ERAS.iter().map(|d| (d.to_string(), vec![format!("a photo from the {d}"), format!("a {d} photograph")])).collect()),
        ("Clothing", CLOTHING.iter().map(|(c, a)| (c.to_string(), vec![format!("a person wearing {a}")])).collect()),
        ("Clothing color", COLORS.iter().map(|c| (c.to_string(), vec![format!("a person wearing {} clothes", c.to_lowercase())])).collect()),
        ("Clothing style", own(&[("Formal", &["a person in formal clothes"]), ("Casual", &["a person in casual clothes"]),
                                 ("Sporty", &["a person in sports clothes"]), ("Work uniform", &["a person in a work uniform"]),
                                 ("Traditional", &["a person in traditional clothes"])])),
    ]
}

/// Groups where a photo can have several answers (e.g. jeans AND a jacket).
pub fn multi(group: &str) -> Option<usize> {
    match group {
        "Clothing" => Some(3),
        "Clothing color" => Some(2),
        _ => None,
    }
}

/// Groups that only make sense when a person is in the photo.
const NEEDS_PERSON: &[&str] = &["Clothing", "Clothing color", "Clothing style"];

/// A trained head: logistic regression on the image numbers (softmax over labels; one row = two labels).
#[derive(Clone, Serialize, Deserialize)]
pub struct Head {
    pub classes: Vec<String>,
    pub coef: Vec<Vec<f32>>,
    pub intercept: Vec<f32>,
}

impl Head {
    pub fn predict_proba(&self, x: &[f32]) -> Vec<f32> {
        let z: Vec<f32> = self.coef.iter().zip(&self.intercept).map(|(w, b)| dot(w, x) + b).collect();
        if z.len() == 1 {
            let p = 1.0 / (1.0 + (-z[0]).exp());
            return vec![1.0 - p, p];
        }
        softmax(&z)
    }
}

fn softmax(z: &[f32]) -> Vec<f32> {
    let m = z.iter().cloned().fold(f32::MIN, f32::max);
    let e: Vec<f32> = z.iter().map(|v| (v - m).exp()).collect();
    let s: f32 = e.iter().sum();
    e.into_iter().map(|v| v / s).collect()
}

pub type Picked = Vec<(String, f32)>;

pub struct Labeler {
    prompts: Vec<(&'static str, Vec<String>, Vec<Vec<f32>>)>,
    pub trained: RwLock<HashMap<String, Head>>,
    pub heads_path: PathBuf,
}

impl Labeler {
    pub async fn load(clip: &Clip, model_dir: &std::path::Path) -> Result<Self> {
        let mut prompts = vec![];
        for (group, labels) in groups() {
            let mut names = vec![];
            let mut vecs = vec![];
            for (name, sentences) in labels {
                let mut mean = vec![];
                for s in &sentences {
                    let v = clip.text(s, None).await?;
                    if mean.is_empty() { mean = vec![0.0; v.len()]; }
                    mean.iter_mut().zip(&v).for_each(|(m, x)| *m += x / sentences.len() as f32);
                }
                names.push(name);
                vecs.push(normalize(mean));
            }
            prompts.push((group, names, vecs));
        }
        let heads_path = model_dir.join("label_heads.json");
        let trained = std::fs::read(&heads_path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
        Ok(Self { prompts, trained: RwLock::new(trained), heads_path })
    }

    fn zero_shot(&self, group: &str, emb: &[f32]) -> Vec<(String, f32)> {
        let Some((_, names, vecs)) = self.prompts.iter().find(|p| p.0 == group) else { return vec![] };
        let logits: Vec<f32> = vecs.iter().map(|v| 100.0 * dot(v, emb)).collect();
        names.iter().cloned().zip(softmax(&logits)).collect()
    }

    pub fn scores(&self, group: &str, emb: &[f32]) -> Vec<(String, f32)> {
        let zs = self.zero_shot(group, emb);
        let trained = self.trained.read().unwrap();
        let Some(head) = trained.get(group) else { return zs };
        let t: Vec<(String, f32)> = head.classes.iter().cloned().zip(head.predict_proba(emb)).collect();
        // Labels the trained model never saw (e.g. Era "2020s") keep their zero-shot share;
        // the trained model splits the rest.
        let missing: Vec<(String, f32)> = zs.into_iter().filter(|(k, _)| !head.classes.contains(k)).collect();
        let rest = 1.0 - missing.iter().map(|(_, v)| v).sum::<f32>();
        t.into_iter().map(|(k, v)| (k, v * rest)).chain(missing).collect()
    }

    pub fn label(&self, emb: &[f32]) -> Vec<(&'static str, Picked)> {
        let mut out: Vec<(&'static str, Picked)> = vec![];
        for (group, _, _) in &self.prompts {
            let mut ranked = self.scores(group, emb);
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            let picked = if let Some(k) = multi(group) {
                let top = ranked[0].1;
                ranked.into_iter().take(k).filter(|(_, s)| *s >= top * 0.5).collect()
            } else {
                // With many choices the winner gets a smaller share, so the bar is set relative
                // to guessing: it must be at least 4x more likely than a random pick (max 35%).
                let bar = (4.0 / ranked.len() as f32).min(0.35);
                ranked.into_iter().take(1).filter(|(_, s)| *s >= bar).collect()
            };
            out.push((group, picked));
        }
        let has_person = out.iter().find(|(g, _)| *g == "People")
            .and_then(|(_, p)| p.first()).is_some_and(|(n, _)| n != "No people");
        if !has_person {
            for (g, p) in out.iter_mut() {
                if NEEDS_PERSON.contains(g) { p.clear(); }
            }
        }
        out
    }
}
