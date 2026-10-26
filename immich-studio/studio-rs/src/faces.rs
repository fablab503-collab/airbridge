//! Face tools: find faces, recognise who they are, guess age, map face points.
//!
//! All models are open source and run on the CPU with ONNX Runtime:
//!   * SCRFD (det_10g)          - finds faces and 5 key points (InsightFace)
//!   * ArcFace (glintr100/R100) - turns a face into 512 numbers; same person = close numbers
//!   * age-invariant layer      - keeps the parts of those numbers that stay the same as people age
//!   * genderage                - InsightFace age/sex guess (fallback when the age head can't run)
//!   * age head (ours)          - ridge regression on SigLIP2 numbers of the aligned face
//!   * 2d106det                 - 106 outline points
//!   * MediaPipe face mesh      - 478 points (468 mesh + 10 iris), converted to ONNX
//!
//! Pictures are prepared exactly like the Python Studio did with OpenCV, so results match.

use std::{
    path::Path,
    sync::{Mutex, RwLock},
};

use anyhow::{anyhow, Context, Result};
use image::{Rgb, RgbImage};
use ndarray::Array4;
use serde::{Deserialize, Serialize};

use crate::clip::{dot, normalize};

/// Reference 5-point positions for a 112x112 aligned face (ArcFace standard).
const ARCFACE_DST: [[f32; 2]; 5] = [[38.2946, 51.6963], [73.5318, 51.5014], [56.0252, 71.7366], [41.5493, 92.3655], [70.7299, 92.2041]];

/// A 2x3 matrix that moves points: [x', y'] = [[a, b, c], [d, e, f]] * [x, y, 1].
type Affine = [[f32; 3]; 2];

fn apply(m: &Affine, x: f32, y: f32) -> (f32, f32) {
    (m[0][0] * x + m[0][1] * y + m[0][2], m[1][0] * x + m[1][1] * y + m[1][2])
}

fn invert(m: &Affine) -> Affine {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    let (a, b, d, e) = (m[1][1] / det, -m[0][1] / det, -m[1][0] / det, m[0][0] / det);
    [[a, b, -(a * m[0][2] + b * m[1][2])], [d, e, -(d * m[0][2] + e * m[1][2])]]
}

/// OpenCV warpAffine (bilinear, black outside): output pixel (x, y) comes from `to_src` * (x, y).
fn warp(img: &RgbImage, to_src: &Affine, w: u32, h: u32) -> RgbImage {
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    let px = |x: i64, y: i64, c: usize| -> f32 {
        if x < 0 || y < 0 || x >= iw || y >= ih { 0.0 } else { img.get_pixel(x as u32, y as u32)[c] as f32 }
    };
    RgbImage::from_fn(w, h, |x, y| {
        let (sx, sy) = apply(to_src, x as f32, y as f32);
        let (x0, y0) = (sx.floor() as i64, sy.floor() as i64);
        // OpenCV works in 1/32 pixel steps
        let fx = ((sx - x0 as f32) * 32.0).round() / 32.0;
        let fy = ((sy - y0 as f32) * 32.0).round() / 32.0;
        let mut out = [0u8; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let top = px(x0, y0, c) * (1.0 - fx) + px(x0 + 1, y0, c) * fx;
            let bottom = px(x0, y0 + 1, c) * (1.0 - fx) + px(x0 + 1, y0 + 1, c) * fx;
            *o = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
        }
        Rgb(out)
    })
}

