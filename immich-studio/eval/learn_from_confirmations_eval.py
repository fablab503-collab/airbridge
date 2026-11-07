"""Does confirming a few photos per person help find the rest? (FG-NET, people never seen in training)

For each of 5 folds: the layer is trained on the other people (like the shipped model), then
"you" confirm K random photos of each test person and the layer is retrained with them.
We measure, on the test people's OTHER photos only (never the confirmed ones):
  * rank-1 at 20+ years apart
  * how many same-person photos reach the "certain" level, and whether any wrong person does

Usage: python3 learn_from_confirmations_eval.py OUT_DIR [K] [CERTAIN]
"""
import os, sys

import numpy as np
from sklearn.decomposition import PCA
from sklearn.discriminant_analysis import LinearDiscriminantAnalysis
from sklearn.pipeline import make_pipeline

out_dir = sys.argv[1]
K = int(sys.argv[2]) if len(sys.argv) > 2 else 3
CERTAIN = float(sys.argv[3]) if len(sys.argv) > 3 else 0.72
d = np.load(os.path.join(out_dir, "fgnet_identity.npz"))
X, P, A = d["antelopev2"], d["people"], d["ages"]


def layer(Xtr, ytr):
    return make_pipeline(PCA(n_components=200), LinearDiscriminantAnalysis(solver="eigen", shrinkage=0.3)).fit(Xtr, ytr)


def norm(z):
    return z / np.linalg.norm(z, axis=1, keepdims=True)


def measure(Z, idx):
    """rank-1 at 20+ years and 'certain'-level counts among idx (rows of Z)."""
    S = Z @ Z.T
    ok = tot = 0
    for a in range(len(idx)):
        i = idx[a]
        others = np.array([b for b in range(len(idx)) if b != a])
        same = P[idx[others]] == P[i]
        far = np.abs(A[idx[others]] - A[i]) >= 20
        if (same & far).any():
            cand = others[~same | far]
            ok += P[idx[cand[np.argmax(S[a, cand])]]] == P[i]
            tot += 1
    iu = np.triu_indices(len(idx), 1)
    s = S[iu]
    eq = P[idx[iu[0]]] == P[idx[iu[1]]]
    return ok, tot, np.sum(s[eq] >= CERTAIN), eq.sum(), np.sum(s[~eq] >= CERTAIN), (~eq).sum()


rng = np.random.default_rng(0)
ids = np.unique(P)
folds = np.array_split(rng.permutation(ids), 5)
tot = {"before": np.zeros(6), "after": np.zeros(6)}
for test_people in folds:
    te = np.where(np.isin(P, test_people))[0]
    tr = np.where(~np.isin(P, test_people))[0]
    confirmed = np.concatenate([rng.permutation(te[P[te] == p])[:K] for p in test_people])
    rest = np.setdiff1d(te, confirmed)
    before = layer(X[tr], P[tr])
    after = layer(np.vstack([X[tr], X[confirmed]]), np.concatenate([P[tr], P[confirmed]]))
    tot["before"] += measure(norm(before.transform(X[rest])), rest)
    tot["after"] += measure(norm(after.transform(X[rest])), rest)

for name, (ok, n, cs, ns, cw, nw) in tot.items():
    print(f"{name:7s} rank-1 20+ yrs {ok / n:6.1%} | same-person pairs reaching 'certain' {cs / ns:6.1%} | "
          f"wrong-person pairs reaching 'certain': {int(cw)} of {int(nw)}")
print(f"(you confirmed {K} photos per person; tested only on their other photos)")
