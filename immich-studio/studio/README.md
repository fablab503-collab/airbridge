# Gimmich Studio (Rust)

One Rust program (axum + tokio) that serves the Studio pages and API next to Immich. Requests run
concurrently; picture and AI work runs on blocking worker threads, so a long job (labelling the whole library,
an AI upscale) never stops the rest from answering.

| File | What it does |
|---|---|
| `src/main.rs` | settings, start-up, helper commands |
| `src/routes.rs` | Immich login (the visitor's own cookie), photo lists, albums, thumbnails, delete, jobs, logo, Connect a device, downloads, camera upload |
| `src/tools.rs` | import any format, edit, Graphite return, print check, print PDF, photo book |
| `src/ai.rs` | AI labels, analyse all, train, faces, face index, find a person at any age, confirm, learn |
| `src/formats.rs` | RAW (rawler), HEIC/AVIF (libheif), JPEG XL, PSD, HDR/EXR, SVG, PDF and rare formats (ImageMagick, ffmpeg) |
| `src/editor.rs` | every edit step |
| `src/mosaic.rs` | photo mosaic: one photo rebuilt from many library photos (classic, blend, puzzle, bubbles) |
| `src/printing.rs` | DPI report, Real-ESRGAN AI upscaling, print and photo book PDFs |
| `src/clip.rs`, `src/labels.rs` | SigLIP2 numbers from Immich's AI server; label groups and trained heads |
| `src/faces.rs` | SCRFD, ArcFace R100, age-ignoring layer, age, 106 outline points, 478-point mesh (ONNX Runtime) |
| `src/learn.rs` | training: label heads (logistic regression) and the age-ignoring layer (PCA + LDA) |
| `src/video.rs` | Whisper speech-to-text (whisper.cpp) with the Silero voice detector |

## How it was moved from Python, step by step

The Studio was first written in Python. It was rewritten in four steps; after each one the same end-to-end
test (`tests/test_api.py`, 16 checks) passed, and the results were compared with the Python version on the
same photos:

| Step | Moved to Rust | Checked against the Python version |
|---|---|---|
| 1 | pages, login, photo lists, logo, Connect a device, camera upload, jobs | 16/16 end-to-end, camera on 10 devices |
| 2 | file formats, import, editing, printing, photo book, AI upscaling | all 20 edit steps give the same pixels as Pillow (most exactly); upscaler identical (max 1 of 255); RAW same brightness/contrast as LibRaw and about 2× faster; 32/32 sample files open |
| 3 | AI labels, faces, age, face index, find at any age, training | face fingerprints 0.9996 similar on average (1.0 = identical); 478-point mesh within 1.2 % of the eye distance of MediaPipe; 91 of 96 label groups identical on 12 photos (the rest borderline calls); ages about 1 year apart on average; training gives scikit-learn's results (age-ignoring layer to 0.000001, label heads to 0.0002) |
| 4 | video speech-to-text; Python removed | same transcript on the test video |

Honest differences: the age model reacts to tiny pixel changes (OpenCV's JPEG writer vs Rust's alone moves it
0.6 years), so ages are not byte-for-byte the Python ones; and the 478-point mesh crops the face from SCRFD's box
and eyes instead of MediaPipe's own face finder.

## Trained models

Plain files in `trained/` (copied to the models volume on first start; training writes the new ones there):
`label_heads.json`, `age_head.json`, `age_invariant.json`, `fgnet_antelopev2.json/.f32`, and `face_mesh_478.onnx`
(MediaPipe face landmarks model, Apache-2.0, converted to ONNX with tf2onnx). They come from the training scripts in
`../eval/` through `../eval/export_trained.py`. Downloaded on first start (`download_models.sh`): InsightFace
buffalo_l + antelopev2, Real-ESRGAN, Whisper (`STUDIO_WHISPER_MODEL`, default base) and the Silero voice detector.

## Build and run

Docker (what `docker-compose.studio.yml` does): `docker build -t gimmich-studio .`

By hand: `cargo build --release` → `target/release/gimmich-studio`. Needs the system `libheif`, `cmake` + `clang`
(to build Whisper), and at run time ONNX Runtime ≥ 1.22 (`ORT_DYLIB_PATH` = path to `libonnxruntime.so`), ffmpeg,
ImageMagick and Poppler.

Settings (environment): `IMMICH_URL`, `IMMICH_ML_URL`, `IMMICH_API_KEY` (optional), `STUDIO_DATA_DIR`,
`STUDIO_MODEL_DIR`, `STUDIO_TRAINED_DIR`, `STUDIO_STATIC_DIR`, `STUDIO_GRAPHITE_DIR`, `STUDIO_DOWNLOADS_DIR`,
`STUDIO_BRAND_DEFAULT_DIR`, `STUDIO_CLIP_MODEL`, `STUDIO_WHISPER_MODEL`, `STUDIO_CAMERA_ALBUM`,
`GIMMICH_PUBLIC_URL`, `PORT`.

Helper commands: `check-formats DIR...` (open every file), `convert IN OUT.jpg [QUALITY]`,
`edit IN OUT.png '[steps]'`, `upscale IN OUT.png 4`, `faces IMG...`, `transcribe VIDEO`,
`train-age-invariant TRAINED_DIR OUT.json`, `train-logistic X.json Y.json OUT.json`. `cargo test` covers the editor,
print maths, PDFs, face alignment, training and the mosaic.
