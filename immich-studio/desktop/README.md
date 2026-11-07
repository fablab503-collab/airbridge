# Gimmich desktop (Windows, macOS, Linux)

A small app (Rust + Tauri) that opens your Gimmich server in its own window, with a Gimmich menu:
Photos (Ctrl/⌘+1), Studio tools (Ctrl/⌘+2), Camera (Ctrl/⌘+3), Photo book, Reload, Change server.
The first start asks for the server address (Tailscale `https://…` or the iMac's local `http://192.168.…:2283`).

## Download

* **Windows / macOS / Linux**: GitHub → **Actions** → *Gimmich desktop* → latest run → **Artifacts**
  (`Gimmich-windows` = `.exe` installer, `Gimmich-macos` = `.dmg`, `Gimmich-linux` = `.deb` + `.AppImage`).
* Ready-made copies are also in `release/` (Windows `.exe`, Mac `.dmg`, Linux `.deb`), and every Gimmich offers the right one on *Connect a device*.

The apps are not signed with an Apple or Microsoft certificate (those cost money and need your own account), so:
* **macOS**: right-click the app → **Open** → **Open** the first time.
* **Windows**: "Windows protected your PC" → **More info** → **Run anyway**.

## Build it yourself

Needs Rust and Node.js 22 (+ on Linux: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf`):

```sh
npm ci
npx tauri build          # macOS: add --target universal-apple-darwin for Intel + Apple-silicon
```
