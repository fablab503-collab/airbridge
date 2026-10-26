"""Turn the models trained by the scripts in this folder (scikit-learn pickles) into plain files the
Studio (Rust) reads. Run from immich-studio/:  python3 eval/export_trained.py eval/python-models studio/trained

  label_heads.pkl              -> label_heads.json      (logistic regression per label group)
  age_head_siglip2.pkl         -> age_head.json         (ridge regression: SigLIP2 numbers -> age)
  age_invariant_antelopev2.pkl -> age_invariant.json    (PCA + LDA, saved as one matrix + offset)
  fgnet_antelopev2.npz         -> fgnet_antelopev2.json + .f32  (training faces for "learn")
"""
import json
import os
import pickle
import sys

import numpy as np

src, d = sys.argv[1], sys.argv[2]
rng = np.random.RandomState(0)

heads = pickle.load(open(os.path.join(src, "label_heads.pkl"), "rb"))
out = {}
for group, m in heads.items():
    out[group] = {"classes": [str(c) for c in m.classes_], "coef": m.coef_.astype(float).round(7).tolist(),
                  "intercept": m.intercept_.astype(float).round(7).tolist()}
    # check: softmax (or sigmoid for 2 labels) of the saved numbers gives sklearn's answer
    x = rng.randn(5, m.coef_.shape[1]).astype(np.float32) * 0.05
    z = x @ np.array(out[group]["coef"]).T + out[group]["intercept"]
    if z.shape[1] == 1:
        p = np.hstack([1 - 1 / (1 + np.exp(-z)), 1 / (1 + np.exp(-z))])
    else:
        p = np.exp(z - z.max(1, keepdims=True))
        p /= p.sum(1, keepdims=True)
    assert np.abs(p - m.predict_proba(x)).max() < 1e-4, group
json.dump(out, open(os.path.join(d, "label_heads.json"), "w"))

age = pickle.load(open(os.path.join(src, "age_head_siglip2.pkl"), "rb"))
json.dump({"clip_model": age["clip_model"], "coef": age["model"].coef_.astype(float).tolist(),
           "intercept": float(age["model"].intercept_)}, open(os.path.join(d, "age_head.json"), "w"))

inv = pickle.load(open(os.path.join(src, "age_invariant_antelopev2.pkl"), "rb"))
dim = 512
b = inv.transform(np.zeros((1, dim)))[0]
A = inv.transform(np.eye(dim)) - b  # the whole PCA + LDA is one straight-line map: x @ A + b
x = rng.randn(3, dim)
assert np.abs(x @ A + b - inv.transform(x)).max() < 1e-6
json.dump({"input": dim, "output": int(A.shape[1]), "matrix": A.round(9).flatten().tolist(), "offset": b.tolist()},
          open(os.path.join(d, "age_invariant.json"), "w"))

z = np.load(os.path.join(src, "fgnet_antelopev2.npz"))
z["embs"].astype("<f4").tofile(os.path.join(d, "fgnet_antelopev2.f32"))
json.dump({"dim": int(z["embs"].shape[1]), "people": z["people"].astype(str).tolist()},
          open(os.path.join(d, "fgnet_antelopev2.json"), "w"))
print("exported", sorted(heads), "age head", "age-invariant", A.shape, "fgnet", z["embs"].shape)
