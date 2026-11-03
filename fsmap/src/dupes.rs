//! Duplicate files: same size -> same first+last 64 KiB -> same full xxh3-128.
//! Files are only opened for reading.

use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use xxhash_rust::xxh3::{xxh3_128, Xxh3};

const EDGE: u64 = 64 * 1024;

pub const APFS_NOTE: &str = "On APFS (and Btrfs/ZFS), copies made with Finder may be clones that share the same blocks, so deleting one copy may free little or no space.";

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct DupeGroup {
    pub size: u64,
    pub hash: String,
    pub wasted_bytes: u64,
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct DupeReport {
    pub groups: Vec<DupeGroup>,
    pub duplicate_files: u64,
    pub wasted_bytes: u64,
    pub candidates: u64,
    pub fully_hashed: u64,
    pub read_errors: u64,
    pub note: &'static str,
}

/// Group items by key, keep only groups with 2+ members (order inside a group is preserved).
pub fn groups_with_dupes<K: Hash + Eq, T>(items: Vec<(K, T)>) -> Vec<Vec<T>> {
    let mut map: HashMap<K, Vec<T>> = HashMap::new();
    for (k, t) in items {
        map.entry(k).or_default().push(t);
    }
    map.into_values().filter(|g| g.len() > 1).collect()
}

/// Run `f` on every item with up to `threads` workers; results keep input order.
fn par_map<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = if threads == 0 {
        std::thread::available_parallelism().map_or(4, |n| n.get())
    } else {
        threads
    }
    .clamp(1, 32)
    .min(items.len().max(1));
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= items.len() {
                    break;
                }
                let r = f(&items[i]);
                out.lock().expect("worker panicked")[i] = Some(r);
            });
        }
    });
    out.into_inner()
        .expect("worker panicked")
        .into_iter()
        .map(|r| r.expect("every item visited"))
        .collect()
}

/// Hash of the first and last 64 KiB (the whole file if it is small).
pub fn edge_hash(path: &Path, size: u64) -> std::io::Result<u128> {
    let mut f = File::open(path)?;
    if size <= 2 * EDGE {
        let mut buf = Vec::with_capacity(size as usize);
        f.read_to_end(&mut buf)?;
        return Ok(xxh3_128(&buf));
    }
    let mut buf = vec![0u8; (2 * EDGE) as usize];
    f.read_exact(&mut buf[..EDGE as usize])?;
    f.seek(SeekFrom::Start(size - EDGE))?;
    f.read_exact(&mut buf[EDGE as usize..])?;
    Ok(xxh3_128(&buf))
}

pub fn full_hash(path: &Path) -> std::io::Result<u128> {
    let mut f = File::open(path)?;
    let mut h = Xxh3::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.digest128())
}

