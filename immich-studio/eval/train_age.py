"""Train an age model on UTKFace and test it on FG-NET (different people, all ages).

Compares three ways of guessing age:
  1. InsightFace genderage (ready-made, what most tools use)
  2. Our ridge model on ArcFace face numbers
  3. Our ridge model on SigLIP2 numbers of the aligned face (via Immich ML server)

Usage: python3 train_age.py UTK_PARQUET FGNET_DIR MODEL_DIR ML_URL OUT_DIR
"""
import glob, json, os, pickle, re, sys
from concurrent.futures import ThreadPoolExecutor

import cv2, numpy as np, pyarrow.parquet as pq, requests
from sklearn.linear_model import RidgeCV

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "pystudio"))
utk_path, fgnet_dir, model_dir, ml_url, out_dir = sys.argv[1:6]
os.environ["STUDIO_MODEL_DIR"] = model_dir
from app.faces import FaceDetector, FaceRecognizer, GenderAge, align  # noqa: E402

CLIP_MODEL = "ViT-B-16-SigLIP2__webli"
det, rec, ga = FaceDetector(size=320), FaceRecognizer(), GenderAge()  # UTK faces fill the frame: small input is enough


def clip_embed(img_bgr):
    ok, jpg = cv2.imencode(".jpg", img_bgr)
    r = requests.post(f"{ml_url}/predict", files={"image": ("f.jpg", jpg.tobytes(), "image/jpeg")},
                      data={"entries": '{"clip":{"visual":{"modelName":"%s"}}}' % CLIP_MODEL})
    r.raise_for_status()
    return np.array(json.loads(r.json()["clip"]), dtype=np.float32)


def faces_from_utk(limit=4000):
    t = pq.read_table(utk_path, columns=["image", "age"]).to_pylist()
    rng = np.random.default_rng(0)
    t = [t[i] for i in rng.permutation(len(t))[:limit]]
    crops, ages = [], []
    for row in t:
        img = cv2.imdecode(np.frombuffer(row["image"]["bytes"], np.uint8), cv2.IMREAD_COLOR)
        pad = cv2.copyMakeBorder(img, 100, 100, 100, 100, cv2.BORDER_CONSTANT)
        found = det.detect(pad, 0.5)
        if not found:
            continue
        f = max(found, key=lambda x: x.score)
        crops.append(align(pad, f.kps))
        ages.append(row["age"])
    return crops, np.array(ages, dtype=np.float32)


def faces_from_fgnet():
    crops, ages, people = [], [], []
    for f in sorted(glob.glob(os.path.join(fgnet_dir, "*"))):
        m = re.match(r"(\d+)A(\d+)", os.path.basename(f))
        crops.append(cv2.imread(f))
        ages.append(int(m.group(2)))
        people.append(int(m.group(1)))
    return crops, np.array(ages, dtype=np.float32), np.array(people)


def features(crops):
    arc = np.vstack([rec.embed(crops[i:i + 64]) for i in range(0, len(crops), 64)])
    with ThreadPoolExecutor(4) as ex:
        clip = np.stack(list(ex.map(clip_embed, crops)))
    return arc, clip


print("Loading UTKFace (training)...", flush=True)
utk_crops, utk_ages = faces_from_utk()
print(f"  {len(utk_crops)} faces", flush=True)
fg_crops, fg_ages, fg_people = faces_from_fgnet()
print(f"FG-NET (testing): {len(fg_crops)} faces of {len(set(fg_people))} people, ages {fg_ages.min():.0f}-{fg_ages.max():.0f}")

utk_arc, utk_clip = features(utk_crops)
fg_arc, fg_clip = features(fg_crops)
np.savez_compressed(os.path.join(out_dir, "fgnet_features.npz"), arc=fg_arc, clip=fg_clip, ages=fg_ages, people=fg_people)

alphas = np.logspace(-2, 3, 12)
results = {}
ga_pred = np.array([ga.predict(c, np.array([8, 8, 104, 104]))[0] for c in fg_crops])
results["InsightFace genderage (ready-made)"] = np.abs(ga_pred - fg_ages)
arc_model = RidgeCV(alphas=alphas).fit(utk_arc, utk_ages)
results["Ours: trained on ArcFace numbers"] = np.abs(np.clip(arc_model.predict(fg_arc), 0, 100) - fg_ages)
clip_model = RidgeCV(alphas=alphas).fit(utk_clip, utk_ages)
results["Ours: trained on SigLIP2 numbers"] = np.abs(np.clip(clip_model.predict(fg_clip), 0, 100) - fg_ages)

print("\nAverage age error on FG-NET (lower is better):")
for name, err in results.items():
    print(f"  {name:40s} {err.mean():5.1f} years   (within 5 years: {np.mean(err <= 5):.0%})")
best = min(results, key=lambda k: results[k].mean())
print(f"Best: {best}")
pickle.dump({"arcface": arc_model, "siglip2": clip_model, "clip_model_name": CLIP_MODEL},
            open(os.path.join(out_dir, "age_models.pkl"), "wb"))
