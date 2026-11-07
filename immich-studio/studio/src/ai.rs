//! AI routes: labels (environment, people, era, clothing, age, year), faces (fingerprints, age,
//! 584 face points), find a person at any age, and training on your own examples.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{anyhow, Result};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use image::RgbImage;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::sync::OnceCell;

use crate::{
    clip::{dot, Clip},
    faces::{Face, FacePipeline},
    formats,
    immich::Immich,
    labels::{multi, Labeler},
    learn,
    routes::{err, immich_for, ApiResult},
    tools::{blocking, load},
    State as AppState,
};

pub struct Ai {
    pub clip: Clip,
    labeler: OnceCell<Arc<Labeler>>,
    faces: OnceCell<Arc<FacePipeline>>,
    model_dir: PathBuf,
    trained_dir: PathBuf,
}

const AGE_GROUPS: &[(f32, &str)] = &[(3.0, "Baby (0-2)"), (13.0, "Child (3-12)"), (20.0, "Teen (13-19)"),
    (30.0, "Young adult (20-29)"), (45.0, "Adult (30-44)"), (65.0, "Middle age (45-64)"), (f32::MAX, "Senior (65+)")];

/// Tag families the Studio writes; re-labelling replaces these and keeps the user's own tags.
const STUDIO_TAG_ROOTS: &[&str] = &["Environment", "Setting", "Light", "People", "Era", "Clothing", "Clothing color",
    "Clothing style", "Age", "Year"];

// Similarity levels, measured on FG-NET people the model never saw (eval/README):
const CERTAIN: f32 = 0.72; // above the highest wrong-person score in ~282 000 wrong pairs: 0 mistakes seen
const LIKELY: f32 = 0.36; // about 1 wrong match in 100

pub fn age_group(age: f32) -> &'static str {
    AGE_GROUPS.iter().find(|(limit, _)| age < *limit).map(|(_, n)| *n).unwrap_or("Senior (65+)")
}

fn r(v: f32, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (v as f64 * m).round() / m
}

impl Ai {
    pub fn new(http: reqwest::Client, model_dir: PathBuf, trained_dir: PathBuf) -> Self {
        Self { clip: Clip::new(http), labeler: OnceCell::new(), faces: OnceCell::new(), model_dir, trained_dir }
    }

    /// Heavy AI models load the first time they are needed.
    async fn labeler(&self) -> Result<Arc<Labeler>> {
        Ok(self.labeler.get_or_try_init(|| async { Labeler::load(&self.clip, &self.model_dir).await.map(Arc::new) }).await?.clone())
    }

    async fn pipeline(&self) -> Result<Arc<FacePipeline>> {
        let dir = self.model_dir.clone();
        Ok(self.faces.get_or_try_init(|| async move { blocking(move || FacePipeline::load(&dir).map(Arc::new)).await }).await?.clone())
    }

    /// All faces in a picture, with our own age model when the AI server is reachable.
    pub async fn find_faces(&self, img: Arc<RgbImage>) -> Result<Vec<Face>> {
        let p = self.pipeline().await?;
        let p2 = p.clone();
        let mut faces = blocking(move || p2.analyze(&img, 0.6)).await?;
        if let Some(head) = &p.age_head {
            let mut ages = vec![];
            for f in &faces {
                let jpeg = formats::to_jpeg(&f.aligned, None, 95)?;
                match self.clip.image(jpeg, Some(&head.clip_model)).await {
                    Ok(e) => ages.push(head.predict(&e)),
                    Err(_) => break, // AI server unreachable: keep InsightFace's guess
                }
            }
            if ages.len() == faces.len() {
                faces.iter_mut().zip(ages).for_each(|(f, a)| f.age = a);
            }
        }
        Ok(faces)
    }
}

