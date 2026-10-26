#!/usr/bin/env python3
"""A stand-in Gimmich server for trying the app on simulators.

Serves the real Gimmich Studio pages (Studio, Camera, Connect a device) from a checkout of
fablab503-collab/airbridge, with a pretend photo library instead of a real Immich server,
so the pages can be opened on iPhone/iPad simulators and Android emulators without
installing Immich. Photos sent from the Camera page are counted and kept in memory.

Usage: gimmich-demo-server.py STATIC_DIR [PORT] [APK_DIR]
Only the Python standard library is used.
"""
import json
import math
import os
import struct
import sys
import zlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

STATIC = os.path.abspath(sys.argv[1])
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8080
APK_DIR = sys.argv[3] if len(sys.argv) > 3 else ""
BRAND = os.path.join(STATIC, "..", "brand-default")
UPLOADS = []
NAMES = ["Beach day", "Mountain walk", "Birthday", "City lights", "Forest", "Sunset", "Lake", "Garden",
         "Snow", "Concert", "Harbour", "Picnic", "Desert", "Bridge", "Waterfall", "Countryside",
         "Kitchen", "Museum", "Playground", "Campsite", "Night sky", "Market", "Old town", "Field"]


def png(w, h, k):
    """A small colourful picture (sky, sun and hills), different for every number k."""
    hue = (k * 47) % 360

    def rgb(hh, s, v):
        c = v * s
        x = c * (1 - abs((hh / 60) % 2 - 1))
        m = v - c
        r, g, b = [(c, x, 0), (x, c, 0), (0, c, x), (0, x, c), (x, 0, c), (c, 0, x)][int(hh // 60) % 6]
        return bytes(int((q + m) * 255) for q in (r, g, b))

    sky, ground, sun = rgb(hue, 0.45, 0.95), rgb((hue + 120) % 360, 0.6, 0.55), rgb((hue + 40) % 360, 0.8, 1.0)
    sx, sy, sr = w * (0.25 + 0.5 * ((k * 37) % 10) / 10), h * 0.3, h * 0.13
    rows = []
    for y in range(h):
        hill = h * 0.62
        row = bytearray(b"\x00")
        for x in range(w):
            top = hill + h * 0.07 * math.sin(x / w * 6.3 + k)
            if y > top:
                row += ground
            elif (x - sx) ** 2 + (y - sy) ** 2 < sr * sr:
                row += sun
            else:
                row += sky
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=STATIC, **kw)

    def log_message(self, fmt, *args):
        sys.stderr.write("demo: " + fmt % args + "\n")

    def send(self, code, body, ctype="application/json"):
        if isinstance(body, (dict, list)):
            body = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def base_url(self):
        proto = self.headers.get("X-Forwarded-Proto") or ("https" if self.headers.get("Cf-Visitor") else "http")
        return f"{proto}://{self.headers.get('Host', f'localhost:{PORT}')}"

    def api(self, path):
        if path == "me":
            return self.send(200, {"name": "Demo", "email": "demo@gimmich.app", "isAdmin": True})
        if path == "health":
            return self.send(200, {"ok": True, "engine": "demo", "uploads": len(UPLOADS)})
        if path == "assets" or path.startswith("assets?"):
            items = [{"id": f"demo-{i}", "name": f"{n}.jpg", "type": "IMAGE", "tags": []} for i, n in enumerate(NAMES)]
            items += [{"id": f"upload-{i}", "name": n, "type": "IMAGE", "tags": ["Studio Camera"]} for i, (n, _) in enumerate(UPLOADS)]
            return self.send(200, {"items": items, "next": None})
        if path == "albums":
            return self.send(200, [{"id": "album-1", "name": "Holidays", "count": 12}, {"id": "album-2", "name": "Studio Camera", "count": len(UPLOADS)}])
        if path.startswith("thumb/") or path.startswith("original/"):
            ident = path.split("/", 1)[1].split("?")[0]
            if ident.startswith("upload-"):
                name, data = UPLOADS[int(ident.split("-")[1])]
                return self.send(200, data, "image/jpeg")
            k = int(ident.split("-")[-1]) if ident.split("-")[-1].isdigit() else 0
            big = path.startswith("original/") or "preview" in path
            return self.send(200, png(480 if big else 240, 320 if big else 160, k), "image/png")
        if path == "brand":
            return self.send(200, {"logo": "api/brand/logo.svg", "name": "Gimmich"})
        if path.startswith("brand/"):
            f = os.path.join(BRAND, os.path.basename(path))
            if os.path.isfile(f):
                return self.send(200, open(f, "rb").read(), "image/svg+xml" if f.endswith(".svg") else "image/png")
        if path.startswith("downloads/android/") and APK_DIR:
            f = os.path.join(APK_DIR, os.path.basename(path))
            if os.path.isfile(f):
                return self.send(200, open(f, "rb").read(), "application/vnd.android.package-archive")
        if path == "connect":
            apks = sorted(os.listdir(APK_DIR)) if APK_DIR and os.path.isdir(APK_DIR) else []
            base = self.base_url()
            return self.send(200, {"address": base, "secure": base.startswith("https://"),
                                   "files": {"android": apks, "desktop": []},
                                   "desktop_builds": "https://github.com/fablab503-collab/airbridge/actions"})
        return self.send(404, {"detail": "This is the demo server: this tool needs a real Gimmich."})

    def do_GET(self):
        p = urlparse(self.path).path
        if p in ("/", "/studio"):
            self.send_response(302)
            self.send_header("Location", "/studio/")
            self.end_headers()
            return
        if p.startswith("/studio/api/"):
            return self.api(self.path.split("/studio/api/", 1)[1])
        if p.startswith("/studio/"):
            self.path = self.path[len("/studio"):]
            return super().do_GET()
        self.send(404, b"not found", "text/plain")

    def do_POST(self):
        p = urlparse(self.path).path
        n = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(n) if n else b""
        if p == "/studio/api/camera/upload":
            # keep the photo's bytes (the part after the first blank line of the form)
            data = body.split(b"\r\n\r\n", 1)[-1].rsplit(b"\r\n--", 1)[0]
            UPLOADS.append((f"camera-{len(UPLOADS) + 1}.jpg", data))
            print(f"demo: camera upload #{len(UPLOADS)} ({len(data)} bytes)", flush=True)
            return self.send(200, {"asset": f"upload-{len(UPLOADS) - 1}", "album": "Studio Camera", "bytes": len(data)})
        return self.send(404, {"detail": "This is the demo server: this tool needs a real Gimmich."})


if __name__ == "__main__":
    print(f"Gimmich demo server on http://localhost:{PORT}/studio/ (pages from {STATIC})", flush=True)
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
