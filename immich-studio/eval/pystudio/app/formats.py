"""Open (almost) any image file and turn it into a normal picture.

Readers, tried in this order:
  RAW camera files  -> rawpy (LibRaw)        CR2 CR3 NEF ARW DNG RAF ORF RW2 PEF SRW 3FR IIQ ...
  HEIC / HEIF / AVIF -> pillow-heif
  JPEG XL            -> pillow-jxl-plugin
  PSD / PSB          -> psd-tools (merged layers)
  SVG                -> CairoSVG
  PDF                -> pypdfium2 (first page)
  EXR / HDR          -> OpenCV (tone-mapped)
  everything Pillow knows (JPEG PNG GIF BMP TIFF WebP ICO TGA PCX PPM DDS JP2 ...)
  anything else      -> ImageMagick, then ffmpeg (XCF, Krita, video frames ...)
"""
from __future__ import annotations

import io
import os
import subprocess
import tempfile

import numpy as np
from PIL import Image, ImageOps

RAW_EXT = {".3fr", ".ari", ".arw", ".bay", ".cap", ".cr2", ".cr3", ".crw", ".dcr", ".dcs", ".dng", ".erf",
           ".fff", ".iiq", ".k25", ".kdc", ".mef", ".mos", ".mrw", ".nef", ".nrw", ".orf", ".pef", ".raf",
           ".raw", ".rw2", ".rwl", ".sr2", ".srf", ".srw", ".x3f"}
HEIF_EXT = {".heic", ".heif", ".hif", ".avif"}
VIDEO_EXT = {".3gp", ".avi", ".flv", ".insv", ".m2ts", ".m4v", ".mkv", ".mov", ".mp4", ".mpg", ".mpeg", ".mts",
             ".webm", ".wmv"}
# Formats the Immich server already shows by itself; others get converted on import.
IMMICH_NATIVE = RAW_EXT | HEIF_EXT | VIDEO_EXT | {".jpg", ".jpeg", ".jpe", ".png", ".gif", ".webp", ".tif", ".tiff",
                                                  ".bmp", ".jxl", ".jp2", ".psd", ".svg", ".insp"}

try:
    import pillow_heif
    pillow_heif.register_heif_opener()  # AVIF is read by Pillow itself (Pillow 11+)
except Exception:  # pragma: no cover - optional
    pass
try:
    import pillow_jxl  # noqa: F401  (registers .jxl with Pillow)
except Exception:  # pragma: no cover - optional
    pass


def ext_of(name: str) -> str:
    return os.path.splitext(name.lower())[1]


def is_raw(name: str) -> bool:
    return ext_of(name) in RAW_EXT


def develop_raw(data: bytes, sixteen_bit: bool = False, half_size: bool = False) -> np.ndarray:
    """Turn RAW sensor data into a full-colour picture (camera white balance, sRGB)."""
    import rawpy
    with rawpy.imread(io.BytesIO(data)) as raw:
        return raw.postprocess(use_camera_wb=True, output_bps=16 if sixteen_bit else 8,
                               half_size=half_size, no_auto_bright=False, output_color=rawpy.ColorSpace.sRGB)


def _via_tool(data: bytes, name: str) -> Image.Image:
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "in" + ext_of(name))
        dst = os.path.join(d, "out.png")
        open(src, "wb").write(data)
        for cmd in (["convert", src + "[0]", "-flatten", dst],
                    ["ffmpeg", "-loglevel", "error", "-y", "-i", src, "-frames:v", "1", dst]):
            try:
                subprocess.run(cmd, check=True, capture_output=True, timeout=300)
                if os.path.exists(dst):  # read the PNG the normal way (16-bit, transparency, colour mode)
                    return open_any(open(dst, "rb").read(), "converted.png")
            except (subprocess.SubprocessError, FileNotFoundError):
                continue
    raise ValueError(f"Cannot read {name}")


def open_any(data: bytes, name: str) -> Image.Image:
    """Return an RGB picture (orientation fixed) from any supported file."""
    ext = ext_of(name)
    if ext in RAW_EXT:
        return Image.fromarray(develop_raw(data))
    if ext in (".psd", ".psb"):
        try:
            from psd_tools import PSDImage
            return PSDImage.open(io.BytesIO(data)).composite().convert("RGB")
        except Exception:
            pass
    if ext in (".svg", ".svgz"):
        import cairosvg
        return Image.open(io.BytesIO(cairosvg.svg2png(bytestring=data, output_width=2000))).convert("RGB")
    if ext == ".pdf":
        import pypdfium2 as pdfium
        page = pdfium.PdfDocument(data)[0]
        return page.render(scale=300 / 72).to_pil().convert("RGB")
    if ext in (".exr", ".hdr"):
        os.environ["OPENCV_IO_ENABLE_OPENEXR"] = "1"
        import cv2
        try:
            hdr = cv2.imdecode(np.frombuffer(data, np.uint8), cv2.IMREAD_ANYDEPTH | cv2.IMREAD_COLOR)
        except cv2.error:
            hdr = None
        if hdr is None:  # this OpenCV build has no EXR reader
            return _via_tool(data, name)
        ldr = np.nan_to_num(cv2.createTonemapReinhard(gamma=2.2).process(hdr.astype(np.float32)))
        return Image.fromarray(cv2.cvtColor(np.clip(ldr * 255, 0, 255).astype(np.uint8), cv2.COLOR_BGR2RGB))
    try:
        img = Image.open(io.BytesIO(data))
        img.load()
        img = ImageOps.exif_transpose(img)
        if img.mode in ("RGBA", "LA", "P"):
            img = img.convert("RGBA")
            bg = Image.new("RGB", img.size, "white")
            bg.paste(img, mask=img.split()[-1])
            return bg
        if img.mode == "I;16":
            return Image.fromarray((np.asarray(img, dtype=np.uint16) >> 8).astype(np.uint8)).convert("RGB")
        return img.convert("RGB")
    except Exception:
        return _via_tool(data, name)


SAVE_FORMATS = {"jpeg": ("JPEG", ".jpg", {"quality": 95}), "png": ("PNG", ".png", {}),
                "tiff": ("TIFF", ".tif", {"compression": "tiff_lzw"}), "webp": ("WEBP", ".webp", {"quality": 95}),
                "avif": ("AVIF", ".avif", {"quality": 90}), "heic": ("HEIF", ".heic", {"quality": 90}),
                "jxl": ("JXL", ".jxl", {"quality": 95})}


def save_as(img: Image.Image, fmt: str = "jpeg") -> tuple[bytes, str]:
    pil_fmt, ext, opts = SAVE_FORMATS[fmt]
    buf = io.BytesIO()
    img.save(buf, pil_fmt, **opts)
    return buf.getvalue(), ext


def to_jpeg(img: Image.Image, max_side: int | None = None, quality: int = 90) -> bytes:
    if max_side:
        img = img.copy()
        img.thumbnail((max_side, max_side))
    buf = io.BytesIO()
    img.convert("RGB").save(buf, "JPEG", quality=quality)
    return buf.getvalue()
