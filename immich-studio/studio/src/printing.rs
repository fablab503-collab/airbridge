//! Printing: check if a photo is sharp enough for A6..A0, upscale it with AI when it is not,
//! and make print-ready PDFs (single prints and photo books).
//!
//! DPI = dots (pixels) per inch on paper. Rule of thumb:
//!   300+  excellent (close-up viewing, photo books, A4)
//!   200+  good
//!   150+  fine for posters seen from a step back (A2/A1/A0)
//!   <150  soft - upscale with AI first

use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, bail, Result};
use image::{imageops, RgbImage};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use serde_json::{json, Value};

use crate::formats;

const MM_PER_INCH: f32 = 25.4;
const PT_PER_MM: f32 = 72.0 / MM_PER_INCH;
pub const PAPERS: &[(&str, f32, f32)] = &[
    ("A6", 105.0, 148.0), ("A5", 148.0, 210.0), ("A4", 210.0, 297.0), ("A3", 297.0, 420.0),
    ("A2", 420.0, 594.0), ("A1", 594.0, 841.0), ("A0", 841.0, 1189.0),
];

/// Smallest DPI we call "good" for each size: bigger prints are looked at from further away.
fn good_dpi(paper: &str) -> f32 {
    match paper {
        "A3" => 240.0,
        "A2" => 200.0,
        "A1" | "A0" => 150.0,
        _ => 300.0,
    }
}

pub fn paper_mm(paper: &str) -> Result<(f32, f32)> {
    PAPERS.iter().find(|p| p.0 == paper).map(|p| (p.1, p.2)).ok_or_else(|| anyhow!("Unknown paper size {paper}"))
}

/// DPI when the photo fills the paper (paper turned to match the photo).
pub fn dpi_on_paper(px_w: u32, px_h: u32, paper: &str) -> Result<f32> {
    let (mut pw, mut ph) = paper_mm(paper)?;
    if (px_w > px_h) != (pw > ph) {
        std::mem::swap(&mut pw, &mut ph);
    }
    Ok((px_w as f32 / (pw / MM_PER_INCH)).min(px_h as f32 / (ph / MM_PER_INCH)))
}

pub fn print_report(px_w: u32, px_h: u32) -> Vec<Value> {
    PAPERS.iter().map(|&(paper, w, h)| {
        let dpi = dpi_on_paper(px_w, px_h, paper).unwrap_or(0.0);
        let need = good_dpi(paper);
        let (verdict, upscale) = if dpi >= need {
            (if dpi >= 300.0 { "excellent" } else { "good" }, 1)
        } else if dpi * 4.0 >= need {
            ("needs AI upscale", if dpi * 2.0 >= need { 2 } else { 4 })
        } else {
            ("too small", 4)
        };
        json!({ "paper": paper, "mm": [w, h], "dpi": dpi.round() as i64, "good_dpi": need as i64,
                "verdict": verdict, "upscale": upscale })
    }).collect()
}

// ------------------------------------------------------------- upscaler ---
/// Real-ESRGAN general x4 (BSD-3), run with ONNX Runtime. Loaded once, shared by all requests.
fn upscaler() -> Result<&'static Mutex<ort::session::Session>> {
    static SESSION: OnceLock<Mutex<ort::session::Session>> = OnceLock::new();
    if let Some(s) = SESSION.get() {
        return Ok(s);
    }
    let dir = std::env::var("STUDIO_MODEL_DIR").unwrap_or_else(|_| "/models".into());
    let path = std::path::Path::new(&dir).join("realesr-general-x4v3.onnx");
    if !path.exists() {
        bail!("The AI upscaler model is missing ({})", path.display());
    }
    let session = ort::session::Session::builder()
        .map_err(|e| anyhow!("ONNX Runtime: {e}"))?
        .commit_from_file(&path)
        .map_err(|e| anyhow!("upscaler model: {e}"))?;
    Ok(SESSION.get_or_init(|| Mutex::new(session)))
}

