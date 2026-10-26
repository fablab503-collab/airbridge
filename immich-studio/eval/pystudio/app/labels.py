"""Label photos: environment, people, era (year), age, clothing.

Each label group works in one of two ways:
  * zero-shot: compare the photo with describing sentences (works with no training)
  * trained:   a small model learned from examples (Places365, or your own
               "Train: <Group>/<Label>" albums in Immich) - used when available
"""
from __future__ import annotations

import os
import pickle

import numpy as np

from . import clip

MODEL_DIR = os.environ.get("STUDIO_MODEL_DIR", "/models")

# label -> describing sentences. Environment labels also list the Places365
# classes they cover, which is how we train and test them.
ENVIRONMENT = {
    "Beach": (["a photo of a sandy beach", "a beach by the sea"], ["beach", "coast"]),
    "Sea": (["a photo of the open ocean", "ocean waves"], ["ocean", "wave"]),
    "Mountains": (["a photo of mountains", "a mountain landscape", "snowy mountain peaks"],
                  ["mountain", "mountain snowy", "mountain path", "valley"]),
    "Forest": (["a photo of a forest", "trees in a forest"],
               ["forest/broadleaf", "forest path", "forest road", "rainforest", "bamboo forest"]),
    "Lake or river": (["a photo of a lake", "a photo of a river"], ["lake/natural", "river", "pond", "creek"]),
    "Waterfall": (["a photo of a waterfall"], ["waterfall"]),
    "Desert": (["a photo of a desert"], ["desert/sand", "desert/vegetation", "badlands"]),
    "Snow": (["a snowy landscape", "a ski slope"], ["snowfield", "ski slope", "ski resort", "glacier"]),
    "Countryside": (["a photo of farmland and fields", "countryside fields"],
                    ["field/cultivated", "field/wild", "pasture", "hayfield", "wheat field", "farm"]),
    "Park or garden": (["a photo of a park", "a garden"], ["park", "botanical garden", "formal garden", "lawn"]),
    "City street": (["a photo of a city street", "downtown buildings"],
                    ["street", "downtown", "alley", "crosswalk", "plaza", "skyscraper"]),
    "Road": (["a photo of a highway", "an open road"], ["highway", "desert road", "field road"]),
    "Bridge": (["a photo of a bridge"], ["bridge"]),
    "Living room": (["a photo of a living room"], ["living room", "television room"]),
    "Bedroom": (["a photo of a bedroom"], ["bedroom", "childs room", "dorm room", "hotel room"]),
    "Kitchen": (["a photo of a kitchen"], ["kitchen", "restaurant kitchen"]),
    "Bathroom": (["a photo of a bathroom"], ["bathroom", "shower"]),
    "Dining room": (["a photo of a dining room"], ["dining room"]),
    "Office": (["a photo of an office"], ["office", "office cubicles", "home office", "conference room"]),
    "Classroom": (["a photo of a classroom"], ["classroom", "lecture room", "kindergarden classroom"]),
    "Restaurant or cafe": (["a photo of a restaurant", "a cafe"],
                           ["restaurant", "coffee shop", "fastfood restaurant", "pizzeria", "cafeteria"]),
    "Bar or pub": (["a photo of a bar", "a pub"], ["bar", "pub/indoor", "beer hall"]),
    "Shop or mall": (["a photo of a shop", "a shopping mall", "a supermarket"],
                     ["shopping mall/indoor", "supermarket", "clothing store", "department store"]),
    "Place of worship": (["a photo of a church", "a temple", "a mosque"],
                         ["church/indoor", "church/outdoor", "mosque/outdoor", "temple/asia", "synagogue/outdoor"]),
    "Castle or palace": (["a photo of a castle", "a palace"], ["castle", "palace"]),
    "Museum or gallery": (["a photo of a museum", "an art gallery"], ["museum/indoor", "art gallery"]),
    "Hospital": (["a photo of a hospital room"], ["hospital room", "operating room", "hospital"]),
    "Prison": (["a photo of a prison cell", "a jail cell with bars"], ["jail cell"]),
    "Airport or plane": (["a photo of an airport", "inside an airplane"],
                         ["airport terminal", "airplane cabin", "runway"]),
    "Train or station": (["a photo of a train station", "inside a train"],
                         ["train station/platform", "train interior", "subway station/platform"]),
    "Inside a car": (["a photo taken inside a car"], ["car interior"]),
    "Stadium or sports field": (["a photo of a stadium", "a sports field"],
                                ["stadium/soccer", "stadium/football", "stadium/baseball", "soccer field",
                                 "football field", "baseball field"]),
    "Gym": (["a photo of a gym"], ["gymnasium/indoor", "martial arts gym"]),
    "Swimming pool": (["a photo of a swimming pool"], ["swimming pool/outdoor", "swimming pool/indoor"]),
    "Concert or stage": (["a photo of a stage", "a concert"], ["stage/indoor", "stage/outdoor", "arena/performance"]),
    "Playground": (["a photo of a playground"], ["playground"]),
    "Amusement park": (["a photo of an amusement park"], ["amusement park", "carrousel"]),
    "Harbor or boat": (["a photo of a harbor", "on a boat"], ["harbor", "boat deck", "pier"]),
    "Campsite": (["a photo of a campsite with tents"], ["campsite"]),
    "Cemetery": (["a photo of a cemetery"], ["cemetery"]),
    "Construction site": (["a photo of a construction site"], ["construction site"]),
    "Library": (["a photo of a library", "a bookstore"], ["library/indoor", "bookstore"]),
}

