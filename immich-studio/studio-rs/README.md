# Gimmich Studio in Rust (`studio-rs`)

One Rust program (axum + tokio) that serves the Studio page and API. Requests run concurrently;
picture work runs on blocking worker threads.

The port is done in steps. Routes already in Rust are handled here; every other `/api/...` request is
passed through unchanged to the Python Studio (`STUDIO_PY_URL`), so everything keeps working while
pieces move over. The same end-to-end tests (`../studio/tests/test_api.py`, `camera_devices.js`) must pass
after every step.

| Step | In Rust | Still Python |
|---|---|---|
| 1 ✅ | pages, Immich login, photo lists, albums, thumbnails, delete, logo/brand, Connect a device, downloads, camera upload, job tracking | formats & import, edit, print & book, AI labels, faces, video |
| 2 ✅ | every file format (RAW via rawler, HEIC, JPEG XL, PSD, HDR/EXR, SVG ...), import, all edit steps, Graphite return, print check, print PDF, photo book PDF, Real-ESRGAN AI upscaling (ONNX Runtime) | AI labels, faces, video |
| 3 ✅ | AI labels (SigLIP2 through Immich's AI server + trained heads), faces (SCRFD, ArcFace R100, age-ignoring layer, age, 106 outline + 478 mesh points), face index, find-at-any-age, confirm, learn, train | video |
| 4 | video (Whisper); Python removed | |

Step 2 checks (same picture in, compare with the Python Studio):
- all 20 edit steps give the same pixels as Pillow (mean difference ≤ 0.6 of 255; most are exact)
- the AI upscaler output is identical (max difference 1)
- RAW development: same brightness and contrast as LibRaw; RAF opens about 2× faster
- `studio-rs check-formats DIR...` opens all 32 sample files; `cargo test` covers editor, print maths and PDFs

Step 3 checks (same photos through both Studios):
- face fingerprints: similarity 0.9996 on average with the Python Studio (1.0 = identical), worst 0.995
- 478-point mesh: within 1.2 % of the eye distance of MediaPipe on average (MediaPipe's own model, converted
  to ONNX; Rust crops the face from SCRFD's box and eyes instead of MediaPipe's face finder)
- label groups: 91 of 96 identical on 12 library photos (the rest were borderline calls); ages ~1 year apart on
  average (the age model is sensitive to the JPEG writer: OpenCV vs Rust alone moves it 0.6 years)
- training gives scikit-learn's results: the age-ignoring layer to 0.000001, label heads to 0.0002

Trained models are read as plain files (made once from the Python pickles by `export_trained.py`):
`label_heads.json`, `age_head.json`, `age_invariant.json`, `fgnet_antelopev2.json/.f32`, and
`face_mesh_478.onnx` (MediaPipe face landmarks model, Apache-2.0, converted with tf2onnx).

Build: `cargo build --release` → `target/release/studio-rs`. Needs the system `libheif`, and ONNX Runtime
for upscaling (`ORT_DYLIB_PATH` = path to `libonnxruntime.so`, models in `STUDIO_MODEL_DIR`).
Helper commands (for checking against the Python Studio): `convert IN OUT.jpg [QUALITY]`, `edit IN OUT.png '[steps]'`,
`upscale IN OUT.png 4`, `faces IMG...`, `train-age-invariant TRAINED_DIR OUT.json`, `train-logistic X.json Y.json OUT.json`.
Settings (environment): `IMMICH_URL`, `STUDIO_PY_URL`, `STUDIO_DATA_DIR`, `STUDIO_STATIC_DIR`,
`STUDIO_GRAPHITE_DIR`, `STUDIO_DOWNLOADS_DIR`, `STUDIO_BRAND_DEFAULT_DIR`, `STUDIO_MODEL_DIR`, `STUDIO_TRAINED_DIR`, `IMMICH_ML_URL`, `STUDIO_CLIP_MODEL`, `GIMMICH_PUBLIC_URL`, `PORT`.
