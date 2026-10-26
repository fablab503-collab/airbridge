// Added to every Immich page by the gateway (nginx sub_filter). Puts a "Studio" button
// in Immich; on a photo it offers Studio's tools for that photo in a panel.
(() => {
  if (window.__immichStudio) return;
  window.__immichStudio = true;
  const BASE = "/studio/";

  // Your own logo (set in Studio → Logo): replaces Immich's logo, tab icon and home-screen icon.
  fetch(BASE + "api/brand").then((r) => r.json()).then((b) => {
    if (!b.logo) return;
    const logo = BASE + b.logo;
    const swap = () => {
      for (const img of document.querySelectorAll('img[alt="Immich logo"]')) {
        // the login page's big blurred background copy of the logo: hidden (a photo-like logo turns into grey
        // smudges there, and Safari/WebKit doesn't blur it)
        if (img.parentElement.className.includes("-z-10")) { img.style.display = "none"; continue; }
        if (img.dataset.studio !== logo) {
          img.src = logo; img.dataset.studio = logo; img.style.objectFit = "contain";
          img.style.aspectRatio = "auto"; img.style.width = "auto"; // Immich draws its logo in a square box on some pages
        }
        // the name written next to the logo (Immich's own logo had the word "immich" inside the picture)
        if (b.name && !img.nextElementSibling?.classList.contains("studio-brand-name")) {
          const big = img.closest("form, [class*=login], main") && !img.closest("a");
          const span = document.createElement("span");
          span.className = "studio-brand-name";
          span.textContent = b.name;
          span.style.cssText = `font:500 ${big ? 34 : 26}px Roboto,"Studio Roboto",sans-serif;color:#4250af;margin-left:10px;` +
                               `vertical-align:middle;${big ? "display:block;text-align:center;margin:6px 0 0" : ""}`;
          if (img.parentElement.tagName === "A") img.parentElement.style.display = "flex", img.parentElement.style.alignItems = "center";
          img.after(span);
        }
      }
      if (b.name && document.title.includes("Immich")) document.title = document.title.replace(/Immich/g, b.name);
    };
    if (b.name) {
      // Roboto, Android's font, served by Studio itself (no outside font service)
      const f = document.createElement("style");
      f.textContent = `@font-face{font-family:"Studio Roboto";font-weight:500;font-display:swap;src:url(${BASE}fonts/roboto-500.woff2) format("woff2")}`;
      document.head.append(f);
      let meta = document.querySelector('meta[name="apple-mobile-web-app-title"]');
      if (!meta) { meta = document.createElement("meta"); meta.name = "apple-mobile-web-app-title"; document.head.append(meta); }
      meta.content = b.name; // name under the icon on the iPhone home screen
    }
    for (const l of document.querySelectorAll('link[rel*="icon"]')) l.remove();
    for (const [rel, file] of [["icon", "favicon-32.png"], ["apple-touch-icon", "icon-180.png"]]) {
      const l = document.createElement("link"); l.rel = rel; l.href = BASE + "api/brand/" + file; document.head.append(l);
    }
    swap();
    new MutationObserver(swap).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["src"] });
  }).catch(() => {});
  const css = `
    #studio-fab{position:fixed;right:18px;bottom:18px;z-index:2147483000;display:flex;flex-direction:column;align-items:flex-end;gap:6px;font:14px system-ui,sans-serif}
    #studio-fab button{border:0;border-radius:999px;padding:9px 14px;background:#4250af;color:#fff;cursor:pointer;box-shadow:0 2px 10px rgba(0,0,0,.3);font:inherit}
    #studio-fab .menu button{background:#fff;color:#1d2130}
    #studio-fab .menu{display:none;flex-direction:column;align-items:flex-end;gap:6px}
    #studio-fab.open .menu{display:flex}
    #studio-panel{position:fixed;inset:0;z-index:2147483001;background:rgba(0,0,0,.55);display:none}
    #studio-panel.open{display:block}
    #studio-panel iframe{position:absolute;inset:3vh 2vw;width:96vw;height:94vh;border:0;border-radius:14px;background:#fff}
    #studio-panel .x{position:absolute;right:2vw;top:3vh;margin:8px;z-index:2;border:0;border-radius:999px;width:34px;height:34px;cursor:pointer;font-size:18px}`;
  const style = document.createElement("style");
  style.textContent = css;
  document.head.append(style);

  const fab = document.createElement("div");
  fab.id = "studio-fab";
  fab.innerHTML = `<div class="menu"></div><button class="main" title="Immich Studio">✨ Studio</button>`;
  const panel = document.createElement("div");
  panel.id = "studio-panel";
  panel.innerHTML = `<button class="x" title="Close">✕</button><iframe title="Immich Studio"></iframe>`;
  document.body.append(fab, panel);

  const open = (url) => { panel.querySelector("iframe").src = url; panel.classList.add("open"); fab.classList.remove("open"); };
  panel.querySelector(".x").onclick = () => { panel.classList.remove("open"); panel.querySelector("iframe").src = "about:blank"; };
  const assetId = () => location.pathname.match(/\/photos\/([0-9a-f-]{36})/)?.[1];

  fab.querySelector(".main").onclick = () => {
    const id = assetId();
    const items = id
      ? [["AI labels & faces", `?asset=${id}&tab=ai`], ["Edit & colours", `?asset=${id}&tab=edit`],
         ["Open in Graphite", "graphite"], ["Print A4–A1", `?asset=${id}&tab=print`], ["Video to text", `?asset=${id}&tab=video`],
         ["Studio home", ""]]
      : [["Studio home", ""], ["Camera", "camera/"], ["Photo book", "?view=book"], ["Label & train", "?view=train"], ["Import any file", "?view=import"]];
    fab.querySelector(".menu").innerHTML = "";
    for (const [label, q] of items) {
      const b = document.createElement("button");
      b.textContent = label;
      b.onclick = () => {
        if (q === "camera/") {
          location.href = BASE + q; // full screen: browsers block cameras inside embedded panels
        } else if (q === "graphite") {
          const base = new URL(BASE, location.origin);
          window.open(`${BASE}graphite/#immich-open=${encodeURIComponent(new URL(`api/original/${id}`, base))}` +
            `&immich-return=${encodeURIComponent(new URL(`api/graphite-return/${id}`, base))}`, "_blank");
          fab.classList.remove("open");
        } else open(BASE + q);
      };
      fab.querySelector(".menu").append(b);
    }
    fab.classList.toggle("open");
  };
})();
