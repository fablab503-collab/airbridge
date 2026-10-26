"""Face tools: find faces, recognise who they are, guess age, map face points.

All models are open source and run on the CPU:
  * SCRFD (det_10g)         - finds faces and 5 key points (InsightFace)
  * ArcFace (w600k_r50)     - turns a face into 512 numbers; same person = close numbers
  * genderage               - InsightFace age/sex guess (fallback)
  * age head (ours)         - small model we train on top of ArcFace numbers (see train_age.py)
  * MediaPipe Face Landmarker - 478 3D points on the face (mesh)
"""
from __future__ import annotations

import os
import pickle
from dataclasses import dataclass, field

import cv2
import numpy as np
import onnxruntime as ort

MODEL_DIR = os.environ.get("STUDIO_MODEL_DIR", "/models")

# Reference 5-point positions for a 112x112 aligned face (ArcFace standard).
ARCFACE_DST = np.array(
    [[38.2946, 51.6963], [73.5318, 51.5014], [56.0252, 71.7366], [41.5493, 92.3655], [70.7299, 92.2041]],
    dtype=np.float32,
)


@dataclass
class Face:
    box: np.ndarray            # x1, y1, x2, y2
    score: float
    kps: np.ndarray            # 5 x 2 key points
    embedding: np.ndarray | None = None
    age: float | None = None
    sex: str | None = None
    mesh: np.ndarray | None = None  # N x 3 landmark points
    extra: dict = field(default_factory=dict)


def _session(path: str) -> ort.InferenceSession:
    opts = ort.SessionOptions()
    opts.log_severity_level = 3
    return ort.InferenceSession(path, opts, providers=["CPUExecutionProvider"])


def _distance2bbox(points, distance):
    return np.stack([points[:, 0] - distance[:, 0], points[:, 1] - distance[:, 1],
                     points[:, 0] + distance[:, 2], points[:, 1] + distance[:, 3]], axis=-1)


def _distance2kps(points, distance):
    out = []
    for i in range(0, distance.shape[1], 2):
        out.append(points[:, i % 2] + distance[:, i])
        out.append(points[:, i % 2 + 1] + distance[:, i + 1])
    return np.stack(out, axis=-1)


def _nms(dets: np.ndarray, thresh: float = 0.4) -> list[int]:
    x1, y1, x2, y2, scores = dets.T
    areas = (x2 - x1 + 1) * (y2 - y1 + 1)
    order = scores.argsort()[::-1]
    keep = []
    while order.size > 0:
        i = order[0]
        keep.append(i)
        xx1 = np.maximum(x1[i], x1[order[1:]])
        yy1 = np.maximum(y1[i], y1[order[1:]])
        xx2 = np.minimum(x2[i], x2[order[1:]])
        yy2 = np.minimum(y2[i], y2[order[1:]])
        inter = np.maximum(0.0, xx2 - xx1 + 1) * np.maximum(0.0, yy2 - yy1 + 1)
        ovr = inter / (areas[i] + areas[order[1:]] - inter)
        order = order[np.where(ovr <= thresh)[0] + 1]
    return keep


class FaceDetector:
    """SCRFD face detector (InsightFace det_10g)."""

    def __init__(self, path: str | None = None, size: int = 640):
        self.sess = _session(path or os.path.join(MODEL_DIR, "buffalo_l", "det_10g.onnx"))
        self.input_name = self.sess.get_inputs()[0].name
        self.size = size
        self.strides = [8, 16, 32]
        self.anchors = 2

    def detect(self, img_bgr: np.ndarray, threshold: float = 0.5) -> list[Face]:
        h, w = img_bgr.shape[:2]
        scale = self.size / max(h, w)
        nh, nw = int(round(h * scale)), int(round(w * scale))
        canvas = np.zeros((self.size, self.size, 3), dtype=np.uint8)
        canvas[:nh, :nw] = cv2.resize(img_bgr, (nw, nh))
        blob = cv2.dnn.blobFromImage(canvas, 1.0 / 128, (self.size, self.size), (127.5, 127.5, 127.5), swapRB=True)
        outs = self.sess.run(None, {self.input_name: blob})
        n = len(self.strides)
        boxes, scores, kpss = [], [], []
        for idx, stride in enumerate(self.strides):
            sc = outs[idx][:, 0] if outs[idx].ndim == 2 else outs[idx][0][:, 0]
            bb = (outs[idx + n] if outs[idx + n].ndim == 2 else outs[idx + n][0]) * stride
            kp = (outs[idx + 2 * n] if outs[idx + 2 * n].ndim == 2 else outs[idx + 2 * n][0]) * stride
            fh = fw = self.size // stride
            centers = np.stack(np.mgrid[:fh, :fw][::-1], axis=-1).astype(np.float32)
            centers = (centers * stride).reshape(-1, 2)
            centers = np.repeat(centers, self.anchors, axis=0)
            pos = np.where(sc >= threshold)[0]
            boxes.append(_distance2bbox(centers, bb)[pos])
            scores.append(sc[pos])
            kpss.append(_distance2kps(centers, kp).reshape(-1, 5, 2)[pos])
        if not sum(len(s) for s in scores):
            return []
        boxes = np.vstack(boxes) / scale
        scores = np.concatenate(scores)
        kpss = np.vstack(kpss) / scale
        keep = _nms(np.hstack([boxes, scores[:, None]]))
        return [Face(box=boxes[i], score=float(scores[i]), kps=kpss[i]) for i in keep]