// ---------------------------------------------------------------- labelling ---
async fn analyze(st: &AppState, im: &Immich, id: &str, write: bool) -> Result<Value> {
    let ai = &st.ai;
    let (img, a) = load(im, id, false).await?;
    let img = Arc::new(img);
    let labeler = ai.labeler().await?;
    let emb = ai.clip.image(formats::to_jpeg(&img, Some(1024), 90)?, None).await?;
    let mut labels = labeler.label(&emb);
    let faces = ai.find_faces(img.clone()).await?;
    if !faces.is_empty() {
        // clothes are judged on each person's body area (face box stretched downwards)
        let (w, h) = (img.width() as f32, img.height() as f32);
        let mut votes: Vec<(&str, Vec<(String, f32)>)> = vec![];
        for f in &faces {
            let [x1, y1, x2, y2] = f.bbox;
            let (fw, fh) = (x2 - x1, y2 - y1);
            let (l, t) = ((x1 - fw).max(0.0) as u32, y1.max(0.0) as u32);
            let (rr, b) = ((x2 + fw).min(w) as u32, (y2 + fh * 5.0).min(h) as u32);
            if rr <= l + 40 || b <= t + 60 {
                continue;
            }
            let body = image::imageops::crop_imm(&*img, l, t, rr - l, b - t).to_image();
            let bemb = ai.clip.image(formats::to_jpeg(&body, Some(512), 90)?, None).await?;
            for g in ["Clothing", "Clothing color", "Clothing style"] {
                let pos = match votes.iter().position(|v| v.0 == g) {
                    Some(p) => p,
                    None => { votes.push((g, vec![])); votes.len() - 1 }
                };
                for (n, s) in labeler.scores(g, &bemb) {
                    let list = &mut votes[pos].1;
                    match list.iter_mut().find(|(k, _)| *k == n) {
                        Some(e) => e.1 += s / faces.len() as f32,
                        None => list.push((n, s / faces.len() as f32)),
                    }
                }
            }
        }
        for (g, mut ranked) in votes {
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            let top = ranked[0].1;
            let picked = ranked.into_iter().take(multi(g).unwrap_or(1)).filter(|(_, s)| *s >= top * 0.5).collect();
            if let Some(e) = labels.iter_mut().find(|(k, _)| *k == g) { e.1 = picked; }
        }
        let n = faces.len();
        let people = if n == 1 { "One person" } else if n == 2 { "Two people" } else if n <= 6 { "Group" } else { "Crowd" };
        if let Some(e) = labels.iter_mut().find(|(k, _)| *k == "People") { e.1 = vec![(people.into(), 1.0)]; }
    }
    let exif = &a["exifInfo"];
    // Without camera details Immich falls back to the file date, which is not when the photo was taken
    let has_camera = exif["make"].as_str().is_some_and(|s| !s.is_empty()) || exif["model"].as_str().is_some_and(|s| !s.is_empty());
    let year: Option<i64> = exif["dateTimeOriginal"].as_str().filter(|_| has_camera).and_then(|t| t.get(..4)?.parse().ok());
    let face_out: Vec<Value> = faces.iter().map(|f| json!({
        "box": f.bbox.map(|v| v.round() as i64), "age": r(f.age, 1), "age_group": age_group(f.age),
        "sex": f.sex, "mesh_points": f.point_count(),
    })).collect();
    let mut label_json = Map::new();
    for (g, v) in &labels {
        label_json.insert(g.to_string(), v.iter().map(|(n, s)| json!([n, r(*s, 3)])).collect());
    }
    let mut result = json!({ "asset": id, "labels": label_json, "faces": face_out, "year_from_camera": year });
    if write {
        let mut tags: Vec<String> = labels.iter().flat_map(|(g, v)| v.iter().map(move |(n, _)| format!("{g}/{n}"))).collect();
        tags.extend(faces.iter().map(|f| format!("Age/{}", age_group(f.age))));
        if let Some(y) = year { tags.push(format!("Year/{y}")); }
        tags.sort();
        tags.dedup();
        for prefix in STUDIO_TAG_ROOTS {
            im.remove_tag_prefix(&a, &format!("{prefix}/")).await?;
        }
        let ids: Vec<String> = im.upsert_tags(&tags).await?.into_iter().map(|(_, id)| id).collect();
        im.tag_assets(&ids, &[id.to_string()]).await?;
        let mut summary = labels.iter().filter(|(_, v)| !v.is_empty())
            .map(|(g, v)| format!("{g}: {}", v.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")))
            .collect::<Vec<_>>().join("; ");
        if !faces.is_empty() {
            summary += &format!("; Ages: {}", faces.iter().map(|f| format!("~{:.0}", f.age)).collect::<Vec<_>>().join(", "));
        }
        let old = exif["description"].as_str().unwrap_or("").split("\n\n[Immich Studio]").next().unwrap_or("").to_string();
        let sep = if old.is_empty() { "" } else { "\n\n" };
        im.set_description(id, &format!("{old}{sep}[Immich Studio] {summary}")).await?;
        result["tags"] = tags.into();
    }
    Ok(result)
}

#[derive(Deserialize)]
pub struct WriteQuery {
    write: Option<bool>,
}

pub async fn analyze_one(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>, Query(q): Query<WriteQuery>) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    Ok(Json(analyze(&st, &im, &id, q.write.unwrap_or(true)).await?))
}

pub async fn analyze_all(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let s = st.clone();
    let job = st.jobs.start("analyze-all", move |job| async move {
        let photos = im.all_assets(json!({ "type": "IMAGE" })).await?;
        let mut done = 0;
        for (i, a) in photos.iter().enumerate() {
            match analyze(&s, &im, a["id"].as_str().unwrap_or_default(), true).await {
                Ok(_) => done += 1,
                Err(e) => tracing::warn!("analyze failed {}: {e:#}", a["id"]),
            }
            job.set((100 * (i + 1) / photos.len()) as u32);
        }
        Ok(json!({ "labelled": done, "total": photos.len() }))
    });
    Ok(Json(json!({ "job": job })))
}

/// Learn from albums named "Train: <Group>/<Label>" (e.g. "Train: Clothing/Uniform").
pub async fn train(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let s = st.clone();
    let job = st.jobs.start("train", move |_job| async move {
        let mut examples: Vec<(String, Vec<(String, String)>)> = vec![];
        for al in im.albums().await? {
            let name = al["albumName"].as_str().unwrap_or_default();
            let Some((group, label)) = name.strip_prefix("Train: ").and_then(|n| n.split_once('/')) else { continue };
            let (group, label) = (group.trim().to_string(), label.trim().to_string());
            let ids = im.album_asset_ids(al["id"].as_str().unwrap_or_default()).await?;
            let pos = match examples.iter().position(|e| e.0 == group) {
                Some(p) => p,
                None => { examples.push((group, vec![])); examples.len() - 1 }
            };
            examples[pos].1.extend(ids.into_iter().map(|id| (id, label.clone())));
        }
        let labeler = s.ai.labeler().await?;
        let mut heads = labeler.trained.read().unwrap().clone();
        let mut report = Map::new();
        for (group, ex) in examples {
            let mut names: Vec<&String> = ex.iter().map(|(_, l)| l).collect();
            names.sort();
            names.dedup();
            if names.len() < 2 {
                report.insert(group, "needs at least 2 labels".into());
                continue;
            }
            let names: Vec<String> = names.into_iter().cloned().collect();
            let mut x = vec![];
            for (id, _) in &ex {
                x.push(s.ai.clip.image(im.thumbnail(id, "preview").await?.to_vec(), None).await?);
            }
            let y: Vec<String> = ex.iter().map(|(_, l)| l.clone()).collect();
            let n = y.len();
            let head = blocking(move || learn::logistic_regression(&x, &y, 5.0)).await?;
            heads.insert(group.clone(), head);
            report.insert(group, json!({ "examples": n, "labels": names }));
        }
        std::fs::write(&labeler.heads_path, serde_json::to_vec(&heads)?)?;
        *labeler.trained.write().unwrap() = heads;
        Ok(Value::Object(report))
    });
    Ok(Json(json!({ "job": job })))
}

// -------------------------------------------------------------------- faces ---
pub async fn faces(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let (img, _) = load(&im, &id, false).await?;
    let (w, hh) = img.dimensions();
    let found = st.ai.find_faces(Arc::new(img)).await?;
    Ok(Json(json!({ "width": w, "height": hh, "faces": found.iter().map(|f| json!({
        "box": f.bbox, "age": r(f.age, 1), "age_group": age_group(f.age), "sex": f.sex, "score": r(f.score, 3),
        "mesh": f.mesh.as_ref().map(|m| m.iter().map(|p| [r(p[0], 1), r(p[1], 1)]).collect::<Vec<_>>()),
        "outline": f.points106.iter().map(|p| [r(p[0], 1), r(p[1], 1)]).collect::<Vec<_>>(),
        "points": f.point_count(),
    })).collect::<Vec<_>>() })))
}

/// One face index per Immich user: rows (photo:face, age) + fingerprints (+ raw ones for learning).
struct FaceIndex {
    rows: Vec<(String, f32)>,
    embs: Vec<Vec<f32>>,
    raws: Vec<Vec<f32>>,
}

fn f32s(v: &[Vec<f32>]) -> Vec<u8> {
    v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect()
}

fn read_f32s(data: &[u8], dim: usize) -> Vec<Vec<f32>> {
    let all: Vec<f32> = data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    all.chunks(dim.max(1)).map(|c| c.to_vec()).collect()
}

async fn user_file(st: &AppState, im: &Immich, kind: &str, ext: &str) -> Result<PathBuf> {
    let uid = im.me().await?["id"].as_str().unwrap_or("me").to_string();
    Ok(st.cfg.data_dir.join(format!("{kind}-{uid}.{ext}")))
}

impl FaceIndex {
    fn save(&self, base: &std::path::Path) -> Result<()> {
        let dim = self.embs.first().map_or(0, Vec::len);
        let raw_dim = self.raws.first().map_or(0, Vec::len);
        std::fs::write(base.with_extension("f32"), f32s(&self.embs))?;
        std::fs::write(base.with_extension("raw.f32"), f32s(&self.raws))?;
        std::fs::write(base, serde_json::to_vec(&json!({ "dim": dim, "raw_dim": raw_dim, "rows": self.rows }))?)?;
        Ok(())
    }

    fn load(base: &std::path::Path) -> Option<Self> {
        let meta: Value = serde_json::from_slice(&std::fs::read(base).ok()?).ok()?;
        let rows: Vec<(String, f32)> = serde_json::from_value(meta["rows"].clone()).ok()?;
        let embs = read_f32s(&std::fs::read(base.with_extension("f32")).ok()?, meta["dim"].as_u64()? as usize);
        let raws = read_f32s(&std::fs::read(base.with_extension("raw.f32")).ok()?, meta["raw_dim"].as_u64()? as usize);
        Some(Self { rows, embs, raws })
    }
}

/// Fingerprint every face in the library (stronger model + age-ignoring layer).
pub async fn index_faces(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let path = user_file(&st, &im, "faces", "json").await?;
    let s = st.clone();
    let job = st.jobs.start("index-faces", move |job| async move {
        let photos = im.all_assets(json!({ "type": "IMAGE" })).await?;
        let mut idx = FaceIndex { rows: vec![], embs: vec![], raws: vec![] };
        for (i, a) in photos.iter().enumerate() {
            let id = a["id"].as_str().unwrap_or_default();
            let found = async {
                let (img, _) = load(&im, id, false).await?;
                s.ai.find_faces(Arc::new(img)).await
            }.await;
            match found {
                Ok(faces) => for (n, f) in faces.into_iter().enumerate() {
                    idx.rows.push((format!("{id}:{n}"), f.age));
                    idx.embs.push(f.embedding);
                    idx.raws.push(f.raw_embedding);
                },
                Err(e) => tracing::warn!("face index failed {id}: {e:#}"),
            }
            job.set((100 * (i + 1) / photos.len()) as u32);
        }
        idx.save(&path)?;
        Ok(json!({ "photos": photos.len(), "faces": idx.rows.len() }))
    });
    Ok(Json(json!({ "job": job })))
}

#[derive(Default, serde::Serialize, Deserialize)]
struct Confirmations {
    same: Vec<[String; 2]>,
    different: Vec<[String; 2]>,
}

async fn confirmations(st: &AppState, im: &Immich) -> Result<(Confirmations, PathBuf)> {
    let path = user_file(st, im, "confirmed", "json").await?;
    let c = std::fs::read(&path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
    Ok((c, path))
}

/// Join confirmed pairs into people: if A=B and B=C then A=C.
fn same_groups(pairs: &[[String; 2]]) -> HashMap<String, String> {
    let mut parent: HashMap<String, String> = HashMap::new();
    fn root(p: &mut HashMap<String, String>, k: &str) -> String {
        let mut k = k.to_string();
        p.entry(k.clone()).or_insert_with(|| k.clone());
        while p[&k] != k {
            let up = p[&p[&k]].clone();
            p.insert(k.clone(), up.clone());
            k = up;
        }
        k
    }
    for [a, b] in pairs {
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        parent.insert(ra, rb);
    }
    let keys: Vec<String> = parent.keys().cloned().collect();
    keys.into_iter().map(|k| { let r = root(&mut parent, &k); (k, r) }).collect()
}

/// Photos of the same person, best match first - also across many years of age difference.
pub async fn find_person(State(st): State<AppState>, h: HeaderMap, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let idx = FaceIndex::load(&user_file(&st, &im, "faces", "json").await?)
        .ok_or_else(|| err(StatusCode::CONFLICT, "Build the face index first (Label & train → Index all faces)."))?;
    let asset = body["asset_id"].as_str().ok_or_else(|| err(StatusCode::BAD_REQUEST, "asset_id is missing"))?.to_string();
    let n = body["face"].as_u64().unwrap_or(0) as usize;
    let (img, _) = load(&im, &asset, false).await?;
    let faces = st.ai.find_faces(Arc::new(img)).await?;
    let face = faces.get(n).ok_or_else(|| err(StatusCode::NOT_FOUND, "No such face in this photo"))?;
    let me_key = format!("{asset}:{n}");
    let (conf, _) = confirmations(&st, &im).await?;
    let groups = same_groups(&conf.same);
    let rejected: Vec<[String; 2]> = conf.different.iter().map(|p| { let mut p = p.clone(); p.sort(); p }).collect();
    let mut best: HashMap<String, (f32, String, f32)> = HashMap::new();
    for ((key, age), e) in idx.rows.iter().zip(&idx.embs) {
        let s = dot(e, &face.embedding);
        let aid = key.split(':').next().unwrap_or_default().to_string();
        if aid != asset && best.get(&aid).is_none_or(|b| s > b.0) {
            best.insert(aid, (s, key.clone(), *age));
        }
    }
    let mut ranked: Vec<(String, (f32, String, f32))> = best.into_iter().collect();
    ranked.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0));
    ranked.truncate(body["limit"].as_u64().unwrap_or(30) as usize);
    let level = |key: &str, s: f32| -> &'static str {
        if groups.get(&me_key).is_some_and(|g| groups.get(key) == Some(g)) {
            return "confirmed"; // you said so
        }
        let mut pair = [me_key.clone(), key.to_string()];
        pair.sort();
        if rejected.contains(&pair) {
            return "rejected";
        }
        if s >= CERTAIN { "certain" } else if s >= LIKELY { "likely" } else { "unsure" }
    };
    let matches: Vec<Value> = ranked.iter().map(|(aid, (s, key, age))| {
        let lv = level(key, *s);
        json!({ "asset": aid, "face": key, "similarity": r(*s, 3), "age": r(*age, 1), "level": lv,
                "likely_same": matches!(lv, "confirmed" | "certain" | "likely") })
    }).collect();
    Ok(Json(json!({ "age_here": r(face.age, 1), "face": me_key, "matches": matches })))
}

