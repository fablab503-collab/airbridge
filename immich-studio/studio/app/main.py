"""Immich Studio - extra AI tools that plug into Immich.

Runs next to Immich behind the same address (see gateway/nginx.conf), so it can
use the visitor's own Immich login cookie: no extra password, and every user
only ever sees their own photos.
"""
from __future__ import annotations

import io
import json
import os
import threading
import traceback
import uuid
from datetime import datetime

import cv2
import numpy as np
from fastapi import FastAPI, File, HTTPException, Request, UploadFile
from fastapi.responses import FileResponse, JSONResponse, Response
from fastapi.staticfiles import StaticFiles
from PIL import Image

from . import clip, editor, formats, printing
from .immich import Immich

HERE = os.path.dirname(__file__)
DATA_DIR = os.environ.get("STUDIO_DATA_DIR", "/data")
GRAPHITE_DIR = os.environ.get("STUDIO_GRAPHITE_DIR", "/graphite")
os.makedirs(DATA_DIR, exist_ok=True)

app = FastAPI(title="Immich Studio")
_lazy: dict = {}
_lock = threading.Lock()
JOBS: dict[str, dict] = {}


def lazy(name: str):
    """Load heavy AI models only the first time they are needed."""
    with _lock:
        if name not in _lazy:
            if name == "labeler":
                from .labels import Labeler
                _lazy[name] = Labeler()
            elif name == "faces":
                from .faces import FacePipeline
                _lazy[name] = FacePipeline(with_mesh=True)
            elif name == "upscaler":
                _lazy[name] = printing.Upscaler()
        return _lazy[name]


def immich_for(request: Request) -> Immich:
    token = request.cookies.get("immich_access_token")
    if token:
        return Immich(token=token)
    key = request.headers.get("x-api-key") or os.environ.get("IMMICH_API_KEY")
    if key:
        return Immich(api_key=key)
    raise HTTPException(401, "Please log in to Immich first.")


def run_job(kind: str, fn, *args) -> str:
    job_id = uuid.uuid4().hex[:12]
    JOBS[job_id] = {"id": job_id, "kind": kind, "status": "running", "progress": 0, "result": None,
                    "started": datetime.now().isoformat()}

    def work():
        try:
            JOBS[job_id]["result"] = fn(JOBS[job_id], *args)
            JOBS[job_id]["status"] = "done"
        except Exception as e:  # report the error to the page instead of crashing
            traceback.print_exc()
            JOBS[job_id].update(status="error", error=str(e))

    threading.Thread(target=work, daemon=True).start()
    return job_id


@app.get("/api/jobs/{job_id}")
def job(job_id: str):
    if job_id not in JOBS:
        raise HTTPException(404)
    j = {k: v for k, v in JOBS[job_id].items() if k != "file"}
    if "file" in JOBS[job_id]:
        j["file_url"] = f"api/jobs/{job_id}/file"
    return j


@app.get("/api/jobs/{job_id}/file")
def job_file(job_id: str):
    j = JOBS.get(job_id)
    if not j or j["status"] != "done":
        raise HTTPException(404)
    data, media, name = j["file"]
    return Response(data, media_type=media, headers={"Content-Disposition": f'attachment; filename="{name}"'})


# ---------------------------------------------------------------- photos ---
@app.get("/api/me")
def me(request: Request):
    im = immich_for(request)
    return im._req("GET", "/users/me").json()


@app.get("/api/assets")
def assets(request: Request, page: int = 1, size: int = 60, type: str | None = None, album: str | None = None):
    filters = {}
    if type:
        filters["type"] = type
    if album:
        filters["albumIds"] = [album]
    items, nxt = immich_for(request).assets(page=page, size=size, **filters)
    return {"items": [{"id": a["id"], "name": a["originalFileName"], "type": a["type"],
                       "tags": [t["value"] for t in a.get("tags") or []]} for a in items], "next": nxt}


@app.get("/api/albums")
def albums(request: Request):
    return [{"id": a["id"], "name": a["albumName"], "count": a["assetCount"]} for a in immich_for(request).albums()]


@app.get("/api/thumb/{asset_id}")
def thumb(asset_id: str, request: Request, size: str = "thumbnail"):
    return Response(immich_for(request).preview(asset_id, size), media_type="image/jpeg",
                    headers={"Cache-Control": "private, max-age=3600"})


