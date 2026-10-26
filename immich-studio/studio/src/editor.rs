//! Photo edits: a list of steps applied in order, so the original is never lost.
//!
//! Example: `[{"op": "rotate", "degrees": 90}, {"op": "saturation", "amount": 1.3},
//!            {"op": "recolor", "from_hue": 0, "to_hue": 220, "width": 25}]`
//!
//! Same steps and the same maths as the Python Studio (which used Pillow), so saved edits
//! look the same whichever one runs them.

use anyhow::{bail, Result};
use image::{imageops, GrayImage, Luma, Rgb, RgbImage};
use rayon::prelude::*;
use serde_json::Value;

fn num(s: &Value, k: &str, default: Option<f32>) -> Result<f32> {
    match s.get(k).and_then(Value::as_f64) {
        Some(v) => Ok(v as f32),
        None => default.ok_or_else(|| anyhow::anyhow!("edit step '{}' needs '{k}'", s["op"].as_str().unwrap_or("?"))),
    }
}

/// Pillow's grey value of a colour.
fn luma(p: &Rgb<u8>) -> u8 {
    ((p[0] as u32 * 19595 + p[1] as u32 * 38470 + p[2] as u32 * 7471 + 0x8000) >> 16) as u8
}

fn clip(v: f32) -> u8 {
    v.clamp(0.0, 255.0) as u8
}

/// Run a function on every pixel, many rows at once.
fn per_pixel(img: &mut RgbImage, f: impl Fn(&mut [u8]) + Sync + Send) {
    img.as_mut().par_chunks_mut(3).for_each(f);
}

/// Pillow's Image.blend(degenerate, img, factor): further from the "degenerate" picture as factor grows.
fn blend(img: &mut RgbImage, degenerate: &RgbImage, factor: f32) {
    img.as_mut().par_iter_mut().zip(degenerate.as_raw().par_iter()).for_each(|(o, &d)| {
        *o = clip(d as f32 + factor * (*o as f32 - d as f32));
    });
}

// Pillow's HSV: every channel is 0..255.
fn rgb_to_hsv(p: [u8; 3]) -> [f32; 3] {
    let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
    let (maxc, minc) = (r.max(g).max(b), r.min(g).min(b));
    if maxc == minc {
        return [0.0, 0.0, maxc];
    }
    let cr = maxc - minc;
    let (rc, gc, bc) = ((maxc - r) / cr, (maxc - g) / cr, (maxc - b) / cr);
    let h = if r == maxc { bc - gc } else if g == maxc { 2.0 + rc - bc } else { 4.0 + gc - rc };
    let h = (h / 6.0 + 1.0) % 1.0;
    [((h * 255.0) as i32).clamp(0, 255) as f32, ((cr / maxc * 255.0) as i32).clamp(0, 255) as f32, maxc]
}

