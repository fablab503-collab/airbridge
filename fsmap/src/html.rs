//! One self-contained offline HTML page (no scripts, no network). Light & dark, phone-friendly.

use crate::scan::Report;
use crate::util::{html_escape as esc, human, thousands, utc_string};
use std::fmt::Write;

const CSS: &str = r#"
:root{--bg:#f7f7f5;--card:#fff;--ink:#1d1d1f;--muted:#6e6e73;--line:#e3e3e0;--track:#ececea;
--a:#2f6fdb;--b:#1f9d74;--c:#d9822b;--d:#8e5bd8;--warn:#b54708;--warnbg:#fff4e5}
@media (prefers-color-scheme:dark){:root{--bg:#151517;--card:#1f1f22;--ink:#f2f2f4;--muted:#a1a1a8;
--line:#333338;--track:#2c2c31;--a:#6ea0ff;--b:#45c79c;--c:#f0a35b;--d:#b08cf0;--warn:#ffb86b;--warnbg:#3a2a16}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.45 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif}
main{max-width:980px;margin:0 auto;padding:20px 16px 48px}
h1{font-size:1.6rem;margin:.2em 0}h2{font-size:1.1rem;margin:0 0 .6em}
.sub{color:var(--muted);margin:0 0 1.2em}
section{background:var(--card);border:1px solid var(--line);border-radius:14px;padding:16px;margin:14px 0}
.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(140px,1fr));gap:10px}
.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:12px}
.card b{display:block;font-size:1.35rem}.card span{color:var(--muted);font-size:.85rem}
.row{display:grid;grid-template-columns:minmax(0,1fr) 90px;gap:4px 12px;align-items:center;padding:6px 0;border-top:1px solid var(--line)}
.row:first-of-type{border-top:0}
.name{overflow-wrap:anywhere}.num{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}
.track{grid-column:1/-1;height:8px;background:var(--track);border-radius:4px;overflow:hidden}
.fill{height:100%;background:var(--a);border-radius:4px}
.k1 .fill{background:var(--b)}.k2 .fill{background:var(--c)}.k3 .fill{background:var(--d)}
.muted{color:var(--muted);font-size:.88rem}
.note{background:var(--warnbg);color:var(--warn);border-radius:10px;padding:10px 12px;margin:8px 0;font-size:.92rem}
ul{margin:.3em 0;padding-left:1.2em}
details{border-top:1px solid var(--line);padding:6px 0}summary{cursor:pointer;overflow-wrap:anywhere}
code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:.85em;overflow-wrap:anywhere}
footer{color:var(--muted);text-align:center;font-size:.85rem;margin-top:24px}
"#;

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64 * 100.0).clamp(0.0, 100.0)
    }
}

fn bar_row(
    o: &mut String,
    name: &str,
    right: &str,
    part: u64,
    whole: u64,
    class: &str,
    extra: &str,
) {
    let _ = write!(
        o,
        r#"<div class="row {class}"><div class="name">{}{extra}</div><div class="num">{}</div><div class="track"><div class="fill" style="width:{:.1}%"></div></div></div>"#,
        esc(name),
        esc(right),
        pct(part, whole)
    );
}