/// OpenCV resize (bilinear, no smoothing), as SCRFD was trained with.
fn resize_linear(img: &RgbImage, w: u32, h: u32) -> RgbImage {
    let (sx, sy) = (img.width() as f32 / w as f32, img.height() as f32 / h as f32);
    let (iw, ih) = (img.width() as i64 - 1, img.height() as i64 - 1);
    RgbImage::from_fn(w, h, |x, y| {
        let fx = ((x as f32 + 0.5) * sx - 0.5).max(0.0);
        let fy = ((y as f32 + 0.5) * sy - 0.5).max(0.0);
        let (x0, y0) = ((fx.floor() as i64).min(iw), (fy.floor() as i64).min(ih));
        let (x1, y1) = ((x0 + 1).min(iw), (y0 + 1).min(ih));
        let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
        let p = |x: i64, y: i64| img.get_pixel(x as u32, y as u32);
        let mut out = [0u8; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let top = p(x0, y0)[c] as f32 * (1.0 - ax) + p(x1, y0)[c] as f32 * ax;
            let bottom = p(x0, y1)[c] as f32 * (1.0 - ax) + p(x1, y1)[c] as f32 * ax;
            *o = (top * (1.0 - ay) + bottom * ay).round() as u8;
        }
        Rgb(out)
    })
}

/// Picture -> network input (1, 3, H, W): (pixel - mean) * scale, colours in RGB order.
fn blob(img: &RgbImage, mean: f32, scale: f32) -> Array4<f32> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut a = Array4::<f32>::zeros((1, 3, h, w));
    for (x, y, p) in img.enumerate_pixels() {
        for c in 0..3 {
            a[[0, c, y as usize, x as usize]] = (p[c] as f32 - mean) * scale;
        }
    }
    a
}

/// Best rotation + size + shift that moves `src` points onto `dst` points (least squares).
fn similarity(src: &[[f32; 2]], dst: &[[f32; 2]]) -> Affine {
    let n = src.len() as f32;
    let mean = |p: &[[f32; 2]]| {
        let (sx, sy) = p.iter().fold((0.0, 0.0), |a, q| (a.0 + q[0], a.1 + q[1]));
        (sx / n, sy / n)
    };
    let ((smx, smy), (dmx, dmy)) = (mean(src), mean(dst));
    let (mut a, mut b, mut norm) = (0.0, 0.0, 0.0);
    for (s, d) in src.iter().zip(dst) {
        let (sx, sy, dx, dy) = (s[0] - smx, s[1] - smy, d[0] - dmx, d[1] - dmy);
        a += sx * dx + sy * dy;
        b += sx * dy - sy * dx;
        norm += sx * sx + sy * sy;
    }
    let (c, s) = (a / norm, b / norm);
    [[c, -s, dmx - (c * smx - s * smy)], [s, c, dmy - (s * smx + c * smy)]]
}

/// The face straightening the Python Studio used (OpenCV estimateAffinePartial2D with LMEDS):
/// try every pair of points, keep the fit whose middle error is smallest, then refit on the
/// points that agree with it. A badly placed point (e.g. a hidden mouth corner) is ignored.
fn similarity_lmeds(src: &[[f32; 2]; 5], dst: &[[f32; 2]; 5]) -> Affine {
    let err = |m: &Affine, i: usize| {
        let (x, y) = apply(m, src[i][0], src[i][1]);
        (x - dst[i][0]).powi(2) + (y - dst[i][1]).powi(2)
    };
    let mut best: Option<(f32, Affine)> = None;
    for i in 0..5 {
        for j in i + 1..5 {
            let m = similarity(&[src[i], src[j]], &[dst[i], dst[j]]);
            let mut e: Vec<f32> = (0..5).map(|k| err(&m, k)).collect();
            e.sort_by(f32::total_cmp);
            if best.is_none_or(|b| e[2] < b.0) {
                best = Some((e[2], m));
            }
        }
    }
    let (median, m) = best.unwrap();
    let sigma = (2.5 * 1.4826 * (1.0 + 5.0 / 3.0) * median.sqrt()).max(0.001);
    let inliers: Vec<usize> = (0..5).filter(|&k| err(&m, k) <= sigma * sigma).collect();
    if inliers.len() < 2 {
        return m;
    }
    let s: Vec<[f32; 2]> = inliers.iter().map(|&k| src[k]).collect();
    let d: Vec<[f32; 2]> = inliers.iter().map(|&k| dst[k]).collect();
    similarity(&s, &d)
}

