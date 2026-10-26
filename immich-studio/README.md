# Immich Studio

Extra AI tools that plug into [Immich](https://immich.app) (the self-hosted photo app).
Everything runs on your own computer, and every model is open source.

After installing, Immich gets a **✨ Studio** button. On any photo it offers:

| Tool | What it does | Open-source parts |
|---|---|---|
| **AI labels** | Tags each photo with environment (42 kinds of place, including prison), indoors/outdoors, light, number of people, era/decade, age group of each face, and clothing (type, colour, style). The tags become normal Immich tags, so Immich search finds them. | SigLIP2 (through Immich's own ML server), InsightFace, our trained models |
| **Teach your own labels** | Make albums named `Train: Group/Label` (e.g. `Train: Clothing/School uniform`) with ~10 photos each, press *Train*. | scikit-learn |
| **Faces** | Finds faces, guesses age, draws a **478-point 3D face mesh**. | InsightFace SCRFD + ArcFace, MediaPipe Face Landmarker |
| **Edit** | Brightness, contrast, colour, exposure, warm/cool, hue, sharpness, rotate, mirror, crop, black & white, sepia, auto-fix and **change one colour** (e.g. red shirt → blue). Save as a new photo or replace the original; delete photos. | Pillow, NumPy |
| **Graphite** | Opens the photo in [Graphite](https://graphite.art), a full editor written in Rust (layers, drawing, text, filters). *File → Export* saves the result straight back into Immich. | Graphite (Apache-2.0) + a small patch |
| **Any file format** | Imports RAW (CR2, CR3, NEF, ARW, DNG, RAF, ORF, RW2 …), HEIC, AVIF, JPEG XL, PSD, SVG, PDF, EXR, HDR, TGA, ICO, DDS, QOI, XPM … Formats Immich can't show are converted to full-quality TIFF. | LibRaw (rawpy), libheif, psd-tools, CairoSVG, PDFium, ImageMagick, ffmpeg |
| **Print** | Shows the print quality (DPI) for A6 … A0, upscales with AI when needed, makes a print-ready PDF at the exact paper size (optional crop marks). RAW files are developed at full resolution for printing. | Real-ESRGAN (BSD-3), ReportLab |
| **Photo book** | Turns an album into a PDF book (cover + 1, 2 or 4 photos per page with date/place captions). | ReportLab |
| **Video to text** | Writes down what people say in a video (Whisper), puts it in the video's description (so Immich search finds it) and makes subtitles (.srt). | faster-whisper (MIT) |

## Install (next to an existing Immich)

1. Put this `immich-studio` folder inside your `immich-app` folder (next to Immich's `docker-compose.yml`).
2. A ready-built Graphite editor is included in `graphite/dist`. To rebuild it yourself (needs Rust + Node.js, 20–60 min):
   ```sh
   ./immich-studio/graphite/build.sh
   ```
3. Start everything:
   ```sh
   docker compose -f docker-compose.yml -f immich-studio/docker-compose.studio.yml up -d --build
   ```
4. Open Immich as usual (`http://<your-computer>:2283`). The ✨ Studio button is bottom-right.
   Studio's own page is `http://<your-computer>:2283/studio/`.

Studio uses your normal Immich login (no extra password) and only sees the photos of whoever is logged in.
The first run downloads about 600 MB of models.

### Make Immich's own face recognition stronger (optional)

Immich → *Administration → Settings → Machine Learning → Facial Recognition → Model*: choose **antelopev2**,
then *Administration → Jobs → Face Detection → All*. See the test results below.

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
At the default "likely same person" threshold (0.36) about **1 in 100** matches is the wrong person and 87 % of
same-person pairs are found.

**Everything else** — `studio/tests/test_formats.py`: 32 of 32 sample files (24 formats + RAW from Canon CR2/CR3,
Nikon, Sony, Fujifilm, Panasonic, Olympus, iPhone DNG). `studio/tests/test_api.py`: 16 of 16 end-to-end checks
(print + AI upscale, photo book, edit/save/delete, RAW/EXR import, Graphite export back to Immich, Whisper transcript, faces).

## Honest limits

* **No face system recognises people "100%"**, least of all across many years (a baby photo vs. the same person at 40).
  Large public tests show the best systems still make mistakes when the age gap is big. Studio shows how sure it is instead of pretending.
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
