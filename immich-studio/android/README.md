# Gimmich Camera (Android)

A small Android app (60 KB) that opens the Gimmich/Studio camera full screen. Photos and videos go
straight to **your** Gimmich server and are never saved on the phone (the app has no storage permission).

## Install

1. Copy `release/GimmichCamera-1.0.apk` to the phone (or download it from GitHub on the phone).
2. Open it. Android asks to allow installing apps from this source: allow it once.
3. Start **Gimmich Camera**, type your Gimmich address (`https://...`, for example from Tailscale),
   log in once, allow the camera.

Long-press the Back button to change the address later.

## Build it yourself

Needs JDK 17+ and the Android SDK (platform 35):

```sh
echo "sdk.dir=/path/to/android-sdk" > local.properties
./gradlew assembleRelease     # -> app/build/outputs/apk/release/app-release.apk
```

## Notes

* The app shows the web camera from `studio/app/static/camera/` inside Android's WebView, so the camera
  code is the same one tested on 10 device profiles (`studio/tests/camera_devices.js`).
* `app/gimmich.keystore` is a personal signing key kept here so new versions install over old ones.
  Its password is in `app/build.gradle`, so it is **not** private: make your own key before publishing
  the app anywhere (Google Play signs with its own key anyway).
* Android 8 or newer. The camera needs an `https://` address (Android's rule, like iPhones).