/// Remember your answer: are these two faces the same person? ({"a": key, "b": key, "same": bool})
pub async fn confirm_face(State(st): State<AppState>, h: HeaderMap, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let (mut conf, path) = confirmations(&st, &im).await?;
    let (a, b) = (body["a"].as_str(), body["b"].as_str());
    let (Some(a), Some(b)) = (a, b) else { return Err(err(StatusCode::BAD_REQUEST, "a and b are needed")) };
    let mut pair = [a.to_string(), b.to_string()];
    pair.sort();
    let same_pair = |p: &[String; 2]| { let mut q = p.clone(); q.sort(); q == pair };
    conf.same.retain(|p| !same_pair(p));
    conf.different.retain(|p| !same_pair(p));
    if body["same"].as_bool().unwrap_or(false) { conf.same.push(pair) } else { conf.different.push(pair) }
    std::fs::write(&path, serde_json::to_vec(&conf).map_err(anyhow::Error::from)?).map_err(anyhow::Error::from)?;
    Ok(Json(json!({ "same": conf.same.len(), "different": conf.different.len() })))
}

/// Retrain the age-ignoring layer on FG-NET plus the people you confirmed.
pub async fn learn_faces(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let index_path = user_file(&st, &im, "faces", "json").await?;
    let s = st.clone();
    let job = st.jobs.start("learn-faces", move |_job| async move {
        let idx = FaceIndex::load(&index_path).ok_or_else(|| anyhow!("Build the face index first (Label & train → Index all faces)."))?;
        let (conf, _) = confirmations(&s, &im).await?;
        let groups = same_groups(&conf.same);
        let mine: Vec<(usize, String)> = idx.rows.iter().enumerate()
            .filter_map(|(i, (k, _))| groups.get(k).map(|g| (i, g.clone()))).collect();
        let dir = [&s.ai.trained_dir, &s.ai.model_dir].into_iter().find(|d| d.join("fgnet_antelopev2.json").exists())
            .ok_or_else(|| anyhow!("FG-NET training faces are missing"))?;
        let meta: Value = serde_json::from_slice(&std::fs::read(dir.join("fgnet_antelopev2.json"))?)?;
        let mut x = read_f32s(&std::fs::read(dir.join("fgnet_antelopev2.f32"))?, meta["dim"].as_u64().unwrap_or(512) as usize);
        let mut y: Vec<String> = serde_json::from_value(meta["people"].clone())?;
        for (i, g) in &mine {
            x.push(idx.raws[*i].clone());
            y.push(format!("you:{g}"));
        }
        let layer = blocking(move || learn::age_invariant(&x, &y, 200, 0.3)).await?;
        let pipeline = s.ai.pipeline().await?;
        std::fs::write(&pipeline.invariant_path, serde_json::to_vec(&layer)?)?;
        *pipeline.age_invariant.write().unwrap() = Some(layer);
        let people: std::collections::HashSet<&String> = mine.iter().map(|(_, g)| g).collect();
        Ok(json!({ "your_faces": mine.len(), "your_people": people.len(),
                   "note": "Run 'Index all faces' again so the whole library uses the new layer." }))
    });
    Ok(Json(json!({ "job": job })))
}
