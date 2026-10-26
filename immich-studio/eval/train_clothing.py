"""Train and test the clothing labels (type, colour, style) on fashion-product-images.

Half teaches, half tests; zero-shot sentences vs trained model, per group.
Note: these are mostly product photos, while Studio looks at people in everyday photos,
so a model that wins here is only shipped if it wins clearly.

Usage: python3 train_clothing.py PARQUET ML_URL OUT_DIR [PER_LABEL]
"""
import io, os, pickle, sys
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor

import numpy as np, pyarrow.parquet as pq
from PIL import Image
from sklearn.linear_model import LogisticRegression

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "pystudio"))
parquet, ml_url, out_dir = sys.argv[1:4]
per_label = int(sys.argv[4]) if len(sys.argv) > 4 else 120
os.environ["IMMICH_ML_URL"] = ml_url
from app import clip, formats  # noqa: E402
from app.labels import Labeler  # noqa: E402

TYPE = {"Tshirts": "T-shirt", "Shirts": "Shirt", "Dresses": "Dress", "Jackets": "Jacket", "Sweatshirts": "Hoodie",
        "Sweaters": "Sweater", "Jeans": "Jeans", "Shorts": "Shorts", "Skirts": "Skirt", "Caps": "Hat",
        "Sunglasses": "Sunglasses", "Scarves": "Scarf", "Kurtas": "Traditional dress", "Sarees": "Traditional dress",
        "Track Pants": "Sportswear", "Tracksuits": "Sportswear", "Blazers": "Suit", "Suits": "Suit", "Swimwear": "Swimwear"}
COLOUR = {"Black": "Black", "White": "White", "Grey": "Grey", "Charcoal": "Grey", "Grey Melange": "Grey",
          "Red": "Red", "Maroon": "Red", "Burgundy": "Red", "Blue": "Blue", "Navy Blue": "Blue",
          "Turquoise Blue": "Blue", "Green": "Green", "Olive": "Green", "Sea Green": "Green", "Lime Green": "Green",
          "Yellow": "Yellow", "Mustard": "Yellow", "Orange": "Orange", "Rust": "Orange", "Pink": "Pink",
          "Magenta": "Pink", "Rose": "Pink", "Purple": "Purple", "Lavender": "Purple", "Mauve": "Purple",
          "Brown": "Brown", "Coffee Brown": "Brown", "Tan": "Brown", "Beige": "Beige", "Cream": "Beige",
          "Khaki": "Beige", "Off White": "Beige"}
STYLE = {"Casual": "Casual", "Formal": "Formal", "Sports": "Sporty", "Ethnic": "Traditional"}
APPAREL = {"Topwear", "Bottomwear", "Dress", "Saree", "Apparel Set", "Innerwear", "Loungewear and Nightwear"}

t = pq.read_table(parquet, columns=["articleType", "baseColour", "usage", "subCategory", "masterCategory"]).to_pylist()
groups = {
    "Clothing": [(i, TYPE[r["articleType"]]) for i, r in enumerate(t) if r["articleType"] in TYPE],
    "Clothing color": [(i, COLOUR[r["baseColour"]]) for i, r in enumerate(t)
                       if r["baseColour"] in COLOUR and r["masterCategory"] == "Apparel"],
    "Clothing style": [(i, STYLE[r["usage"]]) for i, r in enumerate(t)
                       if r["usage"] in STYLE and r["masterCategory"] == "Apparel"],
}
rng = np.random.default_rng(0)
picked = {}
for g, items in groups.items():
    by = defaultdict(list)
    for i, lab in items:
        by[lab].append(i)
    picked[g] = [(i, lab) for lab, ids in sorted(by.items()) for i in rng.permutation(ids)[:per_label]]
    print(f"{g}: {len(picked[g])} photos, {len(by)} labels", flush=True)

need = sorted({i for g in picked.values() for i, _ in g})
images = pq.read_table(parquet, columns=["image"]).column("image")


def embed(i):
    img = Image.open(io.BytesIO(images[int(i)].as_py()["bytes"])).convert("RGB")
    return i, clip.embed_image(formats.to_jpeg(img, 512))


with ThreadPoolExecutor(4) as ex:
    emb = dict(ex.map(embed, need))

lab = Labeler()
heads = {}
for g, items in picked.items():
    X = np.stack([emb[i] for i, _ in items])
    y = np.array([l for _, l in items])
    order = rng.permutation(len(y))
    tr, te = order[: len(y) // 2], order[len(y) // 2:]
    zs = np.array([max({k: v for k, v in lab.zero_shot(g, x).items() if k in set(y)}.items(),
                       key=lambda kv: kv[1])[0] for x in X[te]])
    head = LogisticRegression(max_iter=3000, C=2).fit(X[tr], y[tr])
    print(f"{g} ({len(set(y))} labels, {len(te)} test photos): zero-shot {np.mean(zs == y[te]):.1%}  "
          f"trained {np.mean(head.predict(X[te]) == y[te]):.1%}", flush=True)
    heads[g] = LogisticRegression(max_iter=3000, C=2).fit(X, y)
pickle.dump(heads, open(os.path.join(out_dir, "clothing_heads.pkl"), "wb"))
