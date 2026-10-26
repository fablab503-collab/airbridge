"""Printing: check if a photo is sharp enough for A4/A3/A2/A1/A0, upscale it with
AI when it is not, and make print-ready PDFs (single prints and photo books).

DPI = dots (pixels) per inch on paper. Rule of thumb:
  300+  excellent (close-up viewing, photo books, A4)
  200+  good
  150+  fine for posters seen from a step back (A2/A1/A0)
  <150  soft - upscale with AI first
"""
from __future__ import annotations

import io
import math
import os

import numpy as np
import onnxruntime as ort
from PIL import Image

MODEL_DIR = os.environ.get("STUDIO_MODEL_DIR", "/models")
MM_PER_INCH = 25.4
PAPER_MM = {"A6": (105, 148), "A5": (148, 210), "A4": (210, 297), "A3": (297, 420), "A2": (420, 594),
            "A1": (594, 841), "A0": (841, 1189)}
# Smallest DPI we call "good" for each size: bigger prints are looked at from further away.
GOOD_DPI = {"A6": 300, "A5": 300, "A4": 300, "A3": 240, "A2": 200, "A1": 150, "A0": 150}


def dpi_on_paper(px_w: int, px_h: int, paper: str, margin_mm: float = 0) -> float:
    """DPI when the photo fills the paper (paper turned to match the photo)."""
    pw, ph = PAPER_MM[paper]
    if (px_w > px_h) != (pw > ph):
        pw, ph = ph, pw
    pw, ph = pw - 2 * margin_mm, ph - 2 * margin_mm
    return min(px_w / (pw / MM_PER_INCH), px_h / (ph / MM_PER_INCH))


def print_report(px_w: int, px_h: int) -> list[dict]:
    rows = []
    for paper in PAPER_MM:
        dpi = dpi_on_paper(px_w, px_h, paper)
        need = GOOD_DPI[paper]
        if dpi >= need:
            verdict, upscale = "excellent" if dpi >= 300 else "good", 1
        elif dpi * 4 >= need:
            verdict, upscale = "needs AI upscale", 2 if dpi * 2 >= need else 4
        else:
            verdict, upscale = "too small", 4
        rows.append({"paper": paper, "mm": PAPER_MM[paper], "dpi": round(dpi), "good_dpi": need,
                     "verdict": verdict, "upscale": upscale})
    return rows


class Upscaler:
    """Real-ESRGAN general x4 (BSD-3). Works in tiles so big photos fit in memory."""

    def __init__(self, path: str | None = None, threads: int = 0):
        opts = ort.SessionOptions()
        if threads:
            opts.intra_op_num_threads = threads
        self.sess = ort.InferenceSession(path or os.path.join(MODEL_DIR, "realesr-general-x4v3.onnx"), opts,
                                         providers=["CPUExecutionProvider"])

    def upscale(self, img: Image.Image, scale: int = 4, tile: int = 256, pad: int = 16) -> Image.Image:
        a = np.asarray(img.convert("RGB"), dtype=np.float32) / 255
        h, w = a.shape[:2]
        out = np.zeros((h * 4, w * 4, 3), dtype=np.float32)
        for y in range(0, h, tile):
            for x in range(0, w, tile):
                y0, x0 = max(y - pad, 0), max(x - pad, 0)
                y1, x1 = min(y + tile + pad, h), min(x + tile + pad, w)
                part = a[y0:y1, x0:x1].transpose(2, 0, 1)[None]
                res = self.sess.run(None, {"input": part})[0][0].transpose(1, 2, 0)
                ty, tx = (y - y0) * 4, (x - x0) * 4
                th, tw = min(tile, h - y) * 4, min(tile, w - x) * 4
                out[y * 4:y * 4 + th, x * 4:x * 4 + tw] = res[ty:ty + th, tx:tx + tw]
        big = Image.fromarray((np.clip(out, 0, 1) * 255).round().astype(np.uint8))
        if scale != 4:
            big = big.resize((w * scale, h * scale), Image.LANCZOS)
        return big


def _canvas(c, paper: str, landscape: bool):
    from reportlab.lib.units import mm
    pw, ph = PAPER_MM[paper]
    if landscape:
        pw, ph = ph, pw
    c.setPageSize((pw * mm, ph * mm))
    return pw * mm, ph * mm