@app.get("/api/original/{asset_id}")
def original(asset_id: str, request: Request):
    im = immich_for(request)
    a = im.asset(asset_id)
    data = im.original(asset_id)
    name = a["originalFileName"]
    if formats.ext_of(name) in {".jpg", ".jpeg", ".png", ".webp"}:
        return Response(data, media_type=a.get("originalMimeType") or "image/jpeg")
    return Response(formats.to_jpeg(formats.open_any(data, name), quality=95), media_type="image/jpeg")


@app.delete("/api/asset/{asset_id}")
def delete_asset(asset_id: str, request: Request, forever: bool = False):
    immich_for(request).delete([asset_id], forever=forever)
    return {"ok": True, "trash": not forever}


def _load(im: Immich, asset_id: str, full: bool = False) -> tuple[Image.Image, dict]:
    a = im.asset(asset_id)
    if full:
        return formats.open_any(im.original(asset_id), a["originalFileName"]), a
    return Image.open(io.BytesIO(im.preview(asset_id, "preview"))).convert("RGB"), a


# ------------------------------------------------------------- labelling ---
AGE_GROUPS = [(3, "Baby (0-2)"), (13, "Child (3-12)"), (20, "Teen (13-19)"), (30, "Young adult (20-29)"),
              (45, "Adult (30-44)"), (65, "Middle age (45-64)"), (200, "Senior (65+)")]


STUDIO_TAG_ROOTS = ["Environment", "Setting", "Light", "People", "Era", "Clothing", "Clothing color",
                    "Clothing style", "Age", "Year"]


def age_group(age: float) -> str:
    return next(name for limit, name in AGE_GROUPS if age < limit)


def analyze(im: Immich, asset_id: str, write: bool = True) -> dict:
    img, a = _load(im, asset_id)
    jpeg = formats.to_jpeg(img, 1024)
    emb = clip.embed_image(jpeg)
    labels = lazy("labeler").label(emb)
    bgr = cv2.cvtColor(np.asarray(img), cv2.COLOR_RGB2BGR)
    faces = lazy("faces").analyze(bgr)
    # clothes are judged on each person's body area (face box stretched downwards)
    if faces:
        h, w = bgr.shape[:2]
        clothing_votes: dict[str, dict[str, float]] = {}
        for f in faces:
            x1, y1, x2, y2 = f.box
            fw, fh = x2 - x1, y2 - y1
            body = img.crop((max(0, int(x1 - fw)), max(0, int(y1)), min(w, int(x2 + fw)), min(h, int(y2 + fh * 5))))
            if body.width < 40 or body.height < 60:
                continue
            bemb = clip.embed_image(formats.to_jpeg(body, 512))
            for g in ("Clothing", "Clothing color", "Clothing style"):
                for n, s in lazy("labeler").scores(g, bemb).items():
                    clothing_votes.setdefault(g, {}).setdefault(n, 0)
                    clothing_votes[g][n] += s / len(faces)
        from .labels import MULTI
        for g, votes in clothing_votes.items():
            ranked = sorted(votes.items(), key=lambda kv: -kv[1])
            k = MULTI.get(g, 1)
            labels[g] = [(n, s) for n, s in ranked[:k] if s >= ranked[0][1] * 0.5]
        n = len(faces)
        labels["People"] = [("One person" if n == 1 else "Two people" if n == 2 else
                             "Group" if n <= 6 else "Crowd", 1.0)]
    exif = a.get("exifInfo") or {}
    taken = exif.get("dateTimeOriginal")
    # Without camera details Immich falls back to the file date, which is not when the photo was taken
    year = int(taken[:4]) if taken and (exif.get("make") or exif.get("model")) else None
    face_out = [{"box": [round(float(v)) for v in f.box], "age": round(f.age, 1), "age_group": age_group(f.age),
                 "sex": f.sex, "mesh_points": face_point_count(f)} for f in faces]
    result = {"asset": asset_id, "labels": {g: [(n, round(s, 3)) for n, s in v] for g, v in labels.items()},
              "faces": face_out, "year_from_camera": year}
    if write:
        tags = [f"{g}/{n}" for g, v in labels.items() for n, _ in v]
        tags += [f"Age/{f['age_group']}" for f in face_out]
        if year:
            tags.append(f"Year/{year}")
        for prefix in STUDIO_TAG_ROOTS:  # replace earlier Studio tags, keep the user's own
            im.remove_tag_prefix(asset_id, prefix + "/")
        ids = im.upsert_tags(sorted(set(tags)))
        im.tag_assets(list(ids.values()), [asset_id])
        summary = "; ".join(f"{g}: {', '.join(n for n, _ in v)}" for g, v in labels.items() if v)
        if face_out:
            summary += "; Ages: " + ", ".join(f"~{f['age']:.0f}" for f in face_out)
        old = (a.get("exifInfo") or {}).get("description") or ""
        old = old.split("\n\n[Immich Studio]")[0]
        im.set_description(asset_id, (old + "\n\n" if old else "") + "[Immich Studio] " + summary)
        result["tags"] = sorted(set(tags))
    return result


