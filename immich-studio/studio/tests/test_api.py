"""End-to-end check of every Studio feature against a running Immich + Studio.

Usage: python3 tests/test_api.py http://localhost:2283 TOKEN_FILE SAMPLE_RAW SAMPLE_VIDEO OUT_DIR
"""
import json
import os
import sys
import time

import requests

base, token_file, raw_file, video_file, out = sys.argv[1:6]
token = open(token_file).read().strip()
S = requests.Session()
S.cookies.set("immich_access_token", token)
studio = base + "/studio/api"
immich = base + "/api"
H = {"Authorization": f"Bearer {token}"}
results = []


def check(name, cond, detail=""):
    results.append((name, bool(cond)))
    print(f"{'PASS' if cond else 'FAIL'}  {name}  {detail}")


def wait(job_id, limit=1800):
    t = time.time()
    while time.time() - t < limit:
        j = S.get(f"{studio}/jobs/{job_id}").json()
        if j["status"] in ("done", "error"):
            return j
        time.sleep(2)
    raise TimeoutError(job_id)


def find(name):
    r = requests.post(f"{immich}/search/metadata", headers=H, json={"originalFileName": name}).json()
    return r["assets"]["items"][0]["id"]


puppy = find("photo-237.jpg")

# who am I (Immich login cookie is enough)
me = S.get(f"{studio}/me").json()
check("login via Immich cookie", "email" in me, me.get("email", ""))
check("no cookie -> refused", requests.get(f"{studio}/me").status_code == 401)

# print check + PDF with automatic AI upscale
r = S.get(f"{studio}/print/{puppy}").json()
a4 = next(p for p in r["papers"] if p["paper"] == "A4")
check("print report", a4["dpi"] > 0, f"{r['pixels']} -> A4 {a4['dpi']} dpi, {a4['verdict']}")
j = wait(S.post(f"{studio}/print/{puppy}", json={"paper": "A4", "upscale": "auto", "crop_marks": True}).json()["job"])
pdf = S.get(base + "/studio/" + j["file_url"]).content
open(os.path.join(out, "print-A4.pdf"), "wb").write(pdf)
check("A4 print PDF with AI upscale", j["status"] == "done" and pdf[:4] == b"%PDF",
      f"upscaled x{j['result']['upscaled']} -> {j['result']['pixels']} = {j['result']['dpi']} dpi")

# photo book from an album
album = next(a for a in S.get(f"{studio}/albums").json() if a["name"] == "AI: Dog")
j = wait(S.post(f"{studio}/book", json={"album_id": album["id"], "paper": "A4", "per_page": 4,
                                         "title": "Dogs", "subtitle": "Immich Studio test"}).json()["job"])
pdf = S.get(base + "/studio/" + j["file_url"]).content
open(os.path.join(out, "book.pdf"), "wb").write(pdf)
check("photo book PDF", pdf[:4] == b"%PDF", f"{j['result']['pages']} pages")

# edit: preview, save as copy, then delete the copy (to trash)
steps = [{"op": "saturation", "amount": 1.4}, {"op": "recolor", "from_hue": 30, "to_hue": 200}]
prev = S.post(f"{studio}/edit/{puppy}", json={"steps": steps, "preview": True})
check("edit preview", prev.headers["content-type"] == "image/jpeg" and len(prev.content) > 1000)
new = S.post(f"{studio}/edit/{puppy}", json={"steps": steps, "preview": False, "format": "png"}).json()
info = requests.get(f"{immich}/assets/{new['new_asset']}", headers=H).json()
check("edit saved as new photo", info["originalFileName"].endswith("-edited.png"), info["originalFileName"])
S.delete(f"{studio}/asset/{new['new_asset']}")
info = requests.get(f"{immich}/assets/{new['new_asset']}", headers=H).json()
check("delete moves to trash", info.get("isTrashed") is True)

# import a RAW file
with open(raw_file, "rb") as f:
    r = S.post(f"{studio}/import", files={"file": (os.path.basename(raw_file), f)}).json()
check("import RAW", "asset" in r, json.dumps(r))
exr = os.path.join(os.path.dirname(raw_file), "..", "formats", "sample.exr")
if os.path.exists(exr):
    with open(exr, "rb") as f:
        r = S.post(f"{studio}/import", files={"file": ("sample.exr", f)}).json()
    check("import EXR (converted)", r.get("converted") is True, json.dumps(r))

# graphite return path (simulates Graphite's patched export)
r = S.post(f"{studio}/graphite-return/{puppy}", files={"file": ("puppy.png", prev.content, "image/png")}).json()
check("Graphite export saved to Immich", "new_asset" in r)

# video: upload, transcribe, check description + subtitles
with open(video_file, "rb") as f:
    vid = S.post(f"{studio}/import", files={"file": (os.path.basename(video_file), f)}).json()["asset"]
for _ in range(60):
    if requests.get(f"{immich}/assets/{vid}", headers=H).json().get("type") == "VIDEO":
        break
    time.sleep(2)
j = wait(S.post(f"{studio}/video/{vid}/transcribe").json()["job"])
text = (j.get("result") or {}).get("text", "")
check("video speech to text", "ask not what your country" in text.lower(), text[:80])
desc = requests.get(f"{immich}/assets/{vid}", headers=H).json()["exifInfo"].get("description", "")
check("transcript searchable in Immich", "[Transcript]" in desc)
srt = S.get(f"{studio}/video/{vid}/subtitles.srt").text
check("subtitles file", "-->" in srt)

# faces + mesh
f = S.get(f"{studio}/faces/{find('photo-64.jpg')}").json()
check("face + 478-point mesh", f["faces"] and len(f["faces"][0]["mesh"]) == 478, f"age ~{f['faces'][0]['age']}")

# tags written by analyze are searchable in Immich
tags = requests.get(f"{immich}/tags", headers=H).json()
check("Studio tags exist in Immich", any(t["value"].startswith("Clothing/") for t in tags), f"{len(tags)} tags")

print(f"\n{sum(ok for _, ok in results)}/{len(results)} passed")
sys.exit(0 if all(ok for _, ok in results) else 1)
