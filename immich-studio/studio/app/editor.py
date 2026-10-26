"""Photo edits: a list of steps applied in order, so the original is never lost.

Example:  [{"op": "rotate", "degrees": 90}, {"op": "saturation", "amount": 1.3},
           {"op": "recolor", "from_hue": 0, "to_hue": 220, "width": 25}]
"""
from __future__ import annotations

import numpy as np
from PIL import Image, ImageEnhance, ImageFilter, ImageOps


def _hsv(img: Image.Image) -> np.ndarray:
    return np.asarray(img.convert("HSV"), dtype=np.float32)


def _from_hsv(hsv: np.ndarray) -> Image.Image:
    return Image.fromarray(np.clip(hsv, 0, 255).astype(np.uint8), "HSV").convert("RGB")


def _hue_mask(hue: np.ndarray, center_deg: float, width_deg: float) -> np.ndarray:
    """Soft 0..1 mask of pixels whose colour is near center_deg on the colour wheel."""
    h = hue * 360 / 255
    d = np.abs((h - center_deg + 180) % 360 - 180)
    return np.clip(1 - (d - width_deg) / 6, 0, 1)


def apply(img: Image.Image, steps: list[dict]) -> Image.Image:
    img = img.convert("RGB")
    for s in steps:
        op = s["op"]
        if op == "rotate":
            img = img.rotate(-float(s["degrees"]), expand=True, resample=Image.BICUBIC)
        elif op == "flip":
            img = ImageOps.mirror(img) if s.get("direction", "horizontal") == "horizontal" else ImageOps.flip(img)
        elif op == "crop":  # values are 0..1 fractions of width/height
            w, h = img.size
            img = img.crop((int(s["left"] * w), int(s["top"] * h), int(s["right"] * w), int(s["bottom"] * h)))
        elif op == "resize":
            img = img.resize((int(s["width"]), int(s["height"])), Image.LANCZOS)
        elif op == "brightness":
            img = ImageEnhance.Brightness(img).enhance(float(s["amount"]))
        elif op == "contrast":
            img = ImageEnhance.Contrast(img).enhance(float(s["amount"]))
        elif op == "saturation":
            img = ImageEnhance.Color(img).enhance(float(s["amount"]))
        elif op == "sharpness":
            img = ImageEnhance.Sharpness(img).enhance(float(s["amount"]))
        elif op == "exposure":  # stops, e.g. +1 = twice as bright
            a = np.asarray(img, dtype=np.float32) / 255
            a = (a ** 2.2 * 2 ** float(s["stops"])) ** (1 / 2.2)
            img = Image.fromarray((np.clip(a, 0, 1) * 255).astype(np.uint8))
        elif op == "temperature":  # -100 cool/blue .. +100 warm/orange
            t = float(s["amount"]) / 100
            a = np.asarray(img, dtype=np.float32)
            a[..., 0] *= 1 + 0.25 * t
            a[..., 2] *= 1 - 0.25 * t
            img = Image.fromarray(np.clip(a, 0, 255).astype(np.uint8))
        elif op == "hue":  # turn the whole colour wheel by N degrees
            hsv = _hsv(img)
            hsv[..., 0] = (hsv[..., 0] + float(s["degrees"]) * 255 / 360) % 256
            img = _from_hsv(hsv)
        elif op == "recolor":  # change ONE colour (e.g. red shirt -> blue), optionally only inside a box
            hsv = _hsv(img)
            mask = _hue_mask(hsv[..., 0], float(s["from_hue"]), float(s.get("width", 15)))
            mask *= np.clip((hsv[..., 1] - 70) / 50, 0, 1)  # only clearly coloured pixels, not greys or skin-like tints
            if "box" in s:
                w, h = img.size
                l, t, r, b = s["box"]
                box = np.zeros_like(mask)
                box[int(t * h):int(b * h), int(l * w):int(r * w)] = 1
                mask *= box
            mask = np.asarray(Image.fromarray((mask * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(1.5)),
                              dtype=np.float32)[..., None] / 255
            # Recolour fully, then blend by the mask: half-selected pixels fade between the two colours
            # instead of passing through every colour in between (no rainbow bands).
            shift = (float(s["to_hue"]) - float(s["from_hue"])) * 255 / 360
            moved = hsv.copy()
            moved[..., 0] = (moved[..., 0] + shift) % 256
            a = np.asarray(img, dtype=np.float32)
            b2 = np.asarray(_from_hsv(moved), dtype=np.float32)
            img = Image.fromarray((a * (1 - mask) + b2 * mask).round().astype(np.uint8))
        elif op == "grayscale":
            img = ImageOps.grayscale(img).convert("RGB")
        elif op == "sepia":
            g = np.asarray(ImageOps.grayscale(img), dtype=np.float32)
            img = Image.fromarray(np.stack([g * 1.07, g * 0.74, g * 0.43], -1).clip(0, 255).astype(np.uint8))
        elif op == "invert":
            img = ImageOps.invert(img)
        elif op == "auto_levels":
            img = ImageOps.autocontrast(img, cutoff=1)
        elif op == "blur":
            img = img.filter(ImageFilter.GaussianBlur(float(s.get("radius", 2))))
        elif op == "sharpen":
            img = img.filter(ImageFilter.UnsharpMask(radius=2, percent=int(s.get("amount", 120)), threshold=3))
        else:
            raise ValueError(f"Unknown edit step: {op}")
    return img