@app.post("/api/analyze/{asset_id}")
def analyze_one(asset_id: str, request: Request, write: bool = True):
    return analyze(immich_for(request), asset_id, write)


@app.post("/api/analyze-all")
def analyze_all(request: Request):
    im = immich_for(request)

    def work(job):
        photos = im.all_assets(type="IMAGE")
        done = []
        for i, a in enumerate(photos):
            try:
                analyze(im, a["id"])
                done.append(a["id"])
            except Exception as e:
                print("analyze failed", a["id"], e)
            job["progress"] = round(100 * (i + 1) / len(photos))
        return {"labelled": len(done), "total": len(photos)}

    return {"job": run_job("analyze-all", work)}


@app.post("/api/train")
def train(request: Request):
    """Learn from albums named "Train: <Group>/<Label>" (e.g. "Train: Clothing/Uniform")."""
    im = immich_for(request)
    from sklearn.linear_model import LogisticRegression
    import pickle

    def work(job):
        examples: dict[str, list[tuple[str, str]]] = {}
        for al in im.albums():
            name = al["albumName"]
            if name.startswith("Train: ") and "/" in name:
                group, label = name[7:].split("/", 1)
                for aid in im.album_asset_ids(al["id"]):
                    examples.setdefault(group.strip(), []).append((aid, label.strip()))
        heads_path = os.path.join(os.environ.get("STUDIO_MODEL_DIR", "/models"), "label_heads.pkl")
        heads = pickle.load(open(heads_path, "rb")) if os.path.exists(heads_path) else {}
        report = {}
        for group, ex in examples.items():
            labels = {l for _, l in ex}
            if len(labels) < 2:
                report[group] = "needs at least 2 labels"
                continue
            X = np.stack([clip.embed_image(im.preview(aid, "preview")) for aid, _ in ex])
            y = [l for _, l in ex]
            heads[group] = LogisticRegression(max_iter=3000, C=5).fit(X, y)
            report[group] = {"examples": len(y), "labels": sorted(labels)}
        pickle.dump(heads, open(heads_path, "wb"))
        lazy("labeler").trained = heads
        return report

    return {"job": run_job("train", work)}


# ----------------------------------------------------------------- faces ---
@app.get("/api/faces/{asset_id}")
def faces(asset_id: str, request: Request):
    img, _ = _load(immich_for(request), asset_id)
    found = lazy("faces").analyze(cv2.cvtColor(np.asarray(img), cv2.COLOR_RGB2BGR))
    return {"width": img.width, "height": img.height,
            "faces": [{"box": [float(v) for v in f.box], "age": round(f.age, 1), "age_group": age_group(f.age),
                       "sex": f.sex, "score": round(f.score, 3),
                       "mesh": None if f.mesh is None else np.round(f.mesh[:, :2], 1).tolist(),
                       "outline": np.round(f.extra["points106"], 1).tolist() if "points106" in f.extra else None,
                       "points": face_point_count(f)} for f in found]}


def face_point_count(f) -> int:
    return (0 if f.mesh is None else len(f.mesh)) + len(f.extra.get("points106", []))


def _face_index_path(im: Immich, kind: str = "faces", ext: str = "npz") -> str:
    uid = im._req("GET", "/users/me").json()["id"]  # one index per Immich user
    return os.path.join(DATA_DIR, f"{kind}-{uid}.{ext}")