def align(img_bgr: np.ndarray, kps: np.ndarray, size: int = 112) -> np.ndarray:
    """Rotate/scale the face so the eyes, nose and mouth sit at standard spots."""
    dst = ARCFACE_DST * (size / 112.0)
    m, _ = cv2.estimateAffinePartial2D(kps.astype(np.float32), dst, method=cv2.LMEDS)
    return cv2.warpAffine(img_bgr, m, (size, size), borderValue=0)


class FaceRecognizer:
    """ArcFace identity embeddings. Same person -> cosine similarity close to 1."""

    def __init__(self, path: str | None = None):
        self.sess = _session(path or os.path.join(MODEL_DIR, "buffalo_l", "w600k_r50.onnx"))
        self.input_name = self.sess.get_inputs()[0].name

    def embed(self, aligned_bgr_list: list[np.ndarray]) -> np.ndarray:
        blob = cv2.dnn.blobFromImages(aligned_bgr_list, 1.0 / 127.5, (112, 112), (127.5, 127.5, 127.5), swapRB=True)
        out = self.sess.run(None, {self.input_name: blob})[0]
        return out / np.linalg.norm(out, axis=1, keepdims=True)


class GenderAge:
    """InsightFace genderage model (96x96 input)."""

    def __init__(self, path: str | None = None):
        self.sess = _session(path or os.path.join(MODEL_DIR, "buffalo_l", "genderage.onnx"))
        self.input_name = self.sess.get_inputs()[0].name

    def predict(self, img_bgr: np.ndarray, box: np.ndarray) -> tuple[float, str]:
        x1, y1, x2, y2 = box
        w, h = x2 - x1, y2 - y1
        center = ((x1 + x2) / 2, (y1 + y2) / 2)
        scale = 96 / (max(w, h) * 1.5)
        m = np.array([[scale, 0, 48 - center[0] * scale], [0, scale, 48 - center[1] * scale]], dtype=np.float32)
        crop = cv2.warpAffine(img_bgr, m, (96, 96), borderValue=0)
        blob = cv2.dnn.blobFromImage(crop, 1.0, (96, 96), (0, 0, 0), swapRB=True)
        out = self.sess.run(None, {self.input_name: blob})[0][0]
        return float(out[2] * 100), ("female" if out[0] > out[1] else "male")


class AgeHead:
    """Our own age model: ridge regression on SigLIP2 numbers of the aligned face, trained on
    UTKFace (eval/train_age.py). On FG-NET it is off by 5.4 years on average vs 12.9 for genderage."""

    def __init__(self, path: str | None = None):
        path = path or os.path.join(MODEL_DIR, "age_head_siglip2.pkl")
        saved = pickle.load(open(path, "rb")) if os.path.exists(path) else None
        self.model = saved["model"] if saved else None
        self.clip_model = saved["clip_model"] if saved else None

    def predict(self, aligned_bgr_list: list[np.ndarray]) -> np.ndarray | None:
        if self.model is None:
            return None
        from . import clip
        embs = np.stack([clip.embed_image(cv2.imencode(".jpg", c)[1].tobytes(), self.clip_model)
                         for c in aligned_bgr_list])
        return np.clip(self.model.predict(embs), 0, 100)


