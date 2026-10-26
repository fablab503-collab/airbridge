#!/usr/bin/env bash
# Runs inside a booted Android emulator: installs the Gimmich Camera app, connects it to the
# demo server through the "Connect a device" link, takes a photo with the emulator's pretend
# camera and checks it reached the server. Also opens the Studio pages in the browser.
#
# Usage: gimmich-on-android.sh OUT_DIR LABEL APK SERVER_URL      (SERVER_URL must be https://)
set -uo pipefail

OUT="$1"; LABEL="$2"; APK="$3"; SERVER="$4"
mkdir -p "$OUT"
shot() { adb exec-out screencap -p > "$OUT/$LABEL - $1.png"; echo "    screenshot: $1"; }
uploads() { curl -s "$SERVER/studio/api/health" | sed -n 's/.*"uploads": *\([0-9]*\).*/\1/p'; }
# Tap the on-screen item whose text or description is WORDS (any case). Looks again for a few
# seconds, because dialogs can appear late on a busy emulator; returns 1 if it never shows up.
tap() {
  local try
  for try in 1 2 3 4 5 6; do
    tap_once "$1" && return 0
    sleep 2
  done
  return 1
}
tap_once() {
  adb shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1 || return 1
  local xy
  xy="$(adb exec-out cat /sdcard/ui.xml | python3 -c '
import re, sys
want = sys.argv[1].lower()
for m in re.finditer(r"<node [^>]*>", sys.stdin.read()):
    n = m.group(0)
    t = (re.search(r"text=\"([^\"]*)\"", n) or [None, ""])[1]
    d = (re.search(r"content-desc=\"([^\"]*)\"", n) or [None, ""])[1]
    if want in (t.lower(), d.lower()):
        a, b, c, e = map(int, re.search(r"bounds=\"\[(\d+),(\d+)\]\[(\d+),(\d+)\]\"", n).groups())
        print((a + c) // 2, (b + e) // 2)
        break
' "$1")"
  [[ -n "$xy" ]] || return 1
  adb shell input tap $xy
  echo "    tapped '$1'"
}

echo "==> $LABEL: $(adb shell getprop ro.product.model | tr -d '\r'), Android $(adb shell getprop ro.build.version.release | tr -d '\r')"
adb shell settings put global window_animation_scale 0
adb shell settings put global transition_animation_scale 0
adb shell input keyevent KEYCODE_WAKEUP
adb shell wm dismiss-keyguard || true

echo "--- install the app"
adb install -r "$APK"
adb shell pm grant app.gimmich.camera android.permission.CAMERA || true
adb shell pm grant app.gimmich.camera android.permission.RECORD_AUDIO || true

echo "--- first start: asks for the address"
adb shell am start -W -n app.gimmich.camera/.MainActivity >/dev/null
sleep 6; shot "1 first start"

echo "--- tap 'Open in Gimmich Camera' (the gimmich:// link from Connect a device)"
adb shell am start -W -a android.intent.action.VIEW -d "gimmich://connect?server=$SERVER" >/dev/null
sleep 5; shot "2 asks before connecting"
tap "Connect" || echo "    (no 'Connect' question shown)"
sleep 20
tap_once "Got it" && sleep 3     # Android's one-time "Viewing full screen" tip
shot "3 camera"

echo "--- press the shutter"
before="$(uploads)"
size="$(adb shell wm size | sed -n 's/.*: *\([0-9]*\)x\([0-9]*\).*/\1 \2/p' | tail -1)"
density="$(adb shell wm density | sed -n 's/.*: *\([0-9]*\).*/\1/p' | tail -1)"
w="${size% *}"; h="${size#* }"
# the round shutter button ("Take photo"); if the page doesn't expose it, tap where it sits:
# in the middle, about 95 dp above the bottom edge
tap "Take photo" || adb shell input tap $((w / 2)) $((h - 95 * density / 160))
sleep 10; shot "4 after taking a photo"
after="$(uploads)"
status=0
if [[ -n "$after" && "${after:-0}" -gt "${before:-0}" ]]; then
  echo "RESULT $LABEL: photo arrived at the server ($before -> $after uploads)" | tee -a "$OUT/results.txt"
else
  echo "RESULT $LABEL: no photo arrived ($before -> $after uploads) - see the screenshots" | tee -a "$OUT/results.txt"
  status=1
fi

echo "--- the Studio pages in the browser"
adb root >/dev/null 2>&1 || true
# skip Chrome's welcome screens
adb shell 'echo "chrome --disable-fre --no-default-browser-check --no-first-run" > /data/local/tmp/chrome-command-line' || true
adb shell am set-debug-app --persistent com.android.chrome >/dev/null 2>&1 || true
for page in "studio/:5 Studio" "studio/connect/:6 Connect a device"; do
  adb shell am start -W -a android.intent.action.VIEW -d "$SERVER/${page%%:*}" >/dev/null
  sleep 12
  tap_once "No thanks" && sleep 3  # Chrome's own "notifications" offer
  shot "${page#*:}"
done
exit $status