/// Make the photo `scale` times bigger with AI (2 or 4). Works in tiles so big photos fit in memory.
pub fn upscale(img: &RgbImage, scale: u32, progress: impl Fn(f32)) -> Result<RgbImage> {
    const TILE: u32 = 256;
    const PAD: u32 = 16;
    let (w, h) = img.dimensions();
    let mut out = RgbImage::new(w * 4, h * 4);
    let mut session = upscaler()?.lock().map_err(|_| anyhow!("upscaler is broken"))?;
    let tiles = w.div_ceil(TILE) * h.div_ceil(TILE);
    let mut done = 0;
    for y in (0..h).step_by(TILE as usize) {
        for x in (0..w).step_by(TILE as usize) {
            let (y0, x0) = (y.saturating_sub(PAD), x.saturating_sub(PAD));
            let (y1, x1) = ((y + TILE + PAD).min(h), (x + TILE + PAD).min(w));
            let (tw, th) = ((x1 - x0) as usize, (y1 - y0) as usize);
            let mut input = ndarray::Array4::<f32>::zeros((1, 3, th, tw));
            for yy in 0..th {
                for xx in 0..tw {
                    let p = img.get_pixel(x0 + xx as u32, y0 + yy as u32);
                    for c in 0..3 {
                        input[[0, c, yy, xx]] = p[c] as f32 / 255.0;
                    }
                }
            }
            let tensor = ort::value::Tensor::from_array(input).map_err(|e| anyhow!("{e}"))?;
            let outputs = session.run(ort::inputs!["input" => tensor]).map_err(|e| anyhow!("upscaler: {e}"))?;
            let res = outputs["output"].try_extract_array::<f32>().map_err(|e| anyhow!("{e}"))?;
            // keep only the middle of the tile (the padding is there to hide seams)
            let (ty, tx) = ((y - y0) * 4, (x - x0) * 4);
            let (keep_h, keep_w) = (TILE.min(h - y) * 4, TILE.min(w - x) * 4);
            for yy in 0..keep_h {
                for xx in 0..keep_w {
                    let mut px = [0u8; 3];
                    for (c, v) in px.iter_mut().enumerate() {
                        *v = (res[[0, c, (ty + yy) as usize, (tx + xx) as usize]].clamp(0.0, 1.0) * 255.0).round() as u8;
                    }
                    out.put_pixel(x * 4 + xx, y * 4 + yy, image::Rgb(px));
                }
            }
            done += 1;
            progress(done as f32 / tiles as f32);
        }
    }
    if scale != 4 {
        out = imageops::resize(&out, w * scale, h * scale, imageops::FilterType::Lanczos3);
    }
    Ok(out)
}

// ------------------------------------------------------------------ PDFs ---
// Widths of Helvetica / Helvetica-Bold letters (1/1000 of the font size) for centring text.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833,
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556,
    556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334,
    260, 334, 584,
];
const HELV_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833,
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611,
    556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389,
    280, 389, 584,
];

/// Text in the PDF's built-in fonts (WinAnsi: Latin letters and accents; anything else becomes "?").
fn win_ansi(text: &str) -> Vec<u8> {
    text.chars().map(|c| match c as u32 {
        0x20..=0x7e | 0xa0..=0xff => c as u8,
        _ => b'?',
    }).collect()
}

fn text_width(bytes: &[u8], size: f32, bold: bool) -> f32 {
    let table = if bold { &HELV_BOLD } else { &HELV };
    bytes.iter().map(|&b| if (32..127).contains(&b) { table[(b - 32) as usize] } else { 556 } as f32).sum::<f32>() * size / 1000.0
}

struct PageImage {
    jpeg: Vec<u8>,
    w: u32,
    h: u32,
}

struct Page {
    size: (f32, f32),
    content: Content,
    images: Vec<PageImage>,
}

/// A small PDF builder: pages with photos (as JPEG), text in Helvetica and see-through boxes.
struct Doc {
    title: String,
    pages: Vec<Page>,
}

impl Doc {
    fn new(title: &str) -> Self {
        Self { title: title.into(), pages: vec![] }
    }

    fn page(&mut self, w: f32, h: f32) -> &mut Page {
        self.pages.push(Page { size: (w, h), content: Content::new(), images: vec![] });
        self.pages.last_mut().unwrap()
    }

    fn finish(self) -> Vec<u8> {
        let mut pdf = Pdf::new();
        let mut next = 0;
        let mut id = || { next += 1; Ref::new(next) };
        let (catalog, tree, helv, bold, see_through, info) = (id(), id(), id(), id(), id(), id());
        let page_ids: Vec<Ref> = self.pages.iter().map(|_| id()).collect();
        pdf.catalog(catalog).pages(tree);
        pdf.pages(tree).kids(page_ids.iter().copied()).count(self.pages.len() as i32);
        pdf.document_info(info).title(TextStr(&self.title)).producer(TextStr("Gimmich Studio"));
        pdf.type1_font(helv).base_font(Name(b"Helvetica")).encoding_predefined(Name(b"WinAnsiEncoding"));
        pdf.type1_font(bold).base_font(Name(b"Helvetica-Bold")).encoding_predefined(Name(b"WinAnsiEncoding"));
        pdf.ext_graphics(see_through).non_stroking_alpha(0.85);
        for (page, page_id) in self.pages.into_iter().zip(page_ids) {
            let content_id = id();
            let image_ids: Vec<Ref> = page.images.iter().map(|_| id()).collect();
            let mut p = pdf.page(page_id);
            p.media_box(Rect::new(0.0, 0.0, page.size.0, page.size.1));
            p.parent(tree);
            p.contents(content_id);
            let mut res = p.resources();
            res.fonts().pair(Name(b"F1"), helv).pair(Name(b"F2"), bold);
            res.ext_g_states().pair(Name(b"GS1"), see_through);
            let mut xo = res.x_objects();
            for (i, r) in image_ids.iter().enumerate() {
                xo.pair(Name(format!("Im{i}").as_bytes()), *r);
            }
            xo.finish();
            res.finish();
            p.finish();
            for (img, r) in page.images.iter().zip(&image_ids) {
                let mut x = pdf.image_xobject(*r, &img.jpeg);
                x.filter(pdf_writer::Filter::DctDecode);
                x.width(img.w as i32);
                x.height(img.h as i32);
                x.color_space().device_rgb();
                x.bits_per_component(8);
            }
            pdf.stream(content_id, &page.content.finish());
        }
        pdf.finish()
    }
}