class Landmarks106:
    """InsightFace 2d106det: 106 outline points (jaw, brows, eyes, nose, lips). Together with the
    478-point mesh this gives 584 points per face."""

    def __init__(self, path: str | None = None):
        self.sess = _session(path or os.path.join(MODEL_DIR, "buffalo_l", "2d106det.onnx"))
        self.input_name = self.sess.get_inputs()[0].name

    def points(self, img_bgr: np.ndarray, box: np.ndarray) -> np.ndarray:
        x1, y1, x2, y2 = box
        cx, cy = (x1 + x2) / 2, (y1 + y2) / 2
        s = 192 / (max(x2 - x1, y2 - y1) * 1.5)
        m = np.array([[s, 0, 96 - cx * s], [0, s, 96 - cy * s]], dtype=np.float32)
        crop = cv2.warpAffine(img_bgr, m, (192, 192), borderValue=0)
        blob = cv2.dnn.blobFromImage(crop, 1.0, (192, 192), (0, 0, 0), swapRB=True)
        out = self.sess.run(None, {self.input_name: blob})[0].reshape(-1, 2)
        pts = (out + 1) * 96  # model gives -1..1 inside the 192px crop
        return np.stack([(pts[:, 0] - m[0, 2]) / s, (pts[:, 1] - m[1, 2]) / s], axis=1)


class FaceMesh:
    """MediaPipe Face Landmarker: 478 points (468 face mesh + 10 iris)."""

    def __init__(self, path: str | None = None):
        import mediapipe as mp
        from mediapipe.tasks.python import vision
        from mediapipe.tasks.python.core.base_options import BaseOptions

        self.mp = mp
        path = path or os.path.join(MODEL_DIR, "face_landmarker.task")
        self.landmarker = vision.FaceLandmarker.create_from_options(
            vision.FaceLandmarkerOptions(base_options=BaseOptions(model_asset_path=path), num_faces=20))

    def points(self, img_bgr: np.ndarray) -> list[np.ndarray]:
        rgb = cv2.cvtColor(img_bgr, cv2.COLOR_BGR2RGB)
        res = self.landmarker.detect(self.mp.Image(image_format=self.mp.ImageFormat.SRGB, data=rgb))
        h, w = img_bgr.shape[:2]
        return [np.array([[p.x * w, p.y * h, p.z * w] for p in lm], dtype=np.float32) for lm in res.face_landmarks]


class AgeInvariant:
    """Extra layer trained on FG-NET (same people at ages 0-69) that keeps the parts of the
    face fingerprint that stay the same as people grow up (eval/train_age_invariant.py)."""

    def __init__(self, path: str | None = None):
        path = path or os.path.join(MODEL_DIR, "age_invariant_antelopev2.pkl")
        self.model = pickle.load(open(path, "rb")) if os.path.exists(path) else None

    def __call__(self, embeddings: np.ndarray) -> np.ndarray:
        if self.model is None:
            return embeddings
        z = self.model.transform(embeddings)
        return z / np.linalg.norm(z, axis=1, keepdims=True)


class FacePipeline:
    def __init__(self, with_mesh: bool = False):
        self.detector = FaceDetector()
        # antelopev2 (ArcFace R100) recognised people across ages better than Immich's default R50
        strong = os.path.join(MODEL_DIR, "antelopev2", "glintr100.onnx")
        self.recognizer = FaceRecognizer(strong if os.path.exists(strong) else None)
        self.age_invariant = AgeInvariant()
        self.genderage = GenderAge()
        self.age_head = AgeHead()
        self.mesh = FaceMesh() if with_mesh else None
        self.outline = Landmarks106() if with_mesh else None

    def analyze(self, img_bgr: np.ndarray, min_score: float = 0.6) -> list[Face]:
        faces = [f for f in self.detector.detect(img_bgr) if f.score >= min_score]
        if not faces:
            return faces
        crops = [align(img_bgr, f.kps) for f in faces]
        raw = self.recognizer.embed(crops)
        embs = self.age_invariant(raw)
        try:
            ages = self.age_head.predict(crops)
        except Exception:  # ML server unreachable: fall back to InsightFace's guess
            ages = None
        for i, f in enumerate(faces):
            f.embedding = embs[i]
            f.extra["raw_embedding"] = raw[i]  # kept so the layer can be retrained on your confirmations
            ga_age, f.sex = self.genderage.predict(img_bgr, f.box)
            f.age = float(ages[i]) if ages is not None else ga_age
            f.extra["age_insightface"] = ga_age
        if self.mesh is not None:
            meshes = self.mesh.points(img_bgr)
            for f in faces:  # match each mesh to the face box containing its nose tip
                for m in meshes:
                    nx, ny = m[1, 0], m[1, 1]
                    if f.box[0] <= nx <= f.box[2] and f.box[1] <= ny <= f.box[3]:
                        f.mesh = m
                        break
        if self.outline is not None:
            for f in faces:
                f.extra["points106"] = self.outline.points(img_bgr, f.box)
        return faces