# Similarity levels, measured on FG-NET people the model never saw (eval/README):
CERTAIN = 0.72  # above the highest wrong-person score in ~282 000 wrong pairs: 0 mistakes seen
LIKELY = 0.36   # about 1 wrong match in 100


def _confirmations(im: Immich) -> dict:
    path = _face_index_path(im, "confirmed", "json")
    return json.load(open(path)) if os.path.exists(path) else {"same": [], "different": []}


def _same_groups(pairs: list[list[str]]) -> dict[str, str]:
    """Join confirmed pairs into people: if A=B and B=C then A=C."""
    parent: dict[str, str] = {}

    def root(k):
        parent.setdefault(k, k)
        while parent[k] != k:
            parent[k] = parent[parent[k]]
            k = parent[k]
        return k

    for a, b in pairs:
        parent[root(a)] = root(b)
    return {k: root(k) for k in parent}


@app.post("/api/faces/index")
def index_faces(request: Request):
    """Fingerprint every face in the library (stronger model + age-ignoring layer)."""
    im = immich_for(request)
    path = _face_index_path(im)

    def work(job):
        photos = im.all_assets(type="IMAGE")
        rows, embs, raws = [], [], []
        for i, a in enumerate(photos):
            try:
                img, _ = _load(im, a["id"])
                for n, f in enumerate(lazy("faces").analyze(cv2.cvtColor(np.asarray(img), cv2.COLOR_RGB2BGR))):
                    rows.append((f"{a['id']}:{n}", f.age))
                    embs.append(f.embedding)
                    raws.append(f.extra["raw_embedding"])
            except Exception as e:
                print("face index failed", a["id"], e)
            job["progress"] = round(100 * (i + 1) / len(photos))
        np.savez_compressed(path, rows=np.array(rows, dtype=object), embs=np.array(embs, dtype=np.float32),
                            raws=np.array(raws, dtype=np.float32))
        return {"photos": len(photos), "faces": len(rows)}

    return {"job": run_job("index-faces", work)}


@app.post("/api/faces/find")
async def find_person(request: Request):
    """Photos of the same person, best match first - also across many years of age difference."""
    body = await request.json()
    im = immich_for(request)
    path = _face_index_path(im)
    if not os.path.exists(path):
        raise HTTPException(409, "Build the face index first (Label & train → Index all faces).")
    idx = np.load(path, allow_pickle=True)
    img, _ = _load(im, body["asset_id"])
    faces = lazy("faces").analyze(cv2.cvtColor(np.asarray(img), cv2.COLOR_RGB2BGR))
    face = faces[int(body.get("face", 0))]
    me_key = f"{body['asset_id']}:{int(body.get('face', 0))}"
    conf = _confirmations(im)
    groups = _same_groups(conf["same"])
    rejected = {frozenset(p) for p in conf["different"]}
    sims = idx["embs"] @ face.embedding
    best: dict[str, tuple[float, str, float]] = {}
    for (key, age), s in zip(idx["rows"], sims):
        aid = key.split(":")[0]
        if aid != body["asset_id"] and s > best.get(aid, (-1, "", 0))[0]:
            best[aid] = (float(s), key, float(age))
    ranked = sorted(best.items(), key=lambda kv: -kv[1][0])[: int(body.get("limit", 30))]

    def level(key: str, s: float) -> str:
        if me_key in groups and groups.get(key) == groups[me_key]:
            return "confirmed"  # you said so
        if frozenset((me_key, key)) in rejected:
            return "rejected"
        return "certain" if s >= CERTAIN else "likely" if s >= LIKELY else "unsure"

    out = [{"asset": aid, "face": key, "similarity": round(s, 3), "age": round(age, 1), "level": level(key, s)}
           for aid, (s, key, age) in ranked]
    for m in out:
        m["likely_same"] = m["level"] in ("confirmed", "certain", "likely")
    return {"age_here": round(face.age, 1), "face": me_key, "matches": out}


