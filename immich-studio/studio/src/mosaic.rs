//! Photo mosaic: rebuild one photo out of many small photos from your library, like a giant puzzle.
//!
//! Styles: "classic" (photos nearly as they are), "blend" (each tile's colours moved toward the colour
//! it stands for, so the big picture shows more clearly), "puzzle" (blend + jigsaw piece outlines) and "bubbles"
//! (round tiles on a dark background).

use std::sync::Arc;

use anyhow::{bail, Result};
use axum::{extract::State, http::HeaderMap, Json};
use futures_util::{stream, StreamExt};
use image::{imageops, Rgb, RgbImage, Rgba};
use serde_json::{json, Value};

use crate::{
    formats,
    routes::{err, immich_for, ApiResult},
    tools::{blocking, load},
    State as AppState,
};

/// Average colour of each quarter of a square (top-left, top-right, bottom-left, bottom-right):
/// 12 numbers, so a tile that is dark on top and bright below lands where the picture is like that.
fn signature(img: &RgbImage, x0: u32, y0: u32, w: u32, h: u32) -> [f32; 12] {
    let mut sig = [0f32; 12];
    for q in 0..4 {
        let (qx, qy) = (x0 + (q % 2) * w / 2, y0 + (q / 2) * h / 2);
        let (qw, qh) = ((w / 2).max(1), (h / 2).max(1));
        let mut sum = [0f64; 3];
        let mut n = 0f64;
        for y in qy..(qy + qh).min(img.height()) {
            for x in qx..(qx + qw).min(img.width()) {
                let p = img.get_pixel(x, y);
                for c in 0..3 {
                    sum[c] += p[c] as f64;
                }
                n += 1.0;
            }
        }
        for c in 0..3 {
            sig[q as usize * 3 + c] = (sum[c] / n.max(1.0)) as f32;
        }
    }
    sig
}

fn distance(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    // colours weighted a bit like the eye sees them (green counts most)
    const W: [f32; 3] = [0.30, 0.59, 0.11];
    a.iter().zip(b).enumerate().map(|(i, (x, y))| W[i % 3] * (x - y) * (x - y)).sum()
}

/// A square from the middle of the photo, scaled to `size`.
fn square(img: &RgbImage, size: u32) -> RgbImage {
    let s = img.width().min(img.height());
    let sq = imageops::crop_imm(img, (img.width() - s) / 2, (img.height() - s) / 2, s, s).to_image();
    imageops::resize(&sq, size, size, imageops::FilterType::Triangle)
}

/// Jigsaw outline drawn as an SVG and laid over the mosaic (each piece has a knob on every inner side).
fn puzzle_overlay(cols: u32, rows: u32, tile: u32) -> String {
    let t = tile as f32;
    let mut d = String::new();
    // one edge from (x0,y0) going along (dx,dy) for one tile, knob bulging to side `s` (+1/-1)
    let mut edge = |x0: f32, y0: f32, dx: f32, dy: f32, s: f32| {
        let (nx, ny) = (-dy * s, dx * s); // knob direction
        let p = |a: f32, b: f32| (x0 + dx * a * t + nx * b * t, y0 + dy * a * t + ny * b * t);
        let pts = [
            p(0.0, 0.0), p(0.35, 0.0), p(0.40, 0.0), p(0.38, 0.12), // neck in
            p(0.30, 0.26), p(0.42, 0.30), p(0.50, 0.30),             // round head, left half
            p(0.58, 0.30), p(0.70, 0.26), p(0.62, 0.12),             // right half
            p(0.60, 0.0), p(0.65, 0.0), p(1.0, 0.0),                 // neck out
        ];
        d += &format!("M{:.1},{:.1} L{:.1},{:.1} ", pts[0].0, pts[0].1, pts[1].0, pts[1].1);
        for c in pts[2..11].chunks(3) {
            d += &format!("C{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} ", c[0].0, c[0].1, c[1].0, c[1].1, c[2].0, c[2].1);
        }
        d += &format!("L{:.1},{:.1} ", pts[12].0, pts[12].1);
    };
    // knob side from a simple hash, so the pieces look mixed but stay the same every time
    let side = |a: u32, b: u32, c: u32| if (a.wrapping_mul(73856093) ^ b.wrapping_mul(19349663) ^ c.wrapping_mul(83492791)) % 2 == 0 { 1.0 } else { -1.0 };
    for r in 1..rows {
        for c in 0..cols {
            edge(c as f32 * t, r as f32 * t, 1.0, 0.0, side(r, c, 1));
        }
    }
    for c in 1..cols {
        for r in 0..rows {
            edge(c as f32 * t, r as f32 * t, 0.0, 1.0, side(r, c, 2));
        }
    }
    let (w, h) = (cols * tile, rows * tile);
    let stroke = (t / 28.0).max(1.0);
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<path d="{d}" fill="none" stroke="rgba(255,255,255,0.55)" stroke-width="{s2}" transform="translate(0.8,0.8)"/>
<path d="{d}" fill="none" stroke="rgba(20,20,20,0.75)" stroke-width="{stroke}"/>
<rect x="0" y="0" width="{w}" height="{h}" fill="none" stroke="rgba(20,20,20,0.75)" stroke-width="{s2}"/></svg>"#,
        s2 = stroke * 1.4)
}