fn hsv_to_rgb(hsv: [f32; 3]) -> [u8; 3] {
    let [h, s, v] = hsv.map(|c| c.clamp(0.0, 255.0) as u8 as f32);
    if s == 0.0 {
        return [v as u8; 3];
    }
    let i = (h * 6.0 / 255.0).floor();
    let f = h * 6.0 / 255.0 - i;
    let fs = s / 255.0;
    let p = clip((v * (1.0 - fs)).round());
    let q = clip((v * (1.0 - fs * f)).round());
    let t = clip((v * (1.0 - fs * (1.0 - f))).round());
    let v = v as u8;
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Soft 0..1 weight for colours near `center` degrees on the colour wheel.
fn hue_mask(hue: f32, center: f32, width: f32) -> f32 {
    let h = hue * 360.0 / 255.0;
    let d = ((h - center + 180.0).rem_euclid(360.0) - 180.0).abs();
    (1.0 - (d - width) / 6.0).clamp(0.0, 1.0)
}

/// Turn clockwise by any angle; the canvas grows so nothing is cut off (corners are black).
fn rotate(img: RgbImage, degrees: f32) -> RgbImage {
    let d = degrees.rem_euclid(360.0);
    match d {
        x if x == 0.0 => return img,
        x if x == 90.0 => return imageops::rotate90(&img),
        x if x == 180.0 => return imageops::rotate180(&img),
        x if x == 270.0 => return imageops::rotate270(&img),
        _ => {}
    }
    let (w, h) = (img.width() as f32, img.height() as f32);
    let (sin, cos) = d.to_radians().sin_cos();
    // canvas size the way Pillow works it out (so both Studios give the same size)
    let (ex, ey) = ((w * cos.abs() + h * sin.abs()) / 2.0, (w * sin.abs() + h * cos.abs()) / 2.0);
    let nw = (((w / 2.0 + ex).ceil() - (w / 2.0 - ex).floor()) as u32).max(1);
    let nh = (((h / 2.0 + ey).ceil() - (h / 2.0 - ey).floor()) as u32).max(1);
    let (cx, cy, ncx, ncy) = (w / 2.0, h / 2.0, nw as f32 / 2.0, nh as f32 / 2.0);
    let mut out = RgbImage::new(nw, nh);
    out.as_mut().par_chunks_mut(nw as usize * 3).enumerate().for_each(|(y, row)| {
        for x in 0..nw as usize {
            // where this output pixel comes from in the original (turn back the other way)
            let (dx, dy) = (x as f32 + 0.5 - ncx, y as f32 + 0.5 - ncy);
            let sx = cos * dx + sin * dy + cx;
            let sy = -sin * dx + cos * dy + cy;
            if let Some(p) = bicubic(&img, sx, sy) {
                row[x * 3..x * 3 + 3].copy_from_slice(&p);
            }
        }
    });
    out
}

/// Pillow's bicubic sample at (x, y) in pixel-edge coordinates; None outside the picture.
fn bicubic(img: &RgbImage, x: f32, y: f32) -> Option<[u8; 3]> {
    let (w, h) = (img.width() as i32, img.height() as i32);
    if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
        return None;
    }
    let (x, y) = (x - 0.5, y - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (dx, dy) = ((x - x0) as f64, (y - y0) as f64);
    // the cubic Pillow's transform code uses (its BICUBIC macro)
    let cubic = |v1: f64, v2: f64, v3: f64, v4: f64, d: f64| {
        let (p1, p2, p3, p4) = (v2, -v1 + v3, 2.0 * (v1 - v2) + v3 - v4, -v1 + v2 - v3 + v4);
        p1 + d * (p2 + d * (p3 + d * p4))
    };
    let px = |i: i32, j: i32, ch: usize| {
        img.get_pixel((x0 as i32 - 1 + i).clamp(0, w - 1) as u32, (y0 as i32 - 1 + j).clamp(0, h - 1) as u32)[ch] as f64
    };
    let mut out = [0u8; 3];
    for (ch, o) in out.iter_mut().enumerate() {
        let r: Vec<f64> = (0..4).map(|j| cubic(px(0, j, ch), px(1, j, ch), px(2, j, ch), px(3, j, ch), dx)).collect();
        let v = cubic(r[0], r[1], r[2], r[3], dy);
        *o = if v <= 0.0 { 0 } else if v >= 255.0 { 255 } else { v as u8 };
    }
    Some(out)
}

/// Pillow's SMOOTH filter, the "soft" picture that sharpness moves away from.
fn smooth(img: &RgbImage) -> RgbImage {
    let (w, h) = img.dimensions();
    let mut out = img.clone();
    if w < 3 || h < 3 {
        return out;
    }
    out.as_mut().par_chunks_mut(w as usize * 3).enumerate().skip(1).take(h as usize - 2).for_each(|(y, row)| {
        for x in 1..w - 1 {
            for c in 0..3 {
                let mut s = 0.0;
                for (dy, dx) in (-1i32..=1).flat_map(|dy| (-1i32..=1).map(move |dx| (dy, dx))) {
                    let k = if dx == 0 && dy == 0 { 5.0 } else { 1.0 };
                    s += k * img.get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)[c] as f32;
                }
                row[x as usize * 3 + c] = clip(s / 13.0 + 0.5);
            }
        }
    });
    out
}

/// Pillow's autocontrast(cutoff=1): per colour, ignore the darkest and brightest 1%, stretch the rest.
fn auto_levels(img: &mut RgbImage, cutoff: f64) {
    let mut luts = [[0u8; 256]; 3];
    for (c, lut) in luts.iter_mut().enumerate() {
        let mut hist = [0u64; 256];
        for p in img.pixels() {
            hist[p[c] as usize] += 1;
        }
        let n: u64 = hist.iter().sum();
        let mut cut = (n as f64 * cutoff / 100.0) as u64;
        for v in hist.iter_mut() {
            if cut > *v { cut -= *v; *v = 0 } else { *v -= cut; break }
        }
        let mut cut = (n as f64 * cutoff / 100.0) as u64;
        for v in hist.iter_mut().rev() {
            if cut > *v { cut -= *v; *v = 0 } else { *v -= cut; break }
        }
        let lo = hist.iter().position(|&v| v > 0).unwrap_or(0);
        let hi = hist.iter().rposition(|&v| v > 0).unwrap_or(255);
        for (i, o) in lut.iter_mut().enumerate() {
            *o = if hi <= lo {
                i as u8
            } else {
                let scale = 255.0 / (hi - lo) as f32;
                ((i as f32 * scale - lo as f32 * scale) as i32).clamp(0, 255) as u8
            };
        }
    }
    per_pixel(img, |p| for c in 0..3 { p[c] = luts[c][p[c] as usize] });
}