def _crop_marks(c, x, y, w, h, bleed):
    c.setLineWidth(0.25)
    for cx, cy, dx, dy in [(x, y, -1, -1), (x + w, y, 1, -1), (x, y + h, -1, 1), (x + w, y + h, 1, 1)]:
        c.line(cx + dx * bleed, cy, cx + dx * (bleed + 15), cy)
        c.line(cx, cy + dy * bleed, cx, cy + dy * (bleed + 15))


def print_pdf(img: Image.Image, paper: str = "A4", fit: str = "fill", margin_mm: float = 0,
              crop_marks: bool = False, title: str | None = None) -> bytes:
    """One photo on one page at the exact paper size. fit = "fill" (crop edges) or "fit" (white border)."""
    from reportlab.lib.units import mm
    from reportlab.lib.utils import ImageReader
    from reportlab.pdfgen import canvas

    buf = io.BytesIO()
    c = canvas.Canvas(buf)
    c.setTitle(title or f"Print {paper}")
    landscape = img.width > img.height
    pw, ph = _canvas(c, paper, landscape)
    m = margin_mm * mm + (10 * mm if crop_marks else 0)
    bw, bh = pw - 2 * m, ph - 2 * m
    photo = img
    if fit == "fill":
        target = bw / bh
        if photo.width / photo.height > target:
            nw = int(photo.height * target)
            photo = photo.crop(((photo.width - nw) // 2, 0, (photo.width + nw) // 2, photo.height))
        else:
            nh = int(photo.width / target)
            photo = photo.crop((0, (photo.height - nh) // 2, photo.width, (photo.height + nh) // 2))
        w, h = bw, bh
    else:
        s = min(bw / photo.width, bh / photo.height)
        w, h = photo.width * s, photo.height * s
    x, y = (pw - w) / 2, (ph - h) / 2
    c.drawImage(ImageReader(photo), x, y, w, h)
    if crop_marks:
        _crop_marks(c, x, y, w, h, 3 * mm)
    c.showPage()
    c.save()
    return buf.getvalue()


def book_pdf(photos: list[tuple[Image.Image, str]], paper: str = "A4", per_page: int = 2,
             title: str = "Photo book", subtitle: str = "") -> bytes:
    """A simple photo book: a cover, then 1/2/4 photos per page with captions."""
    from reportlab.lib.units import mm
    from reportlab.lib.utils import ImageReader
    from reportlab.pdfgen import canvas

    buf = io.BytesIO()
    c = canvas.Canvas(buf)
    c.setTitle(title)
    pw, ph = _canvas(c, paper, False)
    # cover: first photo full-bleed with the title on a band
    if photos:
        cover = photos[0][0]
        s = max(pw / cover.width, ph / cover.height)
        c.drawImage(ImageReader(cover), (pw - cover.width * s) / 2, (ph - cover.height * s) / 2,
                    cover.width * s, cover.height * s)
    c.setFillColorRGB(1, 1, 1, alpha=0.85)
    c.rect(0, ph * 0.12, pw, 40 * mm, stroke=0, fill=1)
    c.setFillColorRGB(0.1, 0.1, 0.1)
    c.setFont("Helvetica-Bold", 28)
    c.drawCentredString(pw / 2, ph * 0.12 + 22 * mm, title)
    c.setFont("Helvetica", 13)
    c.drawCentredString(pw / 2, ph * 0.12 + 10 * mm, subtitle)
    c.showPage()

    cols = 1 if per_page <= 2 else 2
    rows = math.ceil(per_page / cols)
    margin, gap, cap = 15 * mm, 8 * mm, 7 * mm
    cell_w = (pw - 2 * margin - (cols - 1) * gap) / cols
    cell_h = (ph - 2 * margin - (rows - 1) * gap) / rows - cap
    for start in range(0, len(photos), per_page):
        _canvas(c, paper, False)
        for k, (photo, caption) in enumerate(photos[start:start + per_page]):
            r, col = divmod(k, cols)
            s = min(cell_w / photo.width, cell_h / photo.height)
            w, h = photo.width * s, photo.height * s
            cx = margin + col * (cell_w + gap) + (cell_w - w) / 2
            top = ph - margin - r * (cell_h + cap + gap)
            c.drawImage(ImageReader(photo), cx, top - h, w, h)
            c.setFont("Helvetica", 9)
            c.setFillColorRGB(0.3, 0.3, 0.3)
            c.drawCentredString(margin + col * (cell_w + gap) + cell_w / 2, top - h - 5 * mm, caption[:110])
        c.setFont("Helvetica", 8)
        c.drawCentredString(pw / 2, 7 * mm, str(start // per_page + 2))
        c.showPage()
    c.save()
    return buf.getvalue()
