//! Readable terminal summaries.

use crate::scan::Report;
use crate::util::{bar, human, thousands};
use crate::volumes::Volume;
use std::fmt::Write;

pub fn volumes_text(vols: &[Volume]) -> String {
    let mut o = String::new();
    if vols.is_empty() {
        o.push_str("No volumes found.\n");
    }
    for v in vols {
        let pct = (v.used_bytes * 100).checked_div(v.total_bytes).unwrap_or(0);
        let _ = writeln!(
            o,
            "{}  ({}{})",
            v.mount_point,
            v.fs_type,
            if v.read_only { ", read-only" } else { "" }
        );
        let _ = writeln!(o, "  device: {}", v.device);
        if v.total_bytes > 0 {
            let _ = writeln!(
                o,
                "  {} {:>3}%  used {} of {}, free {}",
                bar(v.used_bytes, v.total_bytes, 24),
                pct,
                human(v.used_bytes),
                human(v.total_bytes),
                human(v.free_bytes)
            );
        }
        let _ = writeln!(o, "  {}: {}", v.explain.family, v.explain.plain);
        for n in &v.explain.notes {
            let _ = writeln!(o, "    • {n}");
        }
        o.push('\n');
    }
    o
}

pub fn scan_text(r: &Report) -> String {
    let mut o = String::new();
    let t = &r.totals;
    let _ = writeln!(o, "fsmap scan — {:.1}s", r.elapsed_secs);
    for root in &r.roots {
        let _ = writeln!(
            o,
            "\n📁 {}  —  {} in {} files",
            root.path,
            human(root.totals.bytes),
            thousands(root.totals.files)
        );
        if let (Some(fs), Some(e)) = (&root.fs_type, &root.explain) {
            let _ = writeln!(
                o,
                "   on {} ({}): {}",
                root.mount_point.as_deref().unwrap_or("?"),
                fs,
                e.plain
            );
            if let (Some(tot), Some(used), Some(free)) = (
                root.volume_total_bytes,
                root.volume_used_bytes,
                root.volume_free_bytes,
            ) {
                if tot > 0 {
                    let _ = writeln!(
                        o,
                        "   disk: {} used {} of {}, free {}",
                        bar(used, tot, 20),
                        human(used),
                        human(tot),
                        human(free)
                    );
                }
            }
        }
    }
    let _ = writeln!(o, "\nTotals");
    let _ = writeln!(
        o,
        "  {} files · {} folders · {} symlinks{}",
        thousands(t.files),
        thousands(t.folders),
        thousands(t.symlinks),
        if t.other_entries > 0 {
            format!(" · {} other", thousands(t.other_entries))
        } else {
            String::new()
        }
    );
    let _ = writeln!(
        o,
        "  size {}  (on disk {})",
        human(t.bytes),
        human(t.allocated_bytes)
    );
    if t.hardlinks_counted_once > 0 {
        let _ = writeln!(
            o,
            "  {} extra hard links counted once",
            thousands(t.hardlinks_counted_once)
        );
    }
    if t.unreadable > 0 {
        let _ = writeln!(
            o,
            "  ⚠ {} entries could not be read, e.g.:",
            thousands(t.unreadable)
        );
        for e in t.error_samples.iter().take(3) {
            let _ = writeln!(o, "     {e}");
        }
    }

    for l in &r.biggest_folders {
        if l.folders.is_empty() {
            continue;
        }
        let _ = writeln!(o, "\nBiggest folders (level {})", l.level);
        let max = l.folders[0].bytes;
        for f in l.folders.iter().take(r.settings.top.min(15)) {
            let _ = writeln!(
                o,
                "  {:>9}  {}  {}",
                human(f.bytes),
                bar(f.bytes, max, 16),
                f.path
            );
        }
    }

    if !r.kinds.is_empty() {
        let _ = writeln!(o, "\nFile kinds");
        for k in &r.kinds {
            let exts: Vec<String> = k
                .top_extensions
                .iter()
                .take(4)
                .map(|e| e.ext.clone())
                .collect();
            let _ = writeln!(
                o,
                "  {:>9}  {}  {:<17} {:>10} files  ({})",
                human(k.bytes),
                bar(k.bytes, t.bytes, 16),
                k.label,
                thousands(k.files),
                exts.join(", ")
            );
        }
    }

    if !r.largest_files.is_empty() {
        let _ = writeln!(o, "\nLargest files");
        for f in r.largest_files.iter().take(r.settings.top.min(15)) {
            let _ = writeln!(o, "  {:>9}  {}", human(f.bytes), f.path);
        }
    }

    let _ = writeln!(o, "\nAge (last modified)");
    for a in &r.ages {
        let _ = writeln!(
            o,
            "  {:>9}  {}  {:<20} {:>10} files",
            human(a.bytes),
            bar(a.bytes, t.bytes, 16),
            a.label,
            thousands(a.files)
        );
    }

    if !r.special_folders.is_empty() {
        let _ = writeln!(o, "\nSpecial folders");
        for s in &r.special_folders {
            let _ = writeln!(
                o,
                "  {:>9}  {:<34} {} folder(s)",
                human(s.bytes),
                s.label,
                thousands(s.folders)
            );
            let _ = writeln!(o, "             {}", s.explain);
            for b in s.biggest.iter().take(3) {
                let _ = writeln!(o, "             {:>9}  {}", human(b.bytes), b.path);
            }
        }
    }
    if !r.obsidian_vaults.is_empty() {
        let _ = writeln!(o, "\nObsidian vaults");
        for v in &r.obsidian_vaults {
            let _ = writeln!(
                o,
                "  {:>9}  {} notes  {}",
                human(v.bytes),
                thousands(v.notes),
                v.path
            );
        }
    }
    if !r.clutter.is_empty() {
        let _ = writeln!(o, "\nClutter files");
        for c in &r.clutter {
            let _ = writeln!(
                o,
                "  {:>9}  {:<18} {:>8} files — {}",
                human(c.bytes),
                c.label,
                thousands(c.files),
                c.explain
            );
        }
    }
    if let Some(d) = &r.duplicates {
        let _ = writeln!(o, "\nDuplicates");
        let _ = writeln!(
            o,
            "  {} groups, {} extra copies, {} could be saved",
            thousands(d.groups.len() as u64),
            thousands(d.duplicate_files),
            human(d.wasted_bytes)
        );
        for g in d.groups.iter().take(r.settings.top.min(10)) {
            let _ = writeln!(
                o,
                "  {:>9} wasted · {} copies of {}",
                human(g.wasted_bytes),
                g.paths.len(),
                human(g.size)
            );
            for p in g.paths.iter().take(4) {
                let _ = writeln!(o, "             {p}");
            }
        }
        let _ = writeln!(o, "  note: {}", d.note);
    }
    let _ = writeln!(o, "\nfsmap only reads. Nothing was changed.");
    o
}
