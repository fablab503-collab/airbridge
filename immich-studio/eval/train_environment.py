"""Train and test the Environment labels on Places365 photos.

Half the photos teach the model, the other half test it. We compare:
  * zero-shot (sentences only, no training)
  * trained   (small model learned from the teaching half)
The final trained model (learned from all photos) is saved for the Studio.

Usage: python3 train_environment.py PLACES365_DIR ML_URL OUT_DIR [PER_LABEL]
"""
import os, pickle, random, sys
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor

import numpy as np
from sklearn.linear_model import LogisticRegression

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "pystudio"))
places_dir, ml_url, out_dir = sys.argv[1:4]
per_label = int(sys.argv[4]) if len(sys.argv) > 4 else 60
os.environ["IMMICH_ML_URL"] = ml_url
from app import clip  # noqa: E402
from app.labels import ENVIRONMENT, Labeler  # noqa: E402

cats = {}
for line in open(os.path.join(places_dir, "categories_places365.txt")):
    path, idx = line.split()
    cats[int(idx)] = path[3:].replace("_", " ")  # "/f/forest/broadleaf" -> "forest/broadleaf"
places_to_label = {p: lab for lab, (_, places) in ENVIRONMENT.items() for p in places}
missing = {p for p in places_to_label if p not in cats.values()}
assert not missing, f"unknown Places365 classes: {missing}"

by_label = defaultdict(list)
for line in open(os.path.join(places_dir, "places365_val.txt")):
    name, idx = line.split()
    lab = places_to_label.get(cats[int(idx)])
    if lab:
        by_label[lab].append(os.path.join(places_dir, "val_256", name))
random.seed(0)
picked = [(f, lab) for lab, files in sorted(by_label.items()) for f in random.sample(files, min(per_label, len(files)))]
print(f"{len(picked)} photos across {len(by_label)} environment labels")


def embed(item):
    f, lab = item
    return clip.embed_image(open(f, "rb").read()), lab


with ThreadPoolExecutor(4) as ex:
    rows = list(ex.map(embed, picked))
X = np.stack([r[0] for r in rows])
y = np.array([r[1] for r in rows])

rng = np.random.default_rng(0)
order = rng.permutation(len(y))
half = len(y) // 2
tr, te = order[:half], order[half:]

labeler = Labeler()
zs = np.array([max(labeler.zero_shot("Environment", x).items(), key=lambda kv: kv[1])[0] for x in X[te]])
head = LogisticRegression(max_iter=3000, C=5).fit(X[tr], y[tr])
trained = head.predict(X[te])
print(f"Test photos: {len(te)}")
print(f"  Zero-shot (no training): {np.mean(zs == y[te]):.1%} correct")
print(f"  Trained model:           {np.mean(trained == y[te]):.1%} correct")
top3 = np.argsort(-head.predict_proba(X[te]), axis=1)[:, :3]
print(f"  Trained, right answer in top 3: {np.mean([y[te][k] in head.classes_[top3[k]] for k in range(len(te))]):.1%}")
prison = y[te] == "Prison"
print(f"  Prison photos found: {np.sum(trained[prison] == 'Prison')}/{prison.sum()}")

final = LogisticRegression(max_iter=3000, C=5).fit(X, y)
path = os.path.join(out_dir, "label_heads.pkl")
heads = pickle.load(open(path, "rb")) if os.path.exists(path) else {}
heads["Environment"] = final
pickle.dump(heads, open(path, "wb"))
print(f"Saved trained Environment model -> {path}")