fn gray(img: &RgbImage) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| Luma([luma(img.get_pixel(x, y))]))
}

pub fn apply(mut img: RgbImage, steps: &[Value]) -> Result<RgbImage> {
    for s in steps {
        let op = s["op"].as_str().unwrap_or("");
        match op {
            "rotate" => img = rotate(img, num(s, "degrees", None)?),
            "flip" => {
                if s.get("direction").and_then(Value::as_str).unwrap_or("horizontal") == "horizontal" {
                    imageops::flip_horizontal_in_place(&mut img)
                } else {
                    imageops::flip_vertical_in_place(&mut img)
                }
            }
            "crop" => {
                // values are 0..1 fractions of width/height
                let (w, h) = (img.width() as f32, img.height() as f32);
                let l = ((num(s, "left", None)? * w) as u32).min(img.width());
                let t = ((num(s, "top", None)? * h) as u32).min(img.height());
                let r = ((num(s, "right", None)? * w) as u32).clamp(l, img.width());
                let b = ((num(s, "bottom", None)? * h) as u32).clamp(t, img.height());
                if r == l || b == t {
                    bail!("crop leaves nothing of the picture");
                }
                img = imageops::crop_imm(&img, l, t, r - l, b - t).to_image();
            }
            "resize" => {
                img = imageops::resize(&img, num(s, "width", None)?.max(1.0) as u32, num(s, "height", None)?.max(1.0) as u32,
                                       imageops::FilterType::Lanczos3)
            }
            "brightness" => {
                let black = RgbImage::new(img.width(), img.height());
                blend(&mut img, &black, num(s, "amount", None)?);
            }
            "contrast" => {
                let n = (img.width() as f64 * img.height() as f64).max(1.0);
                let mean = (img.pixels().map(|p| luma(p) as f64).sum::<f64>() / n + 0.5) as u8;
                let grey = RgbImage::from_pixel(img.width(), img.height(), Rgb([mean; 3]));
                blend(&mut img, &grey, num(s, "amount", None)?);
            }
            "saturation" => {
                let g = gray(&img);
                let grey = RgbImage::from_fn(img.width(), img.height(), |x, y| Rgb([g.get_pixel(x, y)[0]; 3]));
                blend(&mut img, &grey, num(s, "amount", None)?);
            }
            "sharpness" => {
                let soft = smooth(&img);
                blend(&mut img, &soft, num(s, "amount", None)?);
            }
            "exposure" => {
                // stops, e.g. +1 = twice as bright
                let gain = 2f32.powf(num(s, "stops", None)?);
                let lut: Vec<u8> = (0..256)
                    .map(|v| (((v as f32 / 255.0).powf(2.2) * gain).powf(1.0 / 2.2).clamp(0.0, 1.0) * 255.0) as u8)
                    .collect();
                per_pixel(&mut img, |p| p.iter_mut().for_each(|c| *c = lut[*c as usize]));
            }
            "temperature" => {
                // -100 cool/blue .. +100 warm/orange
                let t = num(s, "amount", None)? / 100.0;
                per_pixel(&mut img, |p| {
                    p[0] = clip(p[0] as f32 * (1.0 + 0.25 * t));
                    p[2] = clip(p[2] as f32 * (1.0 - 0.25 * t));
                });
            }
            "hue" => {
                // turn the whole colour wheel by N degrees
                let shift = num(s, "degrees", None)? * 255.0 / 360.0;
                per_pixel(&mut img, |p| {
                    let mut hsv = rgb_to_hsv([p[0], p[1], p[2]]);
                    hsv[0] = (hsv[0] + shift).rem_euclid(256.0);
                    p.copy_from_slice(&hsv_to_rgb(hsv));
                });
            }
            "recolor" => img = recolor(img, s)?,
            "grayscale" => {
                per_pixel(&mut img, |p| {
                    let g = luma(&Rgb([p[0], p[1], p[2]]));
                    p.fill(g);
                });
            }
            "sepia" => {
                per_pixel(&mut img, |p| {
                    let g = luma(&Rgb([p[0], p[1], p[2]])) as f32;
                    p[0] = clip(g * 1.07);
                    p[1] = clip(g * 0.74);
                    p[2] = clip(g * 0.43);
                });
            }
            "invert" => per_pixel(&mut img, |p| p.iter_mut().for_each(|c| *c = 255 - *c)),
            "auto_levels" => auto_levels(&mut img, 1.0),
            "blur" => img = imageops::blur(&img, num(s, "radius", Some(2.0))?),
            "sharpen" => {
                // unsharp mask: radius 2, threshold 3
                let amount = num(s, "amount", Some(120.0))? / 100.0;
                let soft = imageops::blur(&img, 2.0);
                img.as_mut().par_iter_mut().zip(soft.as_raw().par_iter()).for_each(|(o, &b)| {
                    let diff = *o as i32 - b as i32;
                    if diff.abs() >= 3 {
                        *o = clip(*o as f32 + diff as f32 * amount);
                    }
                });
            }
            _ => bail!("Unknown edit step: {op}"),
        }
    }
    Ok(img)
}