pub struct Settings {
    pub style: String,
    pub cols: u32,
    pub tile: u32,
}

/// Build the mosaic: `target` is the big picture, `tiles` the small photos (already square, `tile` pixels).
pub fn build(target: &RgbImage, tiles: &[RgbImage], s: &Settings) -> Result<(RgbImage, usize)> {
    if tiles.is_empty() {
        bail!("No photos to build the mosaic from");
    }
    let cols = s.cols.clamp(8, 200);
    let rows = ((cols as f32 * target.height() as f32 / target.width() as f32).round() as u32).max(1);
    let (cw, ch) = (target.width() as f32 / cols as f32, target.height() as f32 / rows as f32);
    let sigs: Vec<[f32; 12]> = tiles.iter().map(|t| signature(t, 0, 0, t.width(), t.height())).collect();

    // visit the cells in a shuffled order, so a favourite tile doesn't pile up in one corner
    let mut order: Vec<u32> = (0..cols * rows).collect();
    let mut seed = 0x9E3779B97F4A7C15u64;
    for i in (1..order.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        order.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    let mut chosen = vec![usize::MAX; (cols * rows) as usize];
    let mut used = vec![0u32; tiles.len()];
    let fair = (cols * rows) as f32 / tiles.len() as f32; // how often each photo "should" appear
    for &cell in &order {
        let (c, r) = (cell % cols, cell / cols);
        let want = signature(target, (c as f32 * cw) as u32, (r as f32 * ch) as u32, cw.ceil() as u32, ch.ceil() as u32);
        // photos already used close by (2 cells around) are skipped: no clumps of the same picture
        let near: Vec<usize> = (r.saturating_sub(2)..(r + 3).min(rows))
            .flat_map(|rr| (c.saturating_sub(2)..(c + 3).min(cols)).map(move |cc| (rr * cols + cc) as usize))
            .map(|i| chosen[i]).filter(|&i| i != usize::MAX).collect();
        let mut best = (f32::MAX, 0);
        for (i, sig) in sigs.iter().enumerate() {
            if near.contains(&i) && tiles.len() > 25 {
                continue;
            }
            // using one photo much more than its fair share costs a little extra
            let penalty = 1.0 + 0.5 * (used[i] as f32 / fair.max(1.0));
            let d = distance(sig, &want) * penalty;
            if d < best.0 {
                best = (d, i);
            }
        }
        chosen[cell as usize] = best.1;
        used[best.1] += 1;
    }

    let t = s.tile;
    let mut out = RgbImage::from_pixel(cols * t, rows * t, Rgb([18, 18, 22]));
    // how far each tile's colours move toward the colour of its spot (the tile keeps its own detail)
    let tint = match s.style.as_str() {
        "blend" | "puzzle" => 0.6,
        _ => 0.25,
    };
    for r in 0..rows {
        for c in 0..cols {
            let pick = chosen[(r * cols + c) as usize];
            let (tile, have) = (&tiles[pick], &sigs[pick]);
            // the colour this cell stands for in the big picture
            let want = signature(target, (c as f32 * cw) as u32, (r as f32 * ch) as u32, cw.ceil() as u32, ch.ceil() as u32);
            let (half, rad) = (t as f32 / 2.0, t as f32 * 0.47);
            for y in 0..t {
                for x in 0..t {
                    if s.style == "bubbles" && ((x as f32 + 0.5 - half).powi(2) + (y as f32 + 0.5 - half).powi(2)).sqrt() > rad {
                        continue;
                    }
                    let q = ((y * 2 / t) * 2 + x * 2 / t) as usize;
                    let p = tile.get_pixel(x, y);
                    let px = Rgb(std::array::from_fn(|k| {
                        (p[k] as f32 + tint * (want[q * 3 + k] - have[q * 3 + k])).round().clamp(0.0, 255.0) as u8
                    }));
                    out.put_pixel(c * t + x, r * t + y, px);
                }
            }
        }
    }
    if s.style == "puzzle" {
        let lines = crate::brand::render_svg(puzzle_overlay(cols, rows, t).as_bytes(), Some(out.width()), None)?;
        for (x, y, p) in lines.enumerate_pixels() {
            let Rgba([lr, lg, lb, a]) = *p;
            if a == 0 || x >= out.width() || y >= out.height() {
                continue;
            }
            let a = a as f32 / 255.0;
            let o = out.get_pixel_mut(x, y);
            for (k, v) in [lr, lg, lb].into_iter().enumerate() {
                o[k] = (o[k] as f32 * (1.0 - a) + v as f32 * a).round() as u8;
            }
        }
    }
    let distinct = used.iter().filter(|&&n| n > 0).count();
    Ok((out, distinct))
}

/// POST /api/mosaic  {asset_id, album_id?, style?, tiles_across?, save?}
pub async fn mosaic(State(st): State<AppState>, h: HeaderMap, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let im = immich_for(&st, &h)?;
    let target_id = body["asset_id"].as_str().ok_or_else(|| err(axum::http::StatusCode::BAD_REQUEST, "asset_id is missing"))?.to_string();
    let style = match body["style"].as_str().unwrap_or("blend") {
        s @ ("classic" | "blend" | "puzzle" | "bubbles") => s.to_string(),
        _ => return Err(err(axum::http::StatusCode::BAD_REQUEST, "style must be classic, blend, puzzle or bubbles")),
    };
    let cols = body["tiles_across"].as_u64().unwrap_or(60).clamp(8, 200) as u32;
    let job = st.jobs.start("mosaic", move |job| async move {
        let (target, a) = load(&im, &target_id, false).await?;
        let filters = match body["album_id"].as_str().filter(|s| !s.is_empty()) {
            Some(al) => json!({ "type": "IMAGE", "albumIds": [al] }),
            None => json!({ "type": "IMAGE" }),
        };
        let mut ids: Vec<String> = im.all_assets(filters).await?.iter()
            .filter_map(|x| x["id"].as_str().map(str::to_string)).filter(|i| *i != target_id).collect();
        ids.truncate(2000);
        if ids.is_empty() {
            bail!("No other photos to build the mosaic from");
        }
        // tile size: sharp tiles, but the whole mosaic stays at most ~8000 pixels wide
        let tile = (8000 / cols).clamp(24, 96);
        let total = ids.len();
        let im2 = Arc::new(im.clone());
        let mut tiles = vec![];
        let mut fetched = stream::iter(ids).map(|id| {
            let im = im2.clone();
            async move {
                let data = im.thumbnail(&id, "thumbnail").await.ok()?;
                tokio::task::spawn_blocking(move || image::load_from_memory(&data).ok().map(|i| square(&i.to_rgb8(), tile))).await.ok()?
            }
        }).buffer_unordered(8);
        let mut n = 0;
        while let Some(t) = fetched.next().await {
            tiles.extend(t);
            n += 1;
            job.set((70 * n / total) as u32);
        }
        let settings = Settings { style: style.clone(), cols, tile };
        let (out, distinct) = blocking(move || build(&target, &tiles, &settings)).await?;
        job.set(90);
        let (w, h) = out.dimensions();
        let jpeg = blocking(move || formats::to_jpeg(&out, None, 90)).await?;
        let stem = std::path::Path::new(a["originalFileName"].as_str().unwrap_or("photo"))
            .file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "photo".into());
        let name = format!("{stem}-mosaic-{style}.jpg");
        let new_asset = if body["save"].as_bool().unwrap_or(true) {
            Some(im.upload(jpeg.clone().into(), &name, a["fileCreatedAt"].as_str()).await?)
        } else {
            None
        };
        job.file(jpeg.into(), "image/jpeg", &name);
        Ok(json!({ "style": style, "tiles_across": cols, "tiles_down": h / tile, "size": [w, h],
                   "photos_used": distinct, "photos_available": total, "new_asset": new_asset }))
    });
    Ok(Json(json!({ "job": job })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mosaic_follows_the_big_picture() {
        // big picture: left half red, right half blue; tiles: plain colours
        let target = RgbImage::from_fn(200, 100, |x, _| if x < 100 { Rgb([220, 30, 30]) } else { Rgb([30, 30, 220]) });
        let colours = [[220, 30, 30], [30, 30, 220], [30, 200, 30], [240, 240, 240]];
        let tiles: Vec<RgbImage> = (0..40).map(|i| RgbImage::from_pixel(16, 16, Rgb(colours[i % 4]))).collect();
        for style in ["classic", "blend", "puzzle", "bubbles"] {
            let (out, _) = build(&target, &tiles, &Settings { style: style.into(), cols: 10, tile: 16 }).unwrap();
            assert_eq!(out.dimensions(), (160, 80));
            let left = out.get_pixel(40, 8 + 16);
            let right = out.get_pixel(120, 8 + 16);
            assert!(left[0] > 150 && left[2] < 100, "{style} left {left:?}");
            assert!(right[2] > 150 && right[0] < 100, "{style} right {right:?}");
        }
    }
}
