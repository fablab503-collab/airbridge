"""How well can we recognise the SAME person at DIFFERENT ages? (FG-NET: 82 people, 1002 photos, ages 0-69)

For every photo we ask: "of all the other photos, which one looks most like this person?"
If that photo is the same person -> correct (rank-1 identification).
We also report it only for pairs far apart in age (e.g. child photo vs adult photo).

Models compared:
  buffalo_l  : ArcFace R50  (Immich default)
  antelopev2 : ArcFace R100 (bigger, stronger)
  fusion     : both together
  mesh-478   : only the shape of 478 face points (MediaPipe) - shows why points alone are not enough

Usage: python3 face_age_gap_eval.py FGNET_DIR MODEL_DIR OUT_DIR
"""
import glob, os, re, sys

import cv2, numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "studio"))
fg_dir, model_dir, out_dir = sys.argv[1:4]
os.environ["STUDIO_MODEL_DIR"] = model_dir
from app.faces import FaceRecognizer  # noqa: E402

files = sorted(glob.glob(os.path.join(fg_dir, "*")))
people = np.array([int(re.match(r"(\d+)A", os.path.basename(f)).group(1)) for f in files])
ages = np.array([int(re.match(r"\d+A(\d+)", os.path.basename(f)).group(1)) for f in files])
crops = [cv2.imread(f) for f in files]
print(f"{len(files)} photos, {len(set(people))} people, ages {ages.min()}-{ages.max()}")


def embed(model_path):
    rec = FaceRecognizer(model_path)
    return np.vstack([rec.embed(crops[i:i + 32]) for i in range(0, len(crops), 32)])


feats = {"buffalo_l (Immich default, R50)": embed(os.path.join(model_dir, "buffalo_l", "w600k_r50.onnx")),
         "antelopev2 (R100)": embed(os.path.join(model_dir, "antelopev2", "glintr100.onnx"))}
f = np.hstack(list(feats.values()))
feats["fusion (R50 + R100)"] = f / np.linalg.norm(f, axis=1, keepdims=True)
np.savez_compressed(os.path.join(out_dir, "fgnet_identity.npz"), **{k.split()[0]: v for k, v in feats.items()},
                    people=people, ages=ages)

try:
    from app.faces import FaceMesh
    mesh = FaceMesh(os.path.join(model_dir, "face_landmarker.task"))
    pts, ok = [], []
    for c in crops:
        big = cv2.resize(c, (448, 448), interpolation=cv2.INTER_CUBIC)
        m = mesh.points(big)
        ok.append(bool(m))
        if m:
            p = m[0][:, :2] - m[0][:, :2].mean(0)
            pts.append((p / np.linalg.norm(p)).ravel())
        else:
            pts.append(np.zeros(478 * 2))
    ok = np.array(ok)
    P = np.array(pts)
    feats["mesh-478 points only"] = P
    print(f"Face mesh found on {ok.sum()}/{len(ok)} photos")
except Exception as e:  # mediapipe not installed here
    print(f"(mesh test skipped: {e})")
    ok = np.ones(len(files), bool)


def rank1(F, mask_valid, min_gap=0):
    S = F @ F.T  # all features are length-1 vectors, so this ranks by distance too
    correct = total = 0
    for i in np.where(mask_valid)[0]:
        cand = np.where(mask_valid & (np.arange(len(F)) != i))[0]
        same = people[cand] == people[i]
        far = np.abs(ages[cand] - ages[i]) >= min_gap
        # only count probes that have at least one same-person photo with that age gap
        if not (same & far).any():
            continue
        keep = ~same | far  # remove same-person photos that are too close in age
        cand = cand[keep]
        best = cand[np.argmax(S[i, cand])]
        correct += people[best] == people[i]
        total += 1
    return correct / total, total


print("\nRank-1: is the most similar OTHER photo the same person? (higher is better)")
print(f"{'model':36s} {'any age':>9s} {'10+ yrs apart':>14s} {'20+ yrs apart':>14s}")
results = {}
for name, F in feats.items():
    valid = ok if "mesh" in name else np.ones(len(files), bool)
    r = [rank1(F, valid, g) for g in (0, 10, 20)]
    results[name] = r
    print(f"{name:36s} {r[0][0]:9.1%} {r[1][0]:14.1%} {r[2][0]:14.1%}")
print(f"(tested photos: {results[next(iter(results))][0][1]}, {results[next(iter(results))][1][1]}, "
      f"{results[next(iter(results))][2][1]})")
