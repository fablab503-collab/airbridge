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

RESULTS_PLACEHOLDER

## Honest limits

* **No face system recognises people "100%"**, least of all across many years (a baby photo vs. the same person at 40).
  Large public tests show the best systems still make mistakes when the age gap is big. Studio shows how sure it is instead of pretending.
* A grid of face points (the 478-point mesh) is great for drawing and measuring a face, but on its own it identifies people
  much worse than ArcFace "face fingerprints" — see the table. Studio uses the mesh for display and ArcFace for identity.
* Age and era are estimates. Clothing labels are judged on the area under each face.
* AI upscaling invents plausible detail; it can't recover detail that was never captured. Above ×4 the result gets soft.
* The Era model was trained on `tonyassi/fashion-decade-images-1`, which states no licence — keep it for personal use,
  or delete `trained/label_heads.pkl`'s Era part by retraining without it.

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
