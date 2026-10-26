# Immich Studio

Extra AI tools that plug into [Immich](https://immich.app) (the self-hosted photo app).
Everything runs on your own computer, and every model is open source.

After installing, Immich gets a **✨ Studio** button. On any photo it offers:

| Tool | What it does | Open-source parts |
|---|---|---|
| **AI labels** | Tags each photo with environment (42 kinds of place, including prison), indoors/outdoors, light, number of people, era/decade, age group of each face, and clothing (type, colour, style). The tags become normal Immich tags, so Immich search finds them. | SigLIP2 (through Immich's own ML server), InsightFace, our trained models |
| **Teach your own labels** | Make albums named `Train: Group/Label` (e.g. `Train: Clothing/School uniform`) with ~10 photos each, press *Train*. | scikit-learn |
| **Faces** | Finds faces, guesses age, maps **584 points** per face (478-point 3D mesh + 106-point outline), finds the same person at any age with certain / likely / unsure levels, and learns your people from ✓ / ✗ answers. | InsightFace SCRFD + ArcFace R100 + 2d106det, MediaPipe Face Landmarker, our trained layers |
| **Edit** | Brightness, contrast, colour, exposure, warm/cool, hue, sharpness, rotate, mirror, crop, black & white, sepia, auto-fix and **change one colour** (e.g. red shirt → blue). Save as a new photo or replace the original; delete photos. | Pillow, NumPy |
| **Graphite** | Opens the photo in [Graphite](https://graphite.art), a full editor written in Rust (layers, drawing, text, filters). *File → Export* saves the result straight back into Immich. | Graphite (Apache-2.0) + a small patch |
| **Any file format** | Imports RAW (CR2, CR3, NEF, ARW, DNG, RAF, ORF, RW2 …), HEIC, AVIF, JPEG XL, PSD, SVG, PDF, EXR, HDR, TGA, ICO, DDS, QOI, XPM … Formats Immich can't show are converted to full-quality TIFF. | LibRaw (rawpy), libheif, psd-tools, CairoSVG, PDFium, ImageMagick, ffmpeg |
| **Print** | Shows the print quality (DPI) for A6 … A0, upscales with AI when needed, makes a print-ready PDF at the exact paper size (optional crop marks). RAW files are developed at full resolution for printing. | Real-ESRGAN (BSD-3), ReportLab |
| **Photo book** | Turns an album into a PDF book (cover + 1, 2 or 4 photos per page with date/place captions). | ReportLab |
| **Camera** (phones, tablets, computers) | A camera app at `/studio/camera/` (also in the ✨ menu). Photos and videos go **straight into Immich** (album *Studio Camera*) and are **never saved on the phone**. If the connection drops they wait in memory and upload when it's back. Front/back camera, zoom and light where the phone supports them. "Add to Home Screen" makes it an app icon. | Web standard camera + MediaRecorder |
| **Your logo** | Comes set up as **Gimmich**: vector logo `studio/brand-default/logo.svg`, square app icon `app-icon.svg`, logo with the name `logo-with-name.svg`, name in Roboto (Android's font, SIL Open Font License, served by Studio itself). Studio → *Import any file* → *Your logo*: replaces the Immich logo, browser-tab icon and home-screen icon on your own Immich (admin only), optionally with your own name. *Back to Immich logo* undoes it. | Pillow |
| **Video to text** | Writes down what people say in a video (Whisper), puts it in the video's description (so Immich search finds it) and makes subtitles (.srt). | faster-whisper (MIT) |

## Install

### Easiest: the installer (Mac or Linux)

On the computer that will keep your photos (e.g. the iMac), open **Terminal** and paste:

```sh
curl -fsSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/immich-studio/installer/install.sh | bash
```

Until this work is merged into `main`, use the branch address instead:
`https://raw.githubusercontent.com/fablab503-collab/airbridge/claude/jolly-edison-ugpm94/immich-studio/installer/install.sh`
(or double-click `installer/Install Gimmich.command` on a Mac, which tries both). It installs Docker and Tailscale if needed,
sets up Immich + Gimmich Studio in `~/gimmich`, keeps the Mac awake, gives it a private `https://` address and
opens **Connect a device**. Run it again any time to update. Options: `PHOTOS_DIR=/Volumes/Big/Photos` (external
drive), `GIMMICH_PORT=2283`, `SKIP_TAILSCALE=1`, `YES=1` (put them before `bash`).

### Connect your phone, tablet and laptop

1. Install **Tailscale** on the device and log in with the same account as the iMac.
2. On the iMac open `…/studio/connect/` (✨ Studio → *Connect a device*) and scan the QR code.
3. The page shows the right thing for that device: Android app + one-tap connect, iPhone/iPad "Add to Home
   Screen" steps, or the desktop app for computers.

### Manual install (next to an existing Immich)

1. Put this `immich-studio` folder inside your `immich-app` folder (next to Immich's `docker-compose.yml`).
2. Start everything:
   ```sh
   docker compose -f docker-compose.yml -f immich-studio/docker-compose.studio.yml up -d --build
   ```
3. Open Immich as usual (`http://<your-computer>:2283`). The ✨ Studio button is bottom-right.

Studio uses your normal Immich login (no extra password) and only sees the photos of whoever is logged in.
The first run downloads about 600 MB of models.

### Make Immich's own face recognition stronger (optional)

Immich → *Administration → Settings → Machine Learning → Facial Recognition → Model*: choose **antelopev2**,
then *Administration → Jobs → Face Detection → All*. See the test results below.

### Using the camera on your phone

Phones only allow the camera on **https://** addresses. Open Immich through HTTPS (for example with
[Tailscale](https://tailscale.com) or a reverse proxy with a certificate), go to `https://<your-address>/studio/camera/`,
log in once, then choose **Share → Add to Home Screen** (iPhone/iPad) or **⋮ → Install app** (Android).

## How well does it work? (measured, not guessed)

All numbers come from the scripts in `eval/`, tested on photos the models never saw while learning.

**Labels** (zero-shot = only describing sentences; trained = small model learned from examples; held-out half tested)

| Label group | Test set | Zero-shot | Trained (used) |
|---|---|---|---|
| Environment, 42 labels | Places365 validation, 1 260 photos | 74.6 % | **77.6 %** (93.5 % in top 3; prison 28 of 34) |
| Era (decade) | fashion-decade-images, 1 252 photos | 62.8 % | **75.7 %** exact, 93.7 % within one decade |
| Clothing type, 16 labels | fashion-product-images, 783 photos | 78.9 % | **93.7 %** |
| Clothing style, 4 labels | fashion-product-images, 240 photos | 61.3 % | **84.2 %** |
| Clothing colour, 12 labels | fashion-product-images, 720 photos | **70.4 %** (used) | 69.2 % (not used) |

**Age** — trained on UTKFace, tested on FG-NET (82 different people, ages 0-69):

| Model | Average error | Within 5 years |
|---|---|---|
| InsightFace genderage (ready-made) | 12.9 years | 23 % |
| Ours on ArcFace numbers | 11.6 years | 22 % |
| **Ours on SigLIP2 numbers (used)** | **5.4 years** | **56 %** |

**Same person at different ages** — FG-NET, "is the most similar other photo the same person?" (rank-1):

| Model | Any age gap | 10+ years apart | 20+ years apart |
|---|---|---|---|
| buffalo_l (Immich default) | 96.2 % | 75.4 % | 62.3 % |
| antelopev2 (stronger, recommended for Immich) | 96.7 % | 76.5 % | 65.5 % |
| 478 face-mesh points only | 11.4 % | 4.5 % | 2.6 % |

Adding our trained "ignore ageing" layer (tested on *other* people only, 5-fold, so smaller search sets):
antelopev2 86.0 % → **92.6 %** at 10+ years, 74.2 % → **76.4 %** at 20+ years.
Each match gets a level:

| Level | Meaning (measured on FG-NET people the model never saw) |
|---|---|
| **confirmed** | you ticked ✓ (or it joins people you confirmed: A = B and B = C → A = C) |
| **certain** (score ≥ 0.72) | **0 wrong-person matches** among ~282 000 wrong pairs in 3 test rounds; about 25 % of same-person pairs reach it |
| **likely** (≥ 0.36) | about 1 wrong match in 100; 87 % of same-person pairs reach it |
| unsure | check yourself |

**Teach it your people** (✓ / ✗ on matches, then *Learn from my answers*) — simulated on unseen FG-NET people,
tested only on photos that were *not* confirmed (`eval/learn_from_confirmations_eval.py`):

| You confirm per person | Found at 20+ years apart | Same-person pairs reaching "certain" | Wrong person reaching "certain" |
|---|---|---|---|
| nothing | 74–79 % | ~25 % | 0 |
| 3 photos | **88.8 %** | 43 % | **0 of 53 647** |
| 6 photos | **93.0 %** | 48 % | **0 of 24 370** |

**Face points:** MediaPipe's 478-point 3D mesh + InsightFace's 106-point outline = **584 points per face**.

**Everything else** — `studio/tests/test_formats.py`: 32 of 32 sample files (24 formats + RAW from Canon CR2/CR3,
Nikon, Sony, Fujifilm, Panasonic, Olympus, iPhone DNG). `studio/tests/test_api.py`: 16 of 16 end-to-end checks
(print + AI upscale, photo book, edit/save/delete, RAW/EXR import, Graphite export back to Immich, Whisper transcript, faces).
`studio/tests/camera_devices.js`: camera on 10 device profiles (iPhone 15 Pro Max, iPhone 14, iPad Pro 11, iPad Mini, Pixel 8,
Galaxy S24, Galaxy Tab S9, desktop Chrome/Safari/Edge) — photo, camera switch, video, offline-then-upload, nothing stored on
the device: 10/10 in 3 rounds in a row. These are Chromium with each device's screen, touch and browser ID; Apple's real Safari
engine still needs a check on a real iPhone.

## Honest limits

* **No face system recognises every photo "100%"**, least of all across many years (a baby photo vs. the same person at 40).
  What Studio offers instead: a **certain** level that made no mistakes in our tests (but covers only part of the photos),
  and your own ✓ / ✗ answers, which are always right and make it learn your people. "0 mistakes in our tests" is not a
  promise of 0 mistakes on every future photo.
* "Learn from my answers" updates one shared model for the whole Studio server (all Immich users on it).
* A grid of face points (the 478-point mesh) is great for drawing and measuring a face, but on its own it identifies people
  much worse than ArcFace "face fingerprints" — see the table. Studio uses the mesh for display and ArcFace for identity.
* The "ignore ageing" layer learned from only 82 people (FG-NET). It helped on held-out people, but may help less
  for faces unlike FG-NET's. Delete `trained/age_invariant_antelopev2.pkl` to switch it off.
* Age and era are estimates. Clothing labels are judged on the area under each face; the clothing models learned
  from product photos, so they can be less accurate on everyday snapshots.
* AI upscaling invents plausible detail; it can't recover detail that was never captured. Above ×4 the result gets soft.
* Licences of the training data: the Era model learned from `tonyassi/fashion-decade-images-1` and the clothing
  models from `ashraq/fashion-product-images-small`; neither states a licence, so keep those models for personal use.
  Without `trained/label_heads.pkl` Studio falls back to zero-shot labels.

## Files

```
immich-studio/
  docker-compose.studio.yml   adds Studio + gateway to Immich
  gateway/nginx.conf          one address for Immich + Studio; adds the ✨ button to Immich pages
  studio/                     the Studio app (Python, FastAPI)
    app/main.py               web API
    app/labels.py             label groups (environment, era, clothing …)
    app/faces.py              face finding, recognition, age, 478-point mesh
    app/editor.py             photo edits
    app/formats.py            reading every file format
    app/printing.py           DPI check, AI upscaling, print and book PDFs
    app/video.py              Whisper speech to text
    app/static/               Studio web page + the script added to Immich
    trained/                  small models we trained (environment, era, age)
    tests/                    format test + end-to-end test
  graphite/                   Graphite patch + build script
  eval/                       training + testing scripts behind the numbers above
```