/// One ONNX model, shared by all requests (ONNX Runtime runs one picture at a time per model).
struct Net {
    session: Mutex<ort::session::Session>,
    input: String,
    outputs: Vec<String>,
}

impl Net {
    fn load(path: &Path) -> Result<Self> {
        let session = ort::session::Session::builder().map_err(|e| anyhow!("ONNX Runtime: {e}"))?
            .commit_from_file(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let input = session.inputs[0].name.clone();
        let outputs = session.outputs.iter().map(|o| o.name.clone()).collect();
        Ok(Self { session: Mutex::new(session), input, outputs })
    }

    /// Run the model; every output comes back as a flat list of numbers.
    fn run(&self, x: Array4<f32>) -> Result<Vec<Vec<f32>>> {
        let tensor = ort::value::Tensor::from_array(x).map_err(|e| anyhow!("{e}"))?;
        let mut s = self.session.lock().map_err(|_| anyhow!("face model is broken"))?;
        let out = s.run(ort::inputs![self.input.as_str() => tensor]).map_err(|e| anyhow!("face model: {e}"))?;
        self.outputs.iter().map(|n| {
            let (_, data) = out[n.as_str()].try_extract_tensor::<f32>().map_err(|e| anyhow!("{e}"))?;
            Ok(data.to_vec())
        }).collect()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Linear {
    pub input: usize,
    pub output: usize,
    pub matrix: Vec<f32>, // input x output, row by row
    pub offset: Vec<f32>,
}

impl Linear {
    pub fn apply(&self, x: &[f32]) -> Vec<f32> {
        let mut z = self.offset.clone();
        for (i, xi) in x.iter().enumerate().take(self.input) {
            let row = &self.matrix[i * self.output..(i + 1) * self.output];
            z.iter_mut().zip(row).for_each(|(o, w)| *o += xi * w);
        }
        normalize(z)
    }
}

#[derive(Deserialize)]
pub struct AgeHead {
    pub clip_model: String,
    coef: Vec<f32>,
    intercept: f32,
}

impl AgeHead {
    pub fn predict(&self, emb: &[f32]) -> f32 {
        (dot(&self.coef, emb) + self.intercept).clamp(0.0, 100.0)
    }
}

pub struct Face {
    pub bbox: [f32; 4],
    pub score: f32,
    pub kps: [[f32; 2]; 5],
    pub embedding: Vec<f32>,
    pub raw_embedding: Vec<f32>,
    pub age: f32,
    pub age_insightface: f32,
    pub sex: &'static str,
    pub mesh: Option<Vec<[f32; 3]>>,
    pub points106: Vec<[f32; 2]>,
    /// the face turned upright, 112x112 (fed to the age head)
    pub aligned: RgbImage,
}

impl Face {
    pub fn point_count(&self) -> usize {
        self.mesh.as_ref().map_or(0, |m| m.len()) + self.points106.len()
    }
}

pub struct FacePipeline {
    detector: Net,
    recognizer: Net,
    genderage: Net,
    outline: Net,
    mesh: Option<Net>,
    pub age_invariant: RwLock<Option<Linear>>,
    pub age_head: Option<AgeHead>,
    pub invariant_path: std::path::PathBuf,
}

impl FacePipeline {
    pub fn load(dir: &Path) -> Result<Self> {
        let buffalo = dir.join("buffalo_l");
        // antelopev2 (ArcFace R100) recognised people across ages better than Immich's default R50
        let strong = dir.join("antelopev2").join("glintr100.onnx");
        let recognizer = if strong.exists() { strong } else { buffalo.join("w600k_r50.onnx") };
        let invariant_path = dir.join("age_invariant.json");
        fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Option<T> {
            std::fs::read(p).ok().and_then(|d| serde_json::from_slice(&d).ok())
        }
        let mesh_path = dir.join("face_mesh_478.onnx");
        Ok(Self {
            detector: Net::load(&buffalo.join("det_10g.onnx")).context("face finder")?,
            recognizer: Net::load(&recognizer)?,
            genderage: Net::load(&buffalo.join("genderage.onnx"))?,
            outline: Net::load(&buffalo.join("2d106det.onnx"))?,
            mesh: if mesh_path.exists() { Some(Net::load(&mesh_path)?) } else { None },
            age_invariant: RwLock::new(read_json(&invariant_path)),
            age_head: read_json(&dir.join("age_head.json")),
            invariant_path,
        })
    }

    /// SCRFD: boxes, scores and 5 key points (eyes, nose, mouth corners).
    fn detect(&self, img: &RgbImage, threshold: f32) -> Result<Vec<(f32, [f32; 4], [[f32; 2]; 5])>> {
        const SIZE: u32 = 640;
        let scale = SIZE as f32 / img.width().max(img.height()) as f32;
        let (nw, nh) = ((img.width() as f32 * scale).round() as u32, (img.height() as f32 * scale).round() as u32);
        let mut canvas = RgbImage::new(SIZE, SIZE);
        image::imageops::replace(&mut canvas, &resize_linear(img, nw.max(1), nh.max(1)), 0, 0);
        let outs = self.detector.run(blob(&canvas, 127.5, 1.0 / 128.0))?;
        let mut found = vec![];
        for (i, stride) in [8u32, 16, 32].into_iter().enumerate() {
            let (scores, boxes, kps) = (&outs[i], &outs[i + 3], &outs[i + 6]);
            let side = SIZE / stride;
            for (n, &s) in scores.iter().enumerate() {
                if s < threshold {
                    continue;
                }
                let cell = n / 2; // 2 anchors per cell
                let (cx, cy) = (((cell as u32 % side) * stride) as f32, ((cell as u32 / side) * stride) as f32);
                let st = stride as f32;
                let d = &boxes[n * 4..n * 4 + 4];
                let b = [(cx - d[0] * st) / scale, (cy - d[1] * st) / scale, (cx + d[2] * st) / scale, (cy + d[3] * st) / scale];
                let k = &kps[n * 10..n * 10 + 10];
                let mut p = [[0.0; 2]; 5];
                for j in 0..5 {
                    p[j] = [(cx + k[2 * j] * st) / scale, (cy + k[2 * j + 1] * st) / scale];
                }
                found.push((s, b, p));
            }
        }
        // keep the best box where several overlap
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        let area = |b: &[f32; 4]| (b[2] - b[0] + 1.0) * (b[3] - b[1] + 1.0);
        let mut keep: Vec<(f32, [f32; 4], [[f32; 2]; 5])> = vec![];
        for f in found {
            let overlaps = keep.iter().any(|k| {
                let (a, b) = (&k.1, &f.1);
                let w = (a[2].min(b[2]) - a[0].max(b[0]) + 1.0).max(0.0);
                let h = (a[3].min(b[3]) - a[1].max(b[1]) + 1.0).max(0.0);
                let inter = w * h;
                inter / (area(a) + area(b) - inter) > 0.4
            });
            if !overlaps {
                keep.push(f);
            }
        }
        Ok(keep)
    }

    /// A square crop around the face box, scaled to `out` pixels (for genderage and 2d106det).
    fn box_crop(img: &RgbImage, b: &[f32; 4], out: u32) -> (RgbImage, f32, f32, f32) {
        let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        let s = out as f32 / ((b[2] - b[0]).max(b[3] - b[1]) * 1.5);
        let half = out as f32 / 2.0;
        let m: Affine = [[s, 0.0, half - cx * s], [0.0, s, half - cy * s]];
        (warp(img, &invert(&m), out, out), s, half - cx * s, half - cy * s)
    }

    fn mesh_points(&self, net: &Net, img: &RgbImage, b: &[f32; 4], k: &[[f32; 2]; 5]) -> Result<Option<Vec<[f32; 3]>>> {
        // crop turned so the eyes are level (the way MediaPipe crops before finding the points)
        let r = -(-(k[1][1] - k[0][1])).atan2(k[1][0] - k[0][0]);
        let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        let size = (b[2] - b[0]).max(b[3] - b[1]) * 1.3;
        let (c, s, a) = (r.cos(), r.sin(), size / 256.0);
        let m: Affine = [[a * c, -a * s, cx - size / 2.0 * c + size / 2.0 * s], [a * s, a * c, cy - size / 2.0 * s - size / 2.0 * c]];
        let crop = warp(img, &m, 256, 256);
        let mut x = Array4::<f32>::zeros((1, 256, 256, 3));
        for (u, v, p) in crop.enumerate_pixels() {
            for ch in 0..3 {
                x[[0, v as usize, u as usize, ch]] = p[ch] as f32 / 255.0;
            }
        }
        let out = net.run(x)?;
        // second output says how sure the model is that this is a face
        if out.get(1).and_then(|v| v.first()).is_some_and(|&f| 1.0 / (1.0 + (-f).exp()) < 0.5) {
            return Ok(None);
        }
        Ok(Some(out[0].chunks(3).map(|p| {
            let (x, y) = apply(&m, p[0], p[1]);
            [x, y, p[2] * a]
        }).collect()))
    }

    /// Everything about every face in a picture (the age comes from genderage here; the age head,
    /// which needs the AI server, is applied afterwards by the caller).
    pub fn analyze(&self, img: &RgbImage, min_score: f32) -> Result<Vec<Face>> {
        let mut faces = vec![];
        for (score, bbox, kps) in self.detect(img, 0.5)? {
            if score < min_score {
                continue;
            }
            let dst = ARCFACE_DST;
            let aligned = warp(img, &invert(&similarity_lmeds(&kps, &dst)), 112, 112);
            let raw = normalize(self.recognizer.run(blob(&aligned, 127.5, 1.0 / 127.5))?.swap_remove(0));
            let embedding = match &*self.age_invariant.read().unwrap() {
                Some(l) => l.apply(&raw),
                None => raw.clone(),
            };
            let (crop, ..) = Self::box_crop(img, &bbox, 96);
            let ga = self.genderage.run(blob(&crop, 0.0, 1.0))?.swap_remove(0);
            let age_insightface = ga[2] * 100.0;
            let (crop, s, tx, ty) = Self::box_crop(img, &bbox, 192);
            let pts = self.outline.run(blob(&crop, 0.0, 1.0))?.swap_remove(0);
            let points106 = pts.chunks(2).map(|p| [((p[0] + 1.0) * 96.0 - tx) / s, ((p[1] + 1.0) * 96.0 - ty) / s]).collect();
            let mesh = match &self.mesh {
                Some(net) => self.mesh_points(net, img, &bbox, &kps)?,
                None => None,
            };
            faces.push(Face {
                bbox, score, kps, embedding, raw_embedding: raw, age: age_insightface, age_insightface,
                sex: if ga[0] > ga[1] { "female" } else { "male" }, mesh, points106, aligned,
            });
        }
        Ok(faces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_finds_the_move() {
        // points turned 30 degrees, made 2x bigger and shifted: the fit must undo exactly that
        let (c, s) = (2.0 * 30f32.to_radians().cos(), 2.0 * 30f32.to_radians().sin());
        let m: Affine = [[c, -s, 10.0], [s, c, -4.0]];
        let src = ARCFACE_DST;
        let dst = src.map(|p| { let (x, y) = apply(&m, p[0], p[1]); [x, y] });
        let fit = similarity(&src, &dst);
        assert_eq!(similarity_lmeds(&src, &dst).map(|r| r.map(|v| (v * 100.0).round())), fit.map(|r| r.map(|v| (v * 100.0).round())));
        for r in 0..2 { for k in 0..3 { assert!((fit[r][k] - m[r][k]).abs() < 1e-3); } }
        let back = invert(&fit);
        let (x, y) = apply(&back, dst[2][0], dst[2][1]);
        assert!((x - src[2][0]).abs() < 1e-3 && (y - src[2][1]).abs() < 1e-3);
    }
}