GROUPS: dict[str, dict[str, list[str]]] = {
    "Environment": {k: v[0] for k, v in ENVIRONMENT.items()},
    "Setting": {"Indoors": ["a photo taken indoors"], "Outdoors": ["a photo taken outdoors"]},
    "Light": {"Daytime": ["a photo taken in daylight"], "Night": ["a photo taken at night"],
              "Sunset": ["a photo of a sunset or sunrise"]},
    "People": {"No people": ["a photo with no people in it", "an empty scene"],
               "One person": ["a photo of one person", "a portrait of a single person"],
               "Two people": ["a photo of two people", "a couple"],
               "Group": ["a photo of a small group of people", "a family photo"],
               "Crowd": ["a photo of a big crowd of people"]},
    "Era": {d: [f"a photo from the {d}", f"a {d} photograph"] for d in
            ["1900s", "1910s", "1920s", "1930s", "1940s", "1950s", "1960s", "1970s", "1980s", "1990s", "2000s",
             "2010s", "2020s"]},
    "Clothing": {c: [f"a person wearing {a}"] for c, a in {
        "T-shirt": "a t-shirt", "Shirt": "a button-up shirt", "Suit": "a suit and tie", "Dress": "a dress",
        "Jacket": "a jacket", "Coat": "a winter coat", "Hoodie": "a hoodie", "Sweater": "a sweater",
        "Jeans": "jeans", "Shorts": "shorts", "Skirt": "a skirt", "Swimwear": "swimwear",
        "Uniform": "a uniform", "Sportswear": "sportswear", "Traditional dress": "traditional clothing",
        "Wedding dress": "a wedding dress", "Hat": "a hat", "Glasses": "glasses", "Sunglasses": "sunglasses",
        "Scarf": "a scarf"}.items()},
    "Clothing color": {c: [f"a person wearing {c.lower()} clothes"] for c in
                       ["Black", "White", "Grey", "Red", "Blue", "Green", "Yellow", "Orange", "Pink", "Purple",
                        "Brown", "Beige"]},
    "Clothing style": {"Formal": ["a person in formal clothes"], "Casual": ["a person in casual clothes"],
                       "Sporty": ["a person in sports clothes"], "Work uniform": ["a person in a work uniform"],
                       "Traditional": ["a person in traditional clothes"]},
}

# Groups where a photo can have several answers (e.g. jeans AND a jacket).
MULTI = {"Clothing": 3, "Clothing color": 2}
# Groups that only make sense when a person is in the photo.
NEEDS_PERSON = {"Clothing", "Clothing color", "Clothing style"}


class Labeler:
    def __init__(self, model: str = clip.CLIP_MODEL):
        self.model = model
        self.prompts = {}
        for group, labels in GROUPS.items():
            names = list(labels)
            vecs = np.stack([np.mean([clip.embed_text(p, model) for p in labels[n]], axis=0) for n in names])
            self.prompts[group] = (names, vecs / np.linalg.norm(vecs, axis=1, keepdims=True))
        self.trained = {}
        path = os.path.join(MODEL_DIR, "label_heads.pkl")
        if os.path.exists(path):
            self.trained = pickle.load(open(path, "rb"))

    def zero_shot(self, group: str, emb: np.ndarray) -> dict[str, float]:
        names, vecs = self.prompts[group]
        logits = 100.0 * vecs @ emb
        p = np.exp(logits - logits.max())
        p /= p.sum()
        return dict(zip(names, p.tolist()))

    def scores(self, group: str, emb: np.ndarray) -> dict[str, float]:
        zs = self.zero_shot(group, emb) if group in self.prompts else {}
        head = self.trained.get(group)
        if head is None:
            return zs
        trained = dict(zip(head.classes_, head.predict_proba(emb[None])[0].tolist()))
        # Labels the trained model never saw (e.g. Era "2020s") keep their zero-shot share;
        # the trained model splits the rest.
        missing = {k: v for k, v in zs.items() if k not in trained}
        rest = 1 - sum(missing.values())
        return {**{k: v * rest for k, v in trained.items()}, **missing}

    def label(self, emb: np.ndarray) -> dict[str, list[tuple[str, float]]]:
        out = {}
        for group in GROUPS:
            ranked = sorted(self.scores(group, emb).items(), key=lambda kv: -kv[1])
            if group in MULTI:
                top = ranked[0][1]
                picked = [(n, s) for n, s in ranked[:MULTI[group]] if s >= top * 0.5]
            else:
                # With many choices the winner gets a smaller share, so the bar is set relative
                # to guessing: it must be at least 4x more likely than a random pick (max 35%).
                bar = min(0.35, 4 / len(ranked))
                picked = [ranked[0]] if ranked[0][1] >= bar else []
            out[group] = picked
        has_person = out["People"] and out["People"][0][0] != "No people"
        if not has_person:
            for g in NEEDS_PERSON:
                out[g] = []
        return out
