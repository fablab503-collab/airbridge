//! Open (almost) any picture file and turn it into a normal RGB picture; save to common formats.
//!
//! Readers, tried in this order:
//!   RAW camera files   -> rawler (pure Rust: CR2 CR3 NEF ARW DNG RAF ORF RW2 PEF ...)
//!   HEIC / HEIF / AVIF -> libheif
//!   JPEG XL            -> jxl-oxide (pure Rust)
//!   PSD                -> psd (merged layers)
//!   SVG                -> resvg
//!   PDF                -> pdftoppm (first page, 300 dpi)
//!   EXR / HDR          -> image, tone-mapped
//!   JPEG PNG GIF BMP TIFF WebP ICO TGA PNM QOI -> image (EXIF rotation applied)
//!   anything else      -> ImageMagick, then ffmpeg (PCX, DDS, JP2, XPM, MIFF, XCF, video frames ...)

use std::{io::Cursor, path::Path, process::Command};

use anyhow::{anyhow, bail, Context, Result};
use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageDecoder, ImageEncoder, ImageReader, RgbImage, RgbaImage};

pub const RAW_EXT: &[&str] = &["3fr", "ari", "arw", "bay", "cap", "cr2", "cr3", "crw", "dcr", "dcs", "dng", "erf", "fff",
    "iiq", "k25", "kdc", "mef", "mos", "mrw", "nef", "nrw", "orf", "pef", "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "x3f"];
pub const HEIF_EXT: &[&str] = &["heic", "heif", "hif", "avif"];
pub const VIDEO_EXT: &[&str] = &["3gp", "avi", "flv", "insv", "m2ts", "m4v", "mkv", "mov", "mp4", "mpg", "mpeg", "mts", "webm", "wmv"];
const IMMICH_OTHER: &[&str] = &["jpg", "jpeg", "jpe", "png", "gif", "webp", "tif", "tiff", "bmp", "jxl", "jp2", "psd", "svg", "insp"];

pub fn ext_of(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_raw(name: &str) -> bool {
    RAW_EXT.contains(&ext_of(name).as_str())
}

/// Formats the Immich server shows by itself; others get converted on import.
pub fn immich_native(name: &str) -> bool {
    let e = ext_of(name);
    [RAW_EXT, HEIF_EXT, VIDEO_EXT, IMMICH_OTHER].iter().any(|l| l.contains(&e.as_str()))
}

/// RAW sensor data -> full-colour picture (camera white balance, sRGB).
pub fn develop_raw(data: &[u8]) -> Result<RgbImage> {
    let src = rawler::rawsource::RawSource::new_from_slice(data);
    let raw = rawler::decode(&src, &rawler::decoders::RawDecodeParams::default()).map_err(|e| anyhow!("RAW: {e}"))?;
    let dev = rawler::imgop::develop::RawDevelop::default();
    let mut img = dev.develop_intermediate(&raw).map_err(|e| anyhow!("RAW develop: {e}"))?
        .to_dynamic_image().ok_or_else(|| anyhow!("RAW develop gave no picture"))?;
    // turn it the way the camera was held (EXIF orientation)
    use image::metadata::Orientation as O;
    use rawler::decoders::Orientation as R;
    img.apply_orientation(match raw.orientation {
        R::HorizontalFlip => O::FlipHorizontal,
        R::Rotate180 => O::Rotate180,
        R::VerticalFlip => O::FlipVertical,
        R::Transpose => O::Rotate90FlipH,
        R::Rotate90 => O::Rotate90,
        R::Transverse => O::Rotate270FlipH,
        R::Rotate270 => O::Rotate270,
        R::Normal | R::Unknown => O::NoTransforms,
    });
    Ok(auto_bright(img.to_rgb8()))
}

/// Like LibRaw's auto-brightness: stretch so the darkest 0.1% is black and the
/// brightest 1% just reaches white (a straight RAW develop looks flat and grey).
fn auto_bright(mut img: RgbImage) -> RgbImage {
    let mut hist = [0u64; 256];
    for p in img.pixels() {
        hist[*p.0.iter().max().unwrap() as usize] += 1;
    }
    let total = (img.width() as u64 * img.height() as u64).max(1);
    let pick = |frac: f64| {
        let mut acc = 0;
        for (v, n) in hist.iter().enumerate() {
            acc += n;
            if acc as f64 >= total as f64 * frac { return v as f32; }
        }
        255.0
    };
    let (lo, hi) = (pick(0.001), pick(0.99));
    if hi - lo < 16.0 { return img; }
    let lut: Vec<u8> = (0..256).map(|v| ((v as f32 - lo) * 255.0 / (hi - lo)).clamp(0.0, 255.0) as u8).collect();
    for p in img.pixels_mut() {
        for c in p.0.iter_mut() { *c = lut[*c as usize]; }
    }
    img
}

fn flatten(img: DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    // transparent areas become white, like a print on paper
    let rgba = img.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y).0;
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
    })
}

