"""Train and test the Era (decade) labels on fashion-decade-images (1910s-2000s, ~250 photos each).

Half teaches, half tests. Compares zero-shot sentences with the trained model and
saves the trained model (learned from all photos) for the Studio.
Note: tonyassi/fashion-decade-images-1 states no licence - keep the result for personal use.

Usage: python3 train_era.py PARQUET_DIR ML_URL OUT_DIR
"""
import glob, os, pickle, sys
from concurrent.futures import ThreadPoolExecutor

import numpy as np, pyarrow.parquet as pq
from sklearn.linear_model import LogisticRegression

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "pystudio"))
pq_dir, ml_url, out_dir = sys.argv[1:4]
os.environ["IMMICH_ML_URL"] = ml_url
from app import clip, formats  # noqa: E402
from app.labels import Labeler  # noqa: E402
from PIL import Image  # noqa: E402
import io  # noqa: E402

NAMES = ["1910s", "1920s", "1930s", "1940s", "1950s", "1960s", "1970s", "1980s", "1990s", "2000s"]
rows = []
for f in sorted(glob.glob(os.path.join(pq_dir, "*.parquet"))):
    t = pq.read_table(f)
    rows += list(zip(t.column("image").to_pylist(), t.column("label").to_pylist()))
print(f"{len(rows)} photos", flush=True)


def embed(row):
    img = Image.open(io.BytesIO(row[0]["bytes"])).convert("RGB")
    return clip.embed_image(formats.to_jpeg(img, 768))


cache = os.path.join(out_dir, "era_embeddings.npy")  # computing these is the slow part: keep them
if os.path.exists(cache):
    X = np.load(cache)
else:
    with ThreadPoolExecutor(4) as ex:
        X = np.stack(list(ex.map(embed, rows)))
    np.save(cache, X)
y = np.array([NAMES[r[1]] for r in rows])
decade = {n: i for i, n in enumerate(NAMES)}

rng = np.random.default_rng(0)
order = rng.permutation(len(y))
tr, te = order[: len(y) // 2], order[len(y) // 2:]

lab = Labeler()
zs = []
for x in X[te]:
    s = {k: v for k, v in lab.zero_shot("Era", x).items() if k in decade}
    zs.append(max(s, key=s.get))
zs = np.array(zs)
head = LogisticRegression(max_iter=3000, C=2).fit(X[tr], y[tr])
pred = head.predict(X[te])


def report(name, p):
    exact = np.mean(p == y[te])
    near = np.mean([abs(decade[a] - decade[b]) <= 1 for a, b in zip(p, y[te])])
    print(f"  {name:24s} exact decade {exact:6.1%}   within one decade {near:6.1%}")


print(f"Test photos: {len(te)}  (guessing at random = 10%)")
report("Zero-shot (no training)", zs)
report("Trained model", pred)

final = LogisticRegression(max_iter=3000, C=2).fit(X, y)
path = os.path.join(out_dir, "label_heads.pkl")
heads = pickle.load(open(path, "rb")) if os.path.exists(path) else {}
heads["Era"] = final
pickle.dump(heads, open(path, "wb"))
print(f"Saved trained Era model -> {path}")