pub fn render(r: &Report) -> String {
    let mut o = String::with_capacity(64 * 1024);
    let t = &r.totals;
    let title = r
        .roots
        .iter()
        .map(|x| x.path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let _ = write!(
        o,
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light dark"><title>fsmap report</title><style>{CSS}</style></head><body><main>"#
    );
    let _ = write!(
        o,
        r#"<h1>What is on {}</h1><p class="sub">Made by fsmap {} on {} in {:.1}s. fsmap only reads — nothing was changed.</p>"#,
        esc(&title),
        r.version,
        utc_string(r.generated_at),
        r.elapsed_secs
    );

    // Summary cards
    o.push_str(r#"<div class="cards">"#);
    for (big, small) in [
        (human(t.bytes), "total size".to_string()),
        (human(t.allocated_bytes), "on disk".to_string()),
        (thousands(t.files), "files".to_string()),
        (thousands(t.folders), "folders".to_string()),
        (
            r.duplicates
                .as_ref()
                .map_or("—".into(), |d| human(d.wasted_bytes)),
            "in duplicates".into(),
        ),
    ] {
        let _ = write!(
            o,
            r#"<div class="card"><b>{}</b><span>{}</span></div>"#,
            esc(&big),
            esc(&small)
        );
    }
    o.push_str("</div>");
    if t.unreadable > 0 || t.hardlinks_counted_once > 0 || t.symlinks > 0 {
        o.push_str(r#"<p class="muted">"#);
        if t.symlinks > 0 {
            let _ = write!(o, "{} symlinks (not followed). ", thousands(t.symlinks));
        }
        if t.hardlinks_counted_once > 0 {
            let _ = write!(
                o,
                "{} extra hard links counted once. ",
                thousands(t.hardlinks_counted_once)
            );
        }
        if t.unreadable > 0 {
            let _ = write!(o, "{} entries could not be read.", thousands(t.unreadable));
        }
        o.push_str("</p>");
    }

    // Disks
    o.push_str("<section><h2>Where it lives</h2>");
    for root in &r.roots {
        let _ = write!(
            o,
            "<p><b>{}</b> — {} in {} files</p>",
            esc(&root.path),
            human(root.totals.bytes),
            thousands(root.totals.files)
        );
        if let (Some(fs), Some(e)) = (&root.fs_type, &root.explain) {
            if let (Some(tot), Some(used), Some(free)) = (
                root.volume_total_bytes,
                root.volume_used_bytes,
                root.volume_free_bytes,
            ) {
                if tot > 0 {
                    let label =
                        format!("Disk {} ({fs})", root.mount_point.as_deref().unwrap_or("?"));
                    let right = format!("{:.0}% used", pct(used, tot));
                    let extra = format!(
                        r#"<div class="muted">{} used of {}, {} free</div>"#,
                        human(used),
                        human(tot),
                        human(free)
                    );
                    bar_row(&mut o, &label, &right, used, tot, "", &extra);
                    bar_row(
                        &mut o,
                        "This scan",
                        &human(root.totals.allocated),
                        root.totals.allocated,
                        tot,
                        "k1",
                        "",
                    );
                }
            }
            let _ = write!(
                o,
                r#"<p><b>{}</b>: {}</p><ul>"#,
                esc(e.family),
                esc(e.plain)
            );
            for n in &e.notes {
                let _ = write!(o, "<li>{}</li>", esc(n));
            }
            o.push_str("</ul>");
        }
    }
    o.push_str("</section>");

    // Kinds
    if !r.kinds.is_empty() {
        o.push_str("<section><h2>Kinds of files</h2>");
        for (i, k) in r.kinds.iter().enumerate() {
            let exts: Vec<String> = k
                .top_extensions
                .iter()
                .take(5)
                .map(|e| format!(".{}", e.ext))
                .collect();
            let extra = format!(
                r#"<div class="muted">{} files · {}</div>"#,
                thousands(k.files),
                esc(&exts.join(" "))
            );
            bar_row(
                &mut o,
                k.label,
                &human(k.bytes),
                k.bytes,
                t.bytes,
                &format!("k{}", i % 4),
                &extra,
            );
        }
        o.push_str("</section>");
    }

    // Folders
    for l in &r.biggest_folders {
        if l.folders.is_empty() {
            continue;
        }
        let _ = write!(o, "<section><h2>Biggest folders — level {}</h2>", l.level);
        let max = l.folders[0].bytes;
        for f in &l.folders {
            let extra = format!(r#"<div class="muted">{} files</div>"#, thousands(f.files));
            bar_row(&mut o, &f.path, &human(f.bytes), f.bytes, max, "", &extra);
        }
        o.push_str("</section>");
    }

    // Largest files
    if !r.largest_files.is_empty() {
        o.push_str("<section><h2>Largest files</h2>");
        let max = r.largest_files[0].bytes;
        for f in &r.largest_files {
            let extra = format!(
                r#"<div class="muted">changed {}</div>"#,
                utc_string(f.modified)
            );
            bar_row(&mut o, &f.path, &human(f.bytes), f.bytes, max, "k2", &extra);
        }
        o.push_str("</section>");
    }

    // Ages
    o.push_str(r#"<section><h2>How old (last changed)</h2>"#);
    for a in &r.ages {
        let extra = format!(r#"<div class="muted">{} files</div>"#, thousands(a.files));
        bar_row(
            &mut o,
            a.label,
            &human(a.bytes),
            a.bytes,
            t.bytes,
            "k3",
            &extra,
        );
    }
    o.push_str("</section>");

    // Special folders
    if !r.special_folders.is_empty() || !r.obsidian_vaults.is_empty() {
        o.push_str("<section><h2>Special folders</h2>");
        for v in &r.obsidian_vaults {
            let extra = format!(
                r#"<div class="muted">Obsidian vault · {} notes · {} files</div>"#,
                thousands(v.notes),
                thousands(v.files)
            );
            bar_row(
                &mut o,
                &v.path,
                &human(v.bytes),
                v.bytes,
                t.bytes,
                "k1",
                &extra,
            );
        }
        for s in &r.special_folders {
            let _ = write!(
                o,
                r#"<details><summary><b>{}</b> — {} in {} folder(s)</summary><p class="muted">{}</p>"#,
                esc(s.label),
                human(s.bytes),
                thousands(s.folders),
                esc(s.explain)
            );
            for b in &s.biggest {
                bar_row(
                    &mut o,
                    &b.path,
                    &human(b.bytes),
                    b.bytes,
                    s.bytes.max(1),
                    "k3",
                    "",
                );
            }
            o.push_str("</details>");
        }
        o.push_str("</section>");
    }

    // Clutter
    if !r.clutter.is_empty() {
        o.push_str("<section><h2>Clutter files</h2>");
        for c in &r.clutter {
            let _ = write!(
                o,
                r#"<details><summary><b>{}</b> — {} files, {}</summary><p class="muted">{}</p><ul>"#,
                esc(c.label),
                thousands(c.files),
                human(c.bytes),
                esc(c.explain)
            );
            for s in &c.samples {
                let _ = write!(o, "<li><code>{}</code></li>", esc(s));
            }
            o.push_str("</ul></details>");
        }
        o.push_str("</section>");
    }

    // Duplicates
    if let Some(d) = &r.duplicates {
        let _ = write!(
            o,
            r#"<section><h2>Duplicates</h2><p>{} groups · {} extra copies · <b>{}</b> could be saved (files of {} or more).</p><div class="note">{}</div>"#,
            thousands(d.groups.len() as u64),
            thousands(d.duplicate_files),
            human(d.wasted_bytes),
            human(r.settings.min_dupe_size),
            esc(d.note)
        );
        for g in d.groups.iter().take(200) {
            let _ = write!(
                o,
                r#"<details><summary>{} wasted — {} copies of {}</summary><ul>"#,
                human(g.wasted_bytes),
                g.paths.len(),
                human(g.size)
            );
            for p in &g.paths {
                let _ = write!(o, "<li><code>{}</code></li>", esc(p));
            }
            o.push_str("</ul></details>");
        }
        if d.groups.len() > 200 {
            let _ = write!(
                o,
                r#"<p class="muted">…and {} more groups in the JSON report.</p>"#,
                d.groups.len() - 200
            );
        }
        o.push_str("</section>");
    }

    if !t.error_samples.is_empty() {
        o.push_str("<section><h2>Could not read</h2><ul>");
        for e in &t.error_samples {
            let _ = write!(o, "<li><code>{}</code></li>", esc(e));
        }
        o.push_str("</ul></section>");
    }
    o.push_str(
        "<footer>fsmap · read-only disk map · works offline</footer></main></body></html>\n",
    );
    o
}