impl Page {
    fn image(&mut self, img: &RgbImage, x: f32, y: f32, w: f32, h: f32) -> Result<()> {
        let name = format!("Im{}", self.images.len());
        self.images.push(PageImage { jpeg: formats::to_jpeg(img, None, 92)?, w: img.width(), h: img.height() });
        self.content.save_state().transform([w, 0.0, 0.0, h, x, y]).x_object(Name(name.as_bytes())).restore_state();
        Ok(())
    }

    fn centred_text(&mut self, text: &str, cx: f32, y: f32, size: f32, bold: bool) {
        let bytes = win_ansi(text);
        let w = text_width(&bytes, size, bold);
        let font = if bold { Name(b"F2") } else { Name(b"F1") };
        self.content.begin_text().set_font(font, size).next_line(cx - w / 2.0, y).show(Str(&bytes)).end_text();
    }
}

fn page_size(paper: &str, landscape: bool) -> Result<(f32, f32)> {
    let (w, h) = paper_mm(paper)?;
    Ok(if landscape { (h * PT_PER_MM, w * PT_PER_MM) } else { (w * PT_PER_MM, h * PT_PER_MM) })
}

/// Shrink a photo to what the page needs at `dpi` (no point storing more pixels than paper shows).
fn fit_pixels(img: &RgbImage, w_pt: f32, h_pt: f32, dpi: f32) -> RgbImage {
    let (mw, mh) = ((w_pt / 72.0 * dpi) as u32, (h_pt / 72.0 * dpi) as u32);
    if img.width() <= mw || img.height() <= mh {
        return img.clone();
    }
    let s = (mw as f32 / img.width() as f32).max(mh as f32 / img.height() as f32);
    imageops::resize(img, (img.width() as f32 * s).round() as u32, (img.height() as f32 * s).round() as u32,
                     imageops::FilterType::Lanczos3)
}

/// One photo on one page at the exact paper size. fit = "fill" (crop edges) or "fit" (white border).
pub fn print_pdf(img: &RgbImage, paper: &str, fit: &str, margin_mm: f32, crop_marks: bool, title: &str) -> Result<Vec<u8>> {
    let mut doc = Doc::new(title);
    let (pw, ph) = page_size(paper, img.width() > img.height())?;
    let m = margin_mm * PT_PER_MM + if crop_marks { 10.0 * PT_PER_MM } else { 0.0 };
    let (bw, bh) = (pw - 2.0 * m, ph - 2.0 * m);
    if bw <= 0.0 || bh <= 0.0 {
        bail!("The margin is bigger than the paper");
    }
    let (photo, w, h) = if fit == "fill" {
        let target = bw / bh;
        let (iw, ih) = img.dimensions();
        let cropped = if iw as f32 / ih as f32 > target {
            let nw = ((ih as f32 * target) as u32).max(1);
            imageops::crop_imm(img, (iw - nw) / 2, 0, nw, ih).to_image()
        } else {
            let nh = ((iw as f32 / target) as u32).max(1);
            imageops::crop_imm(img, 0, (ih - nh) / 2, iw, nh).to_image()
        };
        (cropped, bw, bh)
    } else {
        let s = (bw / img.width() as f32).min(bh / img.height() as f32);
        (img.clone(), img.width() as f32 * s, img.height() as f32 * s)
    };
    let (x, y) = ((pw - w) / 2.0, (ph - h) / 2.0);
    let page = doc.page(pw, ph);
    page.image(&photo, x, y, w, h)?;
    if crop_marks {
        let bleed = 3.0 * PT_PER_MM;
        page.content.set_line_width(0.25);
        for (cx, cy, dx, dy) in [(x, y, -1.0, -1.0), (x + w, y, 1.0, -1.0), (x, y + h, -1.0, 1.0), (x + w, y + h, 1.0, 1.0)] {
            page.content.move_to(cx + dx * bleed, cy).line_to(cx + dx * (bleed + 15.0), cy).stroke();
            page.content.move_to(cx, cy + dy * bleed).line_to(cx, cy + dy * (bleed + 15.0)).stroke();
        }
    }
    Ok(doc.finish())
}