fn heif(data: &[u8]) -> Result<RgbImage> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};
    let ctx = HeifContext::read_from_bytes(data)?;
    let handle = ctx.primary_image_handle()?;
    let img = LibHeif::new().decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)?;
    let plane = img.planes().interleaved.ok_or_else(|| anyhow!("HEIF: no picture data"))?;
    let (w, h) = (plane.width, plane.height);
    let mut out = RgbImage::new(w, h);
    for y in 0..h as usize {
        let row = &plane.data[y * plane.stride..y * plane.stride + w as usize * 3];
        out.as_mut()[y * w as usize * 3..(y + 1) * w as usize * 3].copy_from_slice(row);
    }
    Ok(out)
}

fn jxl(data: &[u8]) -> Result<RgbImage> {
    let img = jxl_oxide::JxlImage::builder().read(Cursor::new(data)).map_err(|e| anyhow!("JPEG XL: {e}"))?;
    let render = img.render_frame(0).map_err(|e| anyhow!("JPEG XL: {e}"))?;
    let fb = render.image_all_channels();
    let (w, h, ch) = (fb.width(), fb.height(), fb.channels());
    let buf = fb.buf();
    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Ok(RgbImage::from_fn(w as u32, h as u32, |x, y| {
        let i = (y as usize * w + x as usize) * ch;
        if ch >= 3 { image::Rgb([to8(buf[i]), to8(buf[i + 1]), to8(buf[i + 2])]) } else { let g = to8(buf[i]); image::Rgb([g, g, g]) }
    }))
}

fn psd(data: &[u8]) -> Result<RgbImage> {
    // the psd crate panics on some features (e.g. zip-compressed layers): treat that as "can't read"
    let decoded = std::panic::catch_unwind(|| -> Result<RgbImage> {
        let p = psd::Psd::from_bytes(data).map_err(|e| anyhow!("PSD: {e}"))?;
        let rgba = RgbaImage::from_raw(p.width(), p.height(), p.rgba()).ok_or_else(|| anyhow!("PSD: bad size"))?;
        Ok(flatten(DynamicImage::ImageRgba8(rgba)))
    });
    decoded.unwrap_or_else(|_| Err(anyhow!("PSD: feature not supported by the Rust reader")))
}

fn hdr_tonemap(img: DynamicImage) -> RgbImage {
    // Reinhard: bright parts are squeezed instead of clipped, then gamma 2.2 for the screen
    let f = img.to_rgb32f();
    RgbImage::from_fn(f.width(), f.height(), |x, y| {
        let p = f.get_pixel(x, y).0;
        let t = |v: f32| {
            let v = if v.is_finite() { v.max(0.0) } else { 0.0 };
            ((v / (1.0 + v)).powf(1.0 / 2.2) * 255.0).round() as u8
        };
        image::Rgb([t(p[0]), t(p[1]), t(p[2])])
    })
}

fn via_tool(data: &[u8], ext: &str) -> Result<RgbImage> {
    let dir = tempfile::tempdir()?;
    let src = dir.path().join(format!("in.{ext}"));
    std::fs::write(&src, data)?;
    let dst = dir.path().join("out.png");
    let s = src.to_string_lossy().to_string();
    let d = dst.to_string_lossy().to_string();
    let tries: Vec<Vec<String>> = if ext == "pdf" {
        vec![vec!["pdftoppm".into(), "-png".into(), "-r".into(), "300".into(), "-singlefile".into(), "-f".into(), "1".into(),
                  s.clone(), dir.path().join("out").to_string_lossy().into()]]
    } else {
        vec![vec!["convert".into(), format!("{s}[0]"), "-flatten".into(), d.clone()],
             vec!["ffmpeg".into(), "-loglevel".into(), "error".into(), "-y".into(), "-i".into(), s.clone(), "-frames:v".into(), "1".into(), d.clone()]]
    };
    for cmd in tries {
        let ok = Command::new(&cmd[0]).args(&cmd[1..]).output().map(|o| o.status.success()).unwrap_or(false);
        if ok && dst.exists() {
            return Ok(flatten(image::open(&dst)?));
        }
    }
    bail!("Cannot read .{ext} files")
}