/// Find duplicates among `(path, size)` candidates. Hard-linked copies must already be removed.
pub fn find_dupes(candidates: Vec<(PathBuf, u64)>, threads: usize) -> DupeReport {
    let mut rep = DupeReport {
        candidates: candidates.len() as u64,
        note: APFS_NOTE,
        ..Default::default()
    };
    let errors = AtomicUsize::new(0);

    // 1. same size
    let by_size: Vec<(PathBuf, u64)> =
        groups_with_dupes(candidates.into_iter().map(|(p, s)| (s, (p, s))).collect())
            .into_iter()
            .flatten()
            .collect();

    // 2. same edges
    let edge = par_map(&by_size, threads, |(p, s)| {
        edge_hash(p, *s)
            .map_err(|_| errors.fetch_add(1, Ordering::Relaxed))
            .ok()
    });
    let stage2: Vec<((u64, u128), (PathBuf, u64))> = by_size
        .into_iter()
        .zip(edge)
        .filter_map(|((p, s), h)| h.map(|h| ((s, h), (p, s))))
        .collect();
    let mut confirmed: Vec<((u64, u128), PathBuf)> = Vec::new();
    let mut needs_full: Vec<(PathBuf, u64)> = Vec::new();
    for g in groups_with_dupes(stage2.into_iter().map(|(k, v)| (k, (k, v))).collect()) {
        for ((s, h), (p, _)) in g {
            if s <= 2 * EDGE {
                confirmed.push(((s, h), p)); // edge hash already covered the whole file
            } else {
                needs_full.push((p, s));
            }
        }
    }

    // 3. full hash for big files
    rep.fully_hashed = needs_full.len() as u64;
    let full = par_map(&needs_full, threads, |(p, _)| {
        full_hash(p)
            .map_err(|_| errors.fetch_add(1, Ordering::Relaxed))
            .ok()
    });
    for ((p, s), h) in needs_full.into_iter().zip(full) {
        if let Some(h) = h {
            confirmed.push(((s, h), p));
        }
    }

    let mut groups: Vec<DupeGroup> =
        groups_with_dupes(confirmed.into_iter().map(|(k, p)| (k, (k, p))).collect())
            .into_iter()
            .map(|g| {
                let (size, hash) = g[0].0;
                let mut paths: Vec<String> = g
                    .into_iter()
                    .map(|(_, p)| p.display().to_string())
                    .collect();
                paths.sort();
                DupeGroup {
                    size,
                    hash: format!("{hash:032x}"),
                    wasted_bytes: size * (paths.len() as u64 - 1),
                    paths,
                }
            })
            .collect();
    groups.sort_by(|a, b| {
        b.wasted_bytes
            .cmp(&a.wasted_bytes)
            .then_with(|| a.paths.cmp(&b.paths))
    });
    rep.duplicate_files = groups.iter().map(|g| g.paths.len() as u64 - 1).sum();
    rep.wasted_bytes = groups.iter().map(|g| g.wasted_bytes).sum();
    rep.read_errors = errors.into_inner() as u64;
    rep.groups = groups;
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_keeps_only_real_groups() {
        let mut g = groups_with_dupes(vec![
            (1, "a"),
            (2, "b"),
            (1, "c"),
            (3, "d"),
            (3, "e"),
            (3, "f"),
        ]);
        for x in &mut g {
            x.sort();
        }
        g.sort();
        assert_eq!(g, vec![vec!["a", "c"], vec!["d", "e", "f"]]);
        assert!(groups_with_dupes::<u8, u8>(vec![]).is_empty());
    }

    #[test]
    fn par_map_keeps_order() {
        let v: Vec<u32> = (0..100).collect();
        assert_eq!(
            par_map(&v, 4, |x| x * 2),
            v.iter().map(|x| x * 2).collect::<Vec<_>>()
        );
        assert!(par_map(&Vec::<u32>::new(), 4, |x| *x).is_empty());
    }

    #[test]
    fn finds_dupes_and_ignores_same_edges() {
        let d = tempfile::tempdir().unwrap();
        let big = |fill: u8, mid: u8| {
            let mut v = vec![fill; 300 * 1024];
            v[150 * 1024] = mid; // differs only in the middle
            v
        };
        std::fs::write(d.path().join("a"), big(1, 1)).unwrap();
        std::fs::write(d.path().join("b"), big(1, 1)).unwrap();
        std::fs::write(d.path().join("c"), big(1, 9)).unwrap(); // same size & edges, different middle
        std::fs::write(d.path().join("s1"), b"small same").unwrap();
        std::fs::write(d.path().join("s2"), b"small same").unwrap();
        std::fs::write(d.path().join("s3"), b"small diff").unwrap();
        let c: Vec<(PathBuf, u64)> = ["a", "b", "c", "s1", "s2", "s3"]
            .iter()
            .map(|n| {
                let p = d.path().join(n);
                let s = std::fs::metadata(&p).unwrap().len();
                (p, s)
            })
            .collect();
        let r = find_dupes(c, 2);
        assert_eq!(r.groups.len(), 2);
        assert_eq!(r.groups[0].size, 300 * 1024);
        assert!(r.groups[0].paths[0].ends_with("/a") && r.groups[0].paths[1].ends_with("/b"));
        assert_eq!(r.groups[1].paths.len(), 2);
        assert_eq!(r.fully_hashed, 3);
        assert_eq!(r.wasted_bytes, 300 * 1024 + 10);
        assert_eq!(r.duplicate_files, 2);
    }
}
