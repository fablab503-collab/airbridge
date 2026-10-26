// Added to every Immich page by the gateway (nginx sub_filter). Puts a "Studio" button
// in Immich; on a photo it offers Studio's tools for that photo in a panel.
(() => {
  if (window.__immichStudio) return;
  window.__immichStudio = true;
  const BASE = "/studio/";
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