@app.post("/api/faces/confirm")
async def confirm_face(request: Request):
    """Remember your answer: are these two faces the same person? ({"a": key, "b": key, "same": bool})"""
    body = await request.json()
    im = immich_for(request)
    conf = _confirmations(im)
    pair = sorted([body["a"], body["b"]])
    for k in ("same", "different"):
        conf[k] = [p for p in conf[k] if sorted(p) != pair]
    conf["same" if body["same"] else "different"].append(pair)
    json.dump(conf, open(_face_index_path(im, "confirmed", "json"), "w"))
    return {"same": len(conf["same"]), "different": len(conf["different"])}


@app.post("/api/faces/learn")
def learn_faces(request: Request):
    """Retrain the age-ignoring layer on FG-NET plus the people you confirmed."""
    im = immich_for(request)
    from sklearn.decomposition import PCA
    from sklearn.discriminant_analysis import LinearDiscriminantAnalysis
    from sklearn.pipeline import make_pipeline
    import pickle

    def work(job):
        idx = np.load(_face_index_path(im), allow_pickle=True)
        keys = [k for k, _ in idx["rows"]]
        groups = _same_groups(_confirmations(im)["same"])
        mine = [(i, groups[k]) for i, k in enumerate(keys) if k in groups]
        base = np.load(os.path.join(os.environ.get("STUDIO_TRAINED_DIR", "/srv/trained"), "fgnet_antelopev2.npz"))
        X = np.vstack([base["embs"], idx["raws"][[i for i, _ in mine]]]) if mine else base["embs"]
        y = list(base["people"].astype(str)) + [f"you:{g}" for _, g in mine]
        model = make_pipeline(PCA(n_components=200), LinearDiscriminantAnalysis(solver="eigen", shrinkage=0.3))
        model.fit(X, y)
        pickle.dump(model, open(os.path.join(os.environ.get("STUDIO_MODEL_DIR", "/models"),
                                             "age_invariant_antelopev2.pkl"), "wb"))
        lazy("faces").age_invariant.model = model
        return {"your_faces": len(mine), "your_people": len({g for _, g in mine}),
                "note": "Run 'Index all faces' again so the whole library uses the new layer."}

    return {"job": run_job("learn-faces", work)}


# ---------------------------------------------------------------- editing ---
@app.post("/api/edit/{asset_id}")
async def edit(asset_id: str, request: Request):
    body = await request.json()
    im = immich_for(request)
    preview_only = body.get("preview", True)
    img, a = _load(im, asset_id, full=not preview_only)
    out = editor.apply(img, body.get("steps", []))
    if preview_only:
        return Response(formats.to_jpeg(out, 1400), media_type="image/jpeg")
    data, ext = formats.save_as(out, body.get("format", "jpeg"))
    stem = os.path.splitext(a["originalFileName"])[0]
    new_id = im.upload(data, f"{stem}-edited{ext}", created=a.get("fileCreatedAt"))
    if body.get("replace"):
        im.delete([asset_id])
    return {"new_asset": new_id, "original_in_trash": bool(body.get("replace"))}


@app.post("/api/graphite-return/{asset_id}")
async def graphite_return(asset_id: str, request: Request, file: UploadFile = File(...)):
    """Graphite (patched) posts its exported picture here; we store it in Immich next to the original."""
    im = immich_for(request)
    a = im.asset(asset_id)
    stem = os.path.splitext(a["originalFileName"])[0]
    ext = formats.ext_of(file.filename or ".png") or ".png"
    new_id = im.upload(await file.read(), f"{stem}-graphite{ext}", created=a.get("fileCreatedAt"))
    return {"new_asset": new_id}


# ------------------------------------------------------------------ brand ---
BRAND_DIR = os.path.join(DATA_DIR, "brand")


@app.get("/api/brand")
def brand():
    """Public on purpose: the login page shows the logo before anyone is logged in."""
    path = os.path.join(BRAND_DIR, "logo.png")
    if not os.path.exists(path):
        return {"logo": None}
    meta = json.load(open(os.path.join(BRAND_DIR, "brand.json"))) if os.path.exists(os.path.join(BRAND_DIR, "brand.json")) else {}
    return {"logo": f"api/brand/logo.png?v={int(os.path.getmtime(path))}", "name": meta.get("name")}


