//! Integration test on a real temporary folder tree.

use fsmap::scan::{scan, ScanOptions};
use fsmap::special::{Clutter, Special};
use std::collections::BTreeMap;
use std::os::unix::fs::{symlink, MetadataExt};
use std::path::Path;
use std::process::Command;

/// Snapshot of every entry: path -> (size, mtime, inode). Used to prove nothing changed.
fn snapshot(root: &Path) -> BTreeMap<String, (u64, i64, u64)> {
    let mut m = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let md = std::fs::symlink_metadata(e.path()).unwrap();
            m.insert(
                e.path().display().to_string(),
                (md.len(), md.mtime(), md.ino()),
            );
            if md.is_dir() {
                stack.push(e.path());
            }
        }
    }
    m
}

fn build_tree(root: &Path) {
    let w = |p: &str, data: &[u8]| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    };
    let big: Vec<u8> = (0..2_000_000u32).map(|i| (i % 251) as u8).collect();
    w("photos/2024/IMG_0001.jpg", &big);
    w("photos/backup/IMG_0001 copy.jpg", &big); // duplicate
    w("photos/2024/._IMG_0001.jpg", &[0u8; 4096]); // clutter
    w("photos/.DS_Store", &[0u8; 6148]);
    w("models/llama.gguf", &vec![7u8; 3_000_000]);
    w("vault/.obsidian/app.json", b"{}");
    w("vault/Daily/2026-11-02.md", b"# hello");
    w("vault/Ideas.md", b"# ideas");
    w("code/app/node_modules/a/index.js", b"x");
    w("code/app/node_modules/a/node_modules/b/index.js", b"yy"); // nested node_modules
    w("code/app/.git/HEAD", b"ref: refs/heads/main");
    w("nas/#recycle/old.mov", &vec![1u8; 500_000]);
    w("nas/@eaDir/thumb.jpg", &[2u8; 1000]);
    // hard link: same bytes, must be counted once and never reported as a duplicate
    std::fs::hard_link(
        root.join("models/llama.gguf"),
        root.join("models/llama-link.gguf"),
    )
    .unwrap();
    // symlink loop: loop -> . (must not be followed)
    symlink(".", root.join("code/loop")).unwrap();
    symlink(root, root.join("code/app/up")).unwrap();
}

#[test]
fn scans_tree_correctly_and_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    std::fs::create_dir(&root).unwrap();
    build_tree(&root);
    let before = snapshot(&root);

    let opts = ScanOptions {
        depth: 2,
        top: 10,
        dupes: true,
        min_dupe_size: 1 << 20,
        threads: 4,
        ..Default::default()
    };
    let r = scan(&[root.clone(), root.join(".")], &opts).unwrap();

    // duplicate root collapsed, 13 files (the 14th name is a hard link, counted once), 2 symlinks
    assert_eq!(r.roots.len(), 1);
    assert_eq!(r.totals.files, 13);
    assert_eq!(r.totals.hardlinks_counted_once, 1);
    assert_eq!(r.totals.symlinks, 2);
    assert_eq!(r.totals.unreadable, 0);
    let expected_bytes: u64 =
        2_000_000 * 2 + 4096 + 6148 + 3_000_000 + 2 + 7 + 7 + 1 + 2 + 20 + 500_000 + 1000;
    assert_eq!(r.totals.bytes, expected_bytes);

    // duplicates: only the two photos; the hard link is not a duplicate
    let d = r.duplicates.as_ref().unwrap();
    assert_eq!(d.groups.len(), 1, "{:?}", d.groups);
    assert_eq!(d.groups[0].paths.len(), 2);
    assert_eq!(d.wasted_bytes, 2_000_000);
    assert!(d.groups[0].paths.iter().all(|p| p.ends_with(".jpg")));

    // biggest level-1 folders: photos (~4 MB), then models (3 MB)
    let l1 = &r.biggest_folders[0].folders;
    assert!(l1[0].path.ends_with("/photos"));
    assert!(l1[1].path.ends_with("/models"));

    // special folders: nested node_modules counted once
    let cat = |c: Special| {
        r.special_folders
            .iter()
            .find(|s| s.category == c)
            .unwrap_or_else(|| panic!("{c:?}"))
    };
    assert_eq!(cat(Special::NodeModules).folders, 1);
    assert_eq!(cat(Special::NodeModules).files, 2);
    assert_eq!(cat(Special::Git).files, 1);
    assert_eq!(cat(Special::SynologyRecycle).bytes, 500_000);
    assert_eq!(cat(Special::SynologyThumbs).bytes, 1000);

    // obsidian vault found with 2 notes
    assert_eq!(r.obsidian_vaults.len(), 1);
    assert_eq!(r.obsidian_vaults[0].notes, 2);

    // clutter
    assert!(r
        .clutter
        .iter()
        .any(|c| c.kind == Clutter::AppleDouble && c.files == 1));
    assert!(r
        .clutter
        .iter()
        .any(|c| c.kind == Clutter::DsStore && c.files == 1));

    // AI models kind
    assert!(r
        .kinds
        .iter()
        .any(|k| k.label == "AI models" && k.bytes == 3_000_000));

    assert_eq!(snapshot(&root), before, "scan must not change anything");
}

#[test]
fn cli_writes_only_named_reports() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tree");
    std::fs::create_dir(&root).unwrap();
    build_tree(&root);
    let before = snapshot(&root);
    let json = tmp.path().join("r.json");
    let html = tmp.path().join("r.html");
    let out = Command::new(env!("CARGO_BIN_EXE_fsmap"))
        .args(["scan", "--dupes", "--no-progress", "--json"])
        .arg(&json)
        .arg("--html")
        .arg(&html)
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Duplicates") && text.contains("Nothing was changed"));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(v["totals"]["files"], 13);
    let h = std::fs::read_to_string(&html).unwrap();
    assert!(
        h.starts_with("<!doctype html>")
            && h.contains("prefers-color-scheme")
            && !h.contains("<script")
    );
    assert_eq!(snapshot(&root), before);

    let vol = Command::new(env!("CARGO_BIN_EXE_fsmap"))
        .args(["volumes", "--all"])
        .output()
        .unwrap();
    assert!(vol.status.success());
}
