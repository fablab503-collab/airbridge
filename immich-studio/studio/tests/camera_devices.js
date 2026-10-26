// Loop the Studio Camera over many phone/tablet/computer profiles with a fake camera.
// For each device: open the camera, take a photo, switch cameras, record a short video,
// and check that both arrive in Immich (album "Studio Camera") and nothing was saved on the device.
//
// Usage: node tests/camera_devices.js BASE_URL EMAIL PASSWORD FAKE_CAMERA.y4m OUT_DIR
const { chromium, devices } = require("playwright");
const fs = require("fs");
const path = require("path");

const [base, email, password, fakeCam, outDir] = process.argv.slice(2);
const DEVICES = ["iPhone 15 Pro Max", "iPhone 14", "iPad Pro 11", "iPad Mini landscape", "Pixel 8", "Galaxy S24",
                 "Galaxy Tab S9", "Desktop Chrome", "Desktop Safari", "Desktop Edge"];

async function api(token, method, p, body) {
  const r = await fetch(base + p, { method, headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
                                    body: body && JSON.stringify(body) });
  return r.json();
}

(async () => {
  const login = await fetch(`${base}/api/auth/login`, { method: "POST", headers: { "Content-Type": "application/json" },
                                                         body: JSON.stringify({ email, password }) }).then((r) => r.json());
  const token = login.accessToken;
  const host = new URL(base).hostname;
  const browser = await chromium.launch({ args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream",
                                                 `--use-file-for-fake-video-capture=${fakeCam}`] });
  const results = [];
  for (const name of DEVICES) {
    const d = { ...devices[name] };
    if (d.defaultBrowserType && d.defaultBrowserType !== "chromium") delete d.defaultBrowserType;
    const ctx = await browser.newContext({ ...d, permissions: ["camera", "microphone"], acceptDownloads: true });
    await ctx.addCookies([{ name: "immich_access_token", value: token, domain: host, path: "/" }]);
    const page = await ctx.newPage();
    const errors = [];
    let downloads = 0;
    page.on("pageerror", (e) => errors.push(e.message));
    page.on("download", () => downloads++);
    const r = { device: name, viewport: `${d.viewport.width}x${d.viewport.height}`, checks: {} };
    try {
      await page.goto(`${base}/studio/camera/`, { waitUntil: "load" });
      await page.waitForFunction(() => document.querySelector("#view").videoWidth > 0, null, { timeout: 30000 });
      await page.waitForFunction(() => /Ready/.test(document.querySelector("#status").textContent), null, { timeout: 30000 });
      r.checks.camera_on = true;
      r.camera = await page.evaluate(() => `${document.querySelector("#view").videoWidth}x${document.querySelector("#view").videoHeight}`);

      // photo
      const photo = page.evaluate(() => new Promise((res) => addEventListener("studio-uploaded", (e) => res(e.detail), { once: true })));
      await page.tap("#shutter").catch(() => page.click("#shutter"));
      const p = await Promise.race([photo, new Promise((_, rej) => setTimeout(() => rej(new Error("photo upload timeout")), 60000))]);
      const asset = await api(token, "GET", `/api/assets/${p.asset}`);
      r.checks.photo_in_immich = asset.type === "IMAGE";
      r.photo = `${asset.exifInfo?.exifImageWidth}x${asset.exifInfo?.exifImageHeight}, ${Math.round(p.bytes / 1024)} KB`;
      await page.screenshot({ path: path.join(outDir, `${name.replace(/\W+/g, "_")}.png`) });

      // front camera
      await page.click("#flip");
      await page.waitForFunction(() => document.body.dataset.ready === "1", null, { timeout: 20000 });
      r.checks.switch_camera = true;

      // video
      await page.click('[data-mode="video"]');  // record immediately: the app itself must wait for the camera
      const video = page.evaluate(() => new Promise((res) => addEventListener("studio-uploaded", (e) => res(e.detail), { once: true })));
      await page.click("#shutter");
      await page.waitForTimeout(2500);
      await page.click("#shutter");
      const v = await Promise.race([video, new Promise((_, rej) => setTimeout(() => rej(new Error("video upload timeout")), 90000))]);
      const vasset = await api(token, "GET", `/api/assets/${v.asset}`);
      r.checks.video_in_immich = vasset.type === "VIDEO" || /\.(mp4|webm)$/.test(vasset.originalFileName);

      // no internet: the photo waits in memory, then uploads by itself when the connection is back
      await page.click('[data-mode="photo"]');
      await ctx.setOffline(true);
      const late = page.evaluate(() => new Promise((res) => addEventListener("studio-uploaded", (e) => res(e.detail), { once: true })));
      await page.click("#shutter");
      await page.waitForFunction(() => /Uploading 1/.test(document.querySelector("#status").textContent), null, { timeout: 20000 });
      await page.waitForTimeout(3000);
      await ctx.setOffline(false);
      await page.evaluate(() => dispatchEvent(new Event("online")));
      const o = await Promise.race([late, new Promise((_, rej) => setTimeout(() => rej(new Error("offline photo never uploaded")), 60000))]);
      r.checks.offline_then_upload = !!o.asset;

      // in the album, and nothing kept on the device
      const album = (await api(token, "GET", "/api/albums")).find((a) => a.albumName === "Studio Camera");
      const inAlbum = await api(token, "POST", "/api/search/metadata", { albumIds: [album.id], size: 1000 });
      const ids = inAlbum.assets.items.map((a) => a.id);
      r.checks.in_album = ids.includes(p.asset) && ids.includes(v.asset);
      const stored = await page.evaluate(async () => {
        const cached = [];
        for (const k of await caches.keys()) for (const req of await (await caches.open(k)).keys()) cached.push(req.url);
        const idb = indexedDB.databases ? (await indexedDB.databases()).length : 0;
        return { cached: cached.filter((u) => !/\/camera\/(|index\.html|manifest\.webmanifest|icon-\d+\.png)$/.test(new URL(u).pathname)),
                 local: localStorage.length, idb };
      });
      r.checks.nothing_saved_on_device = downloads === 0 && stored.cached.length === 0 && stored.local === 0 && stored.idb === 0;
      r.checks.no_errors = errors.length === 0;
      if (errors.length) r.errors = errors;
    } catch (e) {
      r.error = e.message;
      r.status = await page.evaluate(() => document.querySelector("#status")?.textContent).catch(() => "?");
      await page.screenshot({ path: path.join(outDir, `${name.replace(/\W+/g, "_")}-FAIL.png`) }).catch(() => {});
    }
    const EXPECTED = ["camera_on", "photo_in_immich", "switch_camera", "video_in_immich", "in_album", "nothing_saved_on_device", "no_errors", "offline_then_upload"];
    r.pass = !r.error && EXPECTED.every((k) => r.checks[k] === true);
    results.push(r);
    console.log(`${r.pass ? "PASS" : "FAIL"}  ${name.padEnd(20)} ${r.viewport.padEnd(10)} camera ${r.camera || "-"}  photo ${r.photo || "-"}` +
                `${r.error ? `  ERROR ${r.error} (status: ${r.status})` : ""}${r.pass ? "" : "  " + JSON.stringify(r.checks)}`);
    await ctx.close();
  }
  await browser.close();
  fs.writeFileSync(path.join(outDir, "results.json"), JSON.stringify(results, null, 1));
  const passed = results.filter((r) => r.pass).length;
  console.log(`\n${passed}/${results.length} devices passed`);
  process.exit(passed === results.length ? 0 : 1);
})();