/// Change ONE colour (e.g. red shirt -> blue), optionally only inside a box.
fn recolor(img: RgbImage, s: &Value) -> Result<RgbImage> {
    let (from, to) = (num(s, "from_hue", None)?, num(s, "to_hue", None)?);
    let width = num(s, "width", Some(15.0))?;
    let (w, h) = img.dimensions();
    let bx = s.get("box").and_then(Value::as_array).map(|b| {
        let f = |i: usize| b.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        ((f(0) * w as f32) as u32, (f(1) * h as f32) as u32, (f(2) * w as f32) as u32, (f(3) * h as f32) as u32)
    });
    let hsv: Vec<[f32; 3]> = img.pixels().map(|p| rgb_to_hsv(p.0)).collect();
    let mask = GrayImage::from_fn(w, h, |x, y| {
        if let Some((l, t, r, b)) = bx {
            if x < l || x >= r || y < t || y >= b {
                return Luma([0]);
            }
        }
        let [hu, sa, _] = hsv[(y * w + x) as usize];
        // only clearly coloured pixels, not greys or skin-like tints
        let m = hue_mask(hu, from, width) * ((sa - 70.0) / 50.0).clamp(0.0, 1.0);
        Luma([(m * 255.0) as u8])
    });
    let mask = imageops::blur(&mask, 1.5);
    // Recolour fully, then blend by the mask: half-selected pixels fade between the two colours
    // instead of passing through every colour in between (no rainbow bands).
    let shift = (to - from) * 255.0 / 360.0;
    let mut out = img;
    out.as_mut().par_chunks_mut(3).zip(hsv.par_iter()).zip(mask.as_raw().par_iter()).for_each(|((p, hsv), &m)| {
        if m == 0 {
            return;
        }
        let m = m as f32 / 255.0;
        let moved = hsv_to_rgb([(hsv[0] + shift).rem_euclid(256.0), hsv[1], hsv[2]]);
        for c in 0..3 {
            p[c] = (p[c] as f32 * (1.0 - m) + moved[c] as f32 * m).round() as u8;
        }
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hsv_round_trip() {
        // the exact numbers Pillow gives (Python Studio)
        for (p, hsv, back) in [([255, 0, 0], [0, 255, 255], [255, 0, 0]), ([10, 200, 30], [89, 242, 200], [10, 200, 28]),
                               ([0, 0, 255], [170, 255, 255], [0, 0, 255]), ([250, 240, 10], [40, 244, 250], [250, 236, 11])] {
            let h = rgb_to_hsv(p);
            assert_eq!(h.map(|v| v as u8), hsv);
            assert_eq!(hsv_to_rgb(h), back);
        }
    }

    #[test]
    fn steps() {
        let img = RgbImage::from_fn(40, 20, |x, y| Rgb([(x * 6) as u8, (y * 12) as u8, 200]));
        let out = apply(img.clone(), &[json!({"op": "rotate", "degrees": 90})]).unwrap();
        assert_eq!(out.dimensions(), (20, 40));
        let out = apply(img.clone(), &[json!({"op": "crop", "left": 0.25, "top": 0, "right": 0.75, "bottom": 1})]).unwrap();
        assert_eq!(out.dimensions(), (20, 20));
        let out = apply(img.clone(), &[json!({"op": "rotate", "degrees": 30})]).unwrap();
        assert!(out.width() > 40 && out.height() > 20);
        let out = apply(img.clone(), &[json!({"op": "grayscale"})]).unwrap();
        assert!(out.pixels().all(|p| p[0] == p[1] && p[1] == p[2]));
        // a red patch turned blue; the grey stays grey
        let mut two = RgbImage::from_pixel(20, 10, Rgb([220, 20, 20]));
        for x in 10..20 { for y in 0..10 { two.put_pixel(x, y, Rgb([128, 128, 128])); } }
        let out = apply(two, &[json!({"op": "recolor", "from_hue": 0, "to_hue": 220, "width": 25})]).unwrap();
        let red = out.get_pixel(2, 5);
        assert!(red[2] > 150 && red[0] < 100, "{red:?}");
        assert_eq!(out.get_pixel(18, 5).0, [128, 128, 128]);
        assert!(apply(img, &[json!({"op": "nope"})]).is_err());
    }
}
