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
| 3 | AI labels, faces, age, find-at-any-age | |
| 4 | video (Whisper); Python removed | |

Step 2 checks (same picture in, compare with the Python Studio):
- all 20 edit steps give the same pixels as Pillow (mean difference ≤ 0.6 of 255; most are exact)
- the AI upscaler output is identical (max difference 1)
- RAW development: same brightness and contrast as LibRaw; RAF opens about 2× faster
- `studio-rs check-formats DIR...` opens all 32 sample files; `cargo test` covers editor, print maths and PDFs

Build: `cargo build --release` → `target/release/studio-rs`. Needs the system `libheif`, and ONNX Runtime
for upscaling (`ORT_DYLIB_PATH` = path to `libonnxruntime.so`, models in `STUDIO_MODEL_DIR`).
Helper commands: `studio-rs convert IN OUT.jpg`, `studio-rs edit IN OUT.png '[steps]'`, `studio-rs upscale IN OUT.png 4`.
Settings (environment): `IMMICH_URL`, `STUDIO_PY_URL`, `STUDIO_DATA_DIR`, `STUDIO_STATIC_DIR`,
`STUDIO_GRAPHITE_DIR`, `STUDIO_DOWNLOADS_DIR`, `STUDIO_BRAND_DEFAULT_DIR`, `STUDIO_MODEL_DIR`, `GIMMICH_PUBLIC_URL`, `PORT`.
