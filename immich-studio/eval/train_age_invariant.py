"""Can we teach face fingerprints to ignore ageing? (person-disjoint cross-validation on FG-NET)

Idea: Linear Discriminant Analysis (LDA) learns which directions in the 512 numbers change
with *who* someone is, and which change with *age/light/pose*, then keeps the "who" ones.
We train on some people and test on OTHER people only (5 folds), so nobody is tested on themselves.

Usage: python3 train_age_invariant.py OUT_DIR   (uses fgnet_identity.npz from face_age_gap_eval.py)
"""
import os, pickle, sys

import numpy as np
from sklearn.discriminant_analysis import LinearDiscriminantAnalysis
from sklearn.decomposition import PCA
from sklearn.pipeline import make_pipeline

out_dir = sys.argv[1]
d = np.load(os.path.join(out_dir, "fgnet_identity.npz"))
people, ages = d["people"], d["ages"]


def norm(x):
    return x / np.linalg.norm(x, axis=1, keepdims=True)


def rank1(F, idx, gap):
    S = F @ F.T
    ok = tot = 0
    for a, i in enumerate(idx):
        others = np.array([b for b in range(len(idx)) if b != a])
        same = people[idx[others]] == people[i]
        far = np.abs(ages[idx[others]] - ages[i]) >= gap
        if not (same & far).any():
            continue
        cand = others[~same | far]
        best = cand[np.argmax(S[a, cand])]
        ok += people[idx[best]] == people[i]
        tot += 1
    return ok, tot


ids = np.unique(people)
rng = np.random.default_rng(0)
folds = np.array_split(rng.permutation(ids), 5)
for name in ("antelopev2", "fusion"):
    X = d[name]
    base = {g: [0, 0] for g in (0, 10, 20)}
    lda = {g: [0, 0] for g in (0, 10, 20)}
    for test_people in folds:
        te = np.where(np.isin(people, test_people))[0]
        tr = np.where(~np.isin(people, test_people))[0]
        model = make_pipeline(PCA(n_components=min(200, len(tr) - 1), whiten=False),
                              LinearDiscriminantAnalysis(solver="eigen", shrinkage=0.3)).fit(X[tr], people[tr])
        Z = norm(model.transform(X[te]))
        for g in (0, 10, 20):
            for store, F in ((base, norm(X[te])), (lda, Z)):
                ok, tot = rank1(F, te, g)
                store[g][0] += ok
                store[g][1] += tot
    print(f"{name}:")
    for label, store in (("as is", base), ("+ trained age-ignoring layer", lda)):
        print(f"  {label:30s} " + "  ".join(f"{g:>2}+ yrs: {store[g][0] / store[g][1]:6.1%}" for g in (0, 10, 20)))

final = make_pipeline(PCA(n_components=200), LinearDiscriminantAnalysis(solver="eigen", shrinkage=0.3)).fit(
    d["antelopev2"], people)
pickle.dump(final, open(os.path.join(out_dir, "age_invariant_antelopev2.pkl"), "wb"))
