"""Talk to Immich's machine-learning server to turn images and text into numbers.

Images and sentences that mean the same thing get similar numbers, which lets us
label a photo by comparing it with sentences like "a photo of a beach".
"""
from __future__ import annotations

import functools
import json
import os

import numpy as np
import requests

ML_URL = os.environ.get("IMMICH_ML_URL", "http://immich-machine-learning:3003")
CLIP_MODEL = os.environ.get("STUDIO_CLIP_MODEL", "ViT-B-16-SigLIP2__webli")


def _predict(entries: dict, **kwargs) -> np.ndarray:
    r = requests.post(f"{ML_URL}/predict", data={"entries": json.dumps(entries), **kwargs.get("data", {})},
                      files=kwargs.get("files"), timeout=300)
    r.raise_for_status()
    v = np.array(json.loads(r.json()["clip"]), dtype=np.float32)
    return v / np.linalg.norm(v)


@functools.lru_cache(maxsize=4096)
def _text(text: str, model: str) -> bytes:
    return _predict({"clip": {"textual": {"modelName": model}}}, data={"text": text}).tobytes()


def embed_text(text: str, model: str = CLIP_MODEL) -> np.ndarray:
    return np.frombuffer(_text(text, model), dtype=np.float32)


def embed_image(jpeg_bytes: bytes, model: str = CLIP_MODEL) -> np.ndarray:
    return _predict({"clip": {"visual": {"modelName": model}}},
                    files={"image": ("image.jpg", jpeg_bytes, "image/jpeg")})