/// A simple photo book: a cover, then 1/2/4 photos per page with captions.
pub fn book_pdf(photos: &[(RgbImage, String)], paper: &str, per_page: usize, title: &str, subtitle: &str) -> Result<Vec<u8>> {
    let per_page = per_page.clamp(1, 4);
    let mut doc = Doc::new(title);
    let (pw, ph) = page_size(paper, false)?;
    // cover: first photo full-bleed with the title on a band
    let cover = doc.page(pw, ph);
    if let Some((img, _)) = photos.first() {
        let s = (pw / img.width() as f32).max(ph / img.height() as f32);
        let (w, h) = (img.width() as f32 * s, img.height() as f32 * s);
        cover.image(&fit_pixels(img, w, h, 300.0), (pw - w) / 2.0, (ph - h) / 2.0, w, h)?;
    }
    cover.content.save_state().set_parameters(Name(b"GS1")).set_fill_rgb(1.0, 1.0, 1.0)
        .rect(0.0, ph * 0.12, pw, 40.0 * PT_PER_MM).fill_nonzero().restore_state();
    cover.content.set_fill_rgb(0.1, 0.1, 0.1);
    cover.centred_text(title, pw / 2.0, ph * 0.12 + 22.0 * PT_PER_MM, 28.0, true);
    cover.centred_text(subtitle, pw / 2.0, ph * 0.12 + 10.0 * PT_PER_MM, 13.0, false);

    let cols = if per_page <= 2 { 1 } else { 2 };
    let rows = per_page.div_ceil(cols);
    let (margin, gap, cap) = (15.0 * PT_PER_MM, 8.0 * PT_PER_MM, 7.0 * PT_PER_MM);
    let cell_w = (pw - 2.0 * margin - (cols - 1) as f32 * gap) / cols as f32;
    let cell_h = (ph - 2.0 * margin - (rows - 1) as f32 * gap) / rows as f32 - cap;
    for (n, chunk) in photos.chunks(per_page).enumerate() {
        let page = doc.page(pw, ph);
        for (k, (photo, caption)) in chunk.iter().enumerate() {
            let (r, col) = (k / cols, k % cols);
            let s = (cell_w / photo.width() as f32).min(cell_h / photo.height() as f32);
            let (w, h) = (photo.width() as f32 * s, photo.height() as f32 * s);
            let cx = margin + col as f32 * (cell_w + gap) + (cell_w - w) / 2.0;
            let top = ph - margin - r as f32 * (cell_h + cap + gap);
            page.image(&fit_pixels(photo, w, h, 300.0), cx, top - h, w, h)?;
            page.content.set_fill_rgb(0.3, 0.3, 0.3);
            let caption: String = caption.chars().take(110).collect();
            page.centred_text(&caption, margin + col as f32 * (cell_w + gap) + cell_w / 2.0, top - h - 5.0 * PT_PER_MM, 9.0, false);
        }
        page.centred_text(&(n + 2).to_string(), pw / 2.0, 7.0 * PT_PER_MM, 8.0, false);
    }
    Ok(doc.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_matches_python() {
        // 6000x4000 (24 MP), same numbers as the Python Studio: A4 484 dpi, A1 171 good, A0 121 needs 2x
        let r = print_report(6000, 4000);
        let get = |p: &str| r.iter().find(|x| x["paper"] == p).unwrap().clone();
        assert_eq!(get("A4")["verdict"], "excellent");
        assert_eq!(get("A4")["dpi"], 484);
        assert_eq!(get("A0")["dpi"], 121);
        assert_eq!(get("A1")["verdict"], "good");
        assert_eq!(get("A0")["verdict"], "needs AI upscale");
        assert_eq!(get("A0")["upscale"], 2);
        assert_eq!(print_report(400, 300).last().unwrap()["verdict"], "too small");
    }

    #[test]
    fn pdfs_open() {
        let img = RgbImage::from_fn(300, 200, |x, y| image::Rgb([x as u8, y as u8, 120]));
        for (fit, marks) in [("fill", true), ("fit", false)] {
            let pdf = print_pdf(&img, "A4", fit, 5.0, marks, "Test").unwrap();
            assert!(pdf.starts_with(b"%PDF"));
        }
        let photos: Vec<_> = (0..5).map(|i| (img.clone(), format!("Photo {i} · Zürich"))).collect();
        let pdf = book_pdf(&photos, "A4", 2, "Holiday", "2025").unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        assert_eq!(pdf.windows(10).filter(|w| w == b"/Type /Pag").count() - 1, 4); // cover + 3 pages (+ the tree)
    }
}
