# Device tests

Tries the Gimmich app on iPhone, iPad and Apple Watch simulators (Mac) and on an Android phone and
tablet emulator (Linux), using GitHub's free computers. Start it from **Actions → Gimmich on devices →
Run workflow** (`.github/workflows/gimmich-devices.yml`); screenshots are attached to the run.

| File | What it does |
|---|---|
| `gimmich-demo-server.py` | stand-in Gimmich: the real Studio pages with pretend photos (Python standard library only) |
| `gimmich-on-ios.sh` | opens Studio, Connect a device and Camera in Safari on the newest iPhone and iPad simulators; Apple Watch home screen |
| `gimmich-on-android.sh` | installs the Gimmich Camera app, connects it, takes a photo and checks it arrived; opens the Studio pages in Chrome |

The latest screenshots are in [`../docs/device-tests/`](../docs/device-tests/) and in [WHAT-WORKS.md](../WHAT-WORKS.md).