@app.get("/api/brand/{name}")
def brand_file(name: str):
    if name not in ("logo.png", "icon-180.png", "icon-512.png", "favicon-32.png"):
        raise HTTPException(404)
    path = os.path.join(BRAND_DIR, name)
    if not os.path.exists(path):
        raise HTTPException(404)
    return FileResponse(path, media_type="image/png", headers={"Cache-Control": "public, max-age=300"})


@app.post("/api/brand")
async def set_brand(request: Request, file: UploadFile | None = File(None)):
    """Admin only: upload your own logo (any image format) and optionally a name to show instead of "Immich"."""
    im = immich_for(request)
    if not im._req("GET", "/users/me").json().get("isAdmin"):
        raise HTTPException(403, "Only the Immich admin can change the logo.")
    form = await request.form()
    os.makedirs(BRAND_DIR, exist_ok=True)
    if form.get("reset"):
        for f in os.listdir(BRAND_DIR):
            os.remove(os.path.join(BRAND_DIR, f))
        return {"logo": None}
    if file is not None:
        img = formats.open_any(await file.read(), file.filename or "logo.png") if not (file.filename or "").lower().endswith(".png") \
            else Image.open(io.BytesIO(await file.read()))
        img = img.convert("RGBA")
        img.thumbnail((1200, 400))
        img.save(os.path.join(BRAND_DIR, "logo.png"))
        # square icons (browser tab, iPhone home screen): logo centred on its own background
        side = max(img.size)
        square = Image.new("RGBA", (side, side), (255, 255, 255, 255))
        square.paste(img, ((side - img.width) // 2, (side - img.height) // 2), img)
        for size, name in ((180, "icon-180.png"), (512, "icon-512.png"), (32, "favicon-32.png")):
            square.resize((size, size), Image.LANCZOS).save(os.path.join(BRAND_DIR, name))
    name = (form.get("name") or "").strip()[:40]
    json.dump({"name": name or None}, open(os.path.join(BRAND_DIR, "brand.json"), "w"))
    return brand()


# ----------------------------------------------------------------- camera ---
CAMERA_ALBUM = os.environ.get("STUDIO_CAMERA_ALBUM", "Studio Camera")


@app.post("/api/camera/upload")
async def camera_upload(request: Request, file: UploadFile = File(...), taken: str | None = None):
    """A photo or video straight from the Studio camera: it goes into Immich and is never kept on the phone."""
    im = immich_for(request)
    form = await request.form()
    taken = form.get("taken") or taken
    data = await file.read()
    if not data:
        raise HTTPException(400, "Empty file")
    asset_id = im.upload(data, file.filename or "camera.jpg", created=taken)
    im.ensure_album(CAMERA_ALBUM, [asset_id])
    return {"asset": asset_id, "album": CAMERA_ALBUM, "bytes": len(data)}


# --------------------------------------------------------------- formats ---
@app.post("/api/import")
async def import_file(request: Request, file: UploadFile = File(...)):
    """Upload any file. Formats Immich can't show are converted (16-bit TIFF keeps full quality)."""
    im = immich_for(request)
    data = await file.read()
    name = file.filename or "file"
    if formats.ext_of(name) in formats.IMMICH_NATIVE:
        return {"asset": im.upload(data, name), "converted": False}
    img = formats.open_any(data, name)
    out, ext = formats.save_as(img, "tiff")
    return {"asset": im.upload(out, os.path.splitext(name)[0] + ext), "converted": True,
            "size": [img.width, img.height]}


# --------------------------------------------------------------- printing ---
def _print_source(im: Immich, asset_id: str) -> tuple[Image.Image, dict]:
    a = im.asset(asset_id)
    data = im.original(asset_id)
    if formats.is_raw(a["originalFileName"]):
        return Image.fromarray(formats.develop_raw(data)), a  # full-resolution RAW development
    return formats.open_any(data, a["originalFileName"]), a


@app.get("/api/print/{asset_id}")
def print_check(asset_id: str, request: Request):
    a = immich_for(request).asset(asset_id)
    exif = a.get("exifInfo") or {}
    w, h = exif.get("exifImageWidth"), exif.get("exifImageHeight")
    if not w:
        img, _ = _print_source(immich_for(request), asset_id)
        w, h = img.size
    return {"pixels": [w, h], "megapixels": round(w * h / 1e6, 1), "raw": formats.is_raw(a["originalFileName"]),
            "papers": printing.print_report(w, h)}


@app.post("/api/print/{asset_id}")
async def print_make(asset_id: str, request: Request):
    body = await request.json()
    im = immich_for(request)
    paper = body.get("paper", "A4")

    def work(job):
        img, a = _print_source(im, asset_id)
        if body.get("steps"):
            img = editor.apply(img, body["steps"])
        up = body.get("upscale", "auto")
        if up == "auto":
            up = next(r["upscale"] for r in printing.print_report(*img.size) if r["paper"] == paper)
        up = int(up)
        job["progress"] = 10
        if up > 1:
            img = lazy("upscaler").upscale(img, scale=up)
        job["progress"] = 80
        pdf = printing.print_pdf(img, paper, body.get("fit", "fill"), float(body.get("margin_mm", 0)),
                                 bool(body.get("crop_marks")), a["originalFileName"])
        dpi = printing.dpi_on_paper(img.width, img.height, paper)
        job["file"] = (pdf, "application/pdf", f"{os.path.splitext(a['originalFileName'])[0]}-{paper}.pdf")
        return {"paper": paper, "upscaled": up, "pixels": list(img.size), "dpi": round(dpi)}

    return {"job": run_job("print", work)}


@app.post("/api/book")
async def book(request: Request):
    body = await request.json()
    im = immich_for(request)

    def work(job):
        ids = body.get("asset_ids") or im.album_asset_ids(body["album_id"])
        photos = []
        for i, aid in enumerate(ids):
            img, a = _load(im, aid, full=bool(body.get("full_quality")))
            exif = a.get("exifInfo") or {}
            date = (exif.get("dateTimeOriginal") or a.get("fileCreatedAt") or "")[:10]
            place = ", ".join(x for x in [exif.get("city"), exif.get("country")] if x)
            desc = (exif.get("description") or "").split("\n\n[Immich Studio]")[0]
            photos.append((img, " · ".join(x for x in [desc, place, date] if x)))
            job["progress"] = round(90 * (i + 1) / len(ids))
        pdf = printing.book_pdf(photos, body.get("paper", "A4"), int(body.get("per_page", 2)),
                                body.get("title", "Photo book"), body.get("subtitle", ""))
        job["file"] = (pdf, "application/pdf", f"{body.get('title', 'Photo book')}.pdf")
        return {"pages": 1 + -(-len(photos) // int(body.get("per_page", 2)))}

    return {"job": run_job("book", work)}


# ------------------------------------------------------------------ video ---
@app.post("/api/video/{asset_id}/transcribe")
def transcribe(asset_id: str, request: Request):
    im = immich_for(request)

    def work(job):
        from . import video
        a = im.asset(asset_id)
        res = video.transcribe(im.original(asset_id), a["originalFileName"])
        open(os.path.join(DATA_DIR, f"{asset_id}.srt"), "w").write(res["srt"])
        if res["text"]:
            old = ((a.get("exifInfo") or {}).get("description") or "").split("\n\n[Transcript]")[0]
            im.set_description(asset_id, (old + "\n\n" if old else "") + "[Transcript] " + res["text"][:1900])
            ids = im.upsert_tags([f"Video/Speech/{res['language']}"])
            im.tag_assets(list(ids.values()), [asset_id])
        return {k: res[k] for k in ("language", "text", "segments")}

    return {"job": run_job("transcribe", work)}


@app.get("/api/video/{asset_id}/subtitles.srt")
def subtitles(asset_id: str):
    path = os.path.join(DATA_DIR, f"{asset_id}.srt")
    if not os.path.exists(path):
        raise HTTPException(404, "Transcribe the video first.")
    return FileResponse(path, media_type="text/plain", filename=f"{asset_id}.srt")


# ------------------------------------------------------------------ pages ---
@app.get("/api/health")
def health():
    return {"ok": True, "graphite": os.path.exists(os.path.join(GRAPHITE_DIR, "index.html"))}


if os.path.isdir(GRAPHITE_DIR):
    app.mount("/graphite", StaticFiles(directory=GRAPHITE_DIR, html=True), name="graphite")
app.mount("/", StaticFiles(directory=os.path.join(HERE, "static"), html=True), name="static")
