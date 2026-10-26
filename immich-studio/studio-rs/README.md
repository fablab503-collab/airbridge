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
| 2 | formats & import, edit, print & book, AI upscaling | |
| 3 | AI labels, faces, age, find-at-any-age | |
| 4 | video (Whisper); Python removed | |

Build: `cargo build --release` → `target/release/studio-rs`.
Settings (environment): `IMMICH_URL`, `STUDIO_PY_URL`, `STUDIO_DATA_DIR`, `STUDIO_STATIC_DIR`,
`STUDIO_GRAPHITE_DIR`, `STUDIO_DOWNLOADS_DIR`, `STUDIO_BRAND_DEFAULT_DIR`, `GIMMICH_PUBLIC_URL`, `PORT`.
