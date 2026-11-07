//! Your own logo and name instead of Immich's (files kept in DATA/brand).

use std::{fs, path::{Path, PathBuf}};

use anyhow::{anyhow, Context, Result};
use image::{imageops::FilterType, DynamicImage, RgbaImage};
use serde_json::{json, Value};

use crate::Config;

pub const FILES: &[(&str, &str)] = &[
    ("logo.svg", "image/svg+xml"), ("app-icon.svg", "image/svg+xml"), ("logo.png", "image/png"),
    ("icon-180.png", "image/png"), ("icon-512.png", "image/png"), ("favicon-32.png", "image/png"),
];

pub fn dir(cfg: &Config) -> PathBuf {
    cfg.data_dir.join("brand")
}

fn mtime(p: &Path) -> u64 {
    fs::metadata(p).and_then(|m| m.modified()).ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

/// What the pages need: the logo address (SVG preferred: sharp at every size) and the name.
pub fn info(cfg: &Config) -> Value {
    let d = dir(cfg);
    let path = [d.join("logo.svg"), d.join("logo.png")].into_iter().find(|p| p.exists());
    let Some(path) = path else { return json!({ "logo": null }) };
    let meta: Value = fs::read_to_string(d.join("brand.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    let name = path.file_name().unwrap().to_string_lossy();
    json!({ "logo": format!("api/brand/{name}?v={}", mtime(&path)), "name": meta["name"] })
}

pub fn render_svg(svg: &[u8], width: Option<u32>, height: Option<u32>) -> Result<RgbaImage> {
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(svg, &opt).context("not a valid SVG")?;
    let size = tree.size();
    let scale = match (width, height) {
        (Some(w), _) => w as f32 / size.width(),
        (_, Some(h)) => h as f32 / size.height(),
        _ => 1.0,
    };
    let (w, h) = ((size.width() * scale).round() as u32, (size.height() * scale).round() as u32);
    let mut pix = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| anyhow!("SVG too big"))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    // tiny-skia stores premultiplied alpha; image wants straight alpha
    let mut img = RgbaImage::new(w.max(1), h.max(1));
    for (p, c) in img.pixels_mut().zip(pix.pixels()) {
        let c = c.demultiply();
        *p = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(img)
}

/// Save the raster logo plus the square icons (browser tab, home screen).
fn save_logo(d: &Path, logo: &DynamicImage, app_icon: Option<RgbaImage>) -> Result<()> {
    let logo = logo.resize(1200.min(logo.width()), 400.min(logo.height()), FilterType::Lanczos3);
    logo.save(d.join("logo.png"))?;
    let icon = match app_icon {
        Some(i) => i,
        None => {
            let side = logo.width().max(logo.height());
            let mut sq = RgbaImage::from_pixel(side, side, image::Rgba([255, 255, 255, 255]));
            image::imageops::overlay(&mut sq, &logo.to_rgba8(), ((side - logo.width()) / 2) as i64, ((side - logo.height()) / 2) as i64);
            sq
        }
    };
    for (size, name) in [(180, "icon-180.png"), (512, "icon-512.png"), (32, "favicon-32.png")] {
        image::imageops::resize(&icon, size, size, FilterType::Lanczos3).save(d.join(name))?;
    }
    Ok(())
}

pub fn save_svg_logo(d: &Path, svg: &[u8], app_icon_svg: Option<&[u8]>) -> Result<()> {
    fs::write(d.join("logo.svg"), svg)?;
    let png = render_svg(svg, None, Some(400))?;
    let icon = match app_icon_svg {
        Some(a) => {
            fs::write(d.join("app-icon.svg"), a)?;
            Some(render_svg(a, Some(512), None)?)
        }
        None => None,
    };
    save_logo(d, &DynamicImage::ImageRgba8(png), icon)
}

pub fn save_raster_logo(d: &Path, data: &[u8]) -> Result<()> {
    let img = image::load_from_memory(data).context("can't read this picture (use PNG, JPEG, WebP or SVG)")?;
    save_logo(d, &img, None)
}

/// First start: use the shipped logo and name (Gimmich) until the admin changes them.
pub fn seed_default(cfg: &Config) -> Result<()> {
    let d = dir(cfg);
    let src = &cfg.brand_default_dir;
    if d.join("brand.json").exists() || !src.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(&d)?;
    if src.join("logo.svg").exists() {
        let app_icon = fs::read(src.join("app-icon.svg")).ok();
        save_svg_logo(&d, &fs::read(src.join("logo.svg"))?, app_icon.as_deref())?;
    } else if src.join("logo.png").exists() {
        save_raster_logo(&d, &fs::read(src.join("logo.png"))?)?;
    }
    fs::copy(src.join("brand.json"), d.join("brand.json"))?;
    Ok(())
}

pub fn reset(d: &Path) -> Result<()> {
    if d.exists() {
        for e in fs::read_dir(d)? {
            fs::remove_file(e?.path())?;
        }
    }
    fs::create_dir_all(d)?;
    fs::write(d.join("brand.json"), json!({ "name": null }).to_string())?; // stay on Immich's logo
    Ok(())
}