fn with_image_crate(data: &[u8]) -> Result<RgbImage> {
    let reader = ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation); // photos taken sideways
    Ok(match img {
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => hdr_tonemap(img),
        other => flatten(other),
    })
}

/// Return an RGB picture from any supported file.
pub fn open_any(data: &[u8], name: &str) -> Result<RgbImage> {
    let ext = ext_of(name);
    let e = ext.as_str();
    let first: Option<Result<RgbImage>> = if RAW_EXT.contains(&e) {
        Some(develop_raw(data))
    } else if HEIF_EXT.contains(&e) {
        Some(heif(data))
    } else if e == "jxl" {
        Some(jxl(data))
    } else if e == "psd" || e == "psb" {
        Some(psd(data))
    } else if e == "svg" || e == "svgz" {
        Some(crate::brand::render_svg(data, Some(2000), None).map(|i| flatten(DynamicImage::ImageRgba8(i))))
    } else if e == "pdf" {
        Some(via_tool(data, e))
    } else {
        None
    };
    match first {
        Some(Ok(img)) => return Ok(img),
        Some(Err(err)) if !matches!(e, "psd" | "psb") && !RAW_EXT.contains(&e) => return Err(err),
        _ => {} // RAW/PSD the Rust reader doesn't know: let ImageMagick try
    }
    with_image_crate(data).or_else(|_| via_tool(data, if e.is_empty() { "bin" } else { e }))
        .with_context(|| format!("Cannot read {name}"))
}

pub fn to_jpeg(img: &RgbImage, max_side: Option<u32>, quality: u8) -> Result<Vec<u8>> {
    let resized;
    let img = match max_side {
        Some(m) if img.width().max(img.height()) > m => {
            resized = DynamicImage::ImageRgb8(img.clone()).resize(m, m, image::imageops::FilterType::Triangle).to_rgb8();
            &resized
        }
        _ => img,
    };
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality).encode_image(img)?;
    Ok(out)
}

/// Encode to one of the formats the editor offers. Returns the bytes and the file extension.
pub fn save_as(img: &RgbImage, fmt: &str) -> Result<(Vec<u8>, &'static str)> {
    let mut out = Vec::new();
    let ext = match fmt {
        "jpeg" => { out = to_jpeg(img, None, 95)?; ".jpg" }
        "png" => { DynamicImage::ImageRgb8(img.clone()).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)?; ".png" }
        "tiff" => { DynamicImage::ImageRgb8(img.clone()).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Tiff)?; ".tif" }
        "webp" => { out = webp::Encoder::from_rgb(img.as_raw(), img.width(), img.height()).encode(95.0).to_vec(); ".webp" }
        "avif" => {
            image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut out, 6, 90).write_image(
                img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8)?;
            ".avif"
        }
        "heic" => { out = heic(img)?; ".heic" }
        other => bail!("Unknown format: {other}"),
    };
    Ok((out, ext))
}

fn heic(img: &RgbImage) -> Result<Vec<u8>> {
    use libheif_rs::{Channel, ColorSpace, CompressionFormat, EncoderQuality, HeifContext, Image, LibHeif, RgbChroma};
    let lib = LibHeif::new();
    let (w, h) = img.dimensions();
    let mut out = Image::new(w, h, ColorSpace::Rgb(RgbChroma::Rgb))?;
    out.create_plane(Channel::Interleaved, w, h, 8)?;
    {
        let planes = out.planes_mut();
        let plane = planes.interleaved.ok_or_else(|| anyhow!("HEIC: no plane"))?;
        for y in 0..h as usize {
            plane.data[y * plane.stride..y * plane.stride + w as usize * 3]
                .copy_from_slice(&img.as_raw()[y * w as usize * 3..(y + 1) * w as usize * 3]);
        }
    }
    let mut ctx = HeifContext::new()?;
    let mut enc = lib.encoder_for_format(CompressionFormat::Hevc).context("no HEIC encoder installed")?;
    enc.set_quality(EncoderQuality::Lossy(90))?;
    ctx.encode_image(&out, &mut enc, None)?;
    Ok(ctx.write_to_bytes()?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_formats() {
        assert!(super::immich_native("a.CR3") && super::immich_native("b.mp4") && super::immich_native("c.heic"));
        assert!(!super::immich_native("d.exr") && !super::immich_native("e.pcx"));
    }
}
