//! The read-only walk. Each worker thread keeps its own `Tally`; tallies are merged at the end.

use crate::dupes::{self, DupeReport};
use crate::fstypes::FsInfo;
use crate::kinds::{self, Kind};
use crate::special::{Clutter, Special};
use crate::volumes;
use ignore::{DirEntry, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub depth: usize,
    pub top: usize,
    pub dupes: bool,
    pub min_dupe_size: u64,
    pub cross_filesystems: bool,
    pub threads: usize,
    pub progress: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            depth: 2,
            top: 25,
            dupes: false,
            min_dupe_size: 1 << 20,
            cross_filesystems: false,
            threads: 0,
            progress: false,
        }
    }
}

/// Files, logical bytes and allocated ("on disk") bytes.
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
pub struct Agg {
    pub files: u64,
    pub bytes: u64,
    pub allocated: u64,
}

impl Agg {
    fn add(&mut self, size: u64, alloc: u64) {
        self.files += 1;
        self.bytes += size;
        self.allocated += alloc;
    }
    fn merge(&mut self, o: &Agg) {
        self.files += o.files;
        self.bytes += o.bytes;
        self.allocated += o.allocated;
    }
}

pub const AGE_LABELS: [&str; 5] = [
    "Under 30 days",
    "30 days – 1 year",
    "1 – 3 years",
    "3 – 10 years",
    "Older than 10 years",
];

pub fn age_bucket(age_secs: u64) -> usize {
    const DAY: u64 = 86_400;
    match age_secs {
        a if a < 30 * DAY => 0,
        a if a < 365 * DAY => 1,
        a if a < 3 * 365 * DAY => 2,
        a if a < 10 * 365 * DAY => 3,
        _ => 4,
    }
}

const MAX_ERROR_SAMPLES: usize = 8;
const MAX_CLUTTER_SAMPLES: usize = 5;

/// Everything one worker has counted so far. Pure data: unit-testable without a disk.
#[derive(Default, Debug)]
pub struct Tally {
    pub totals: Agg,
    pub dirs: u64,
    pub symlinks: u64,
    pub other_entries: u64,
    pub hardlinks_skipped: u64,
    pub unreadable: u64,
    pub error_samples: Vec<String>,
    pub roots: HashMap<PathBuf, Agg>,
    /// (depth, folder) -> totals
    pub folders: HashMap<(usize, PathBuf), Agg>,
    pub kinds: [Agg; Kind::ALL.len()],
    pub exts: HashMap<(Kind, String), Agg>,
    pub largest: Vec<(u64, u64, i64, PathBuf)>,
    pub ages: [Agg; 5],
    pub special: HashMap<(Special, PathBuf), Agg>,
    pub clutter: [Agg; 4],
    pub clutter_samples: [Vec<PathBuf>; 4],
    pub obsidian_dirs: Vec<PathBuf>,
    pub dupe_candidates: Vec<(PathBuf, u64)>,
}

/// Facts about one regular file, already read from `lstat`.
pub struct FileFacts<'a> {
    pub path: &'a Path,
    pub depth: usize,
    pub size: u64,
    pub allocated: u64,
    pub mtime: i64,
}

impl Tally {
    pub fn add_file(&mut self, f: &FileFacts, opts: &ScanOptions, now: i64) {
        let (size, alloc) = (f.size, f.allocated);
        self.totals.add(size, alloc);
        // f.path.ancestors(): [file, parent, ..., root(nth depth), ...]
        if let Some(root) = f.path.ancestors().nth(f.depth) {
            self.roots
                .entry(root.to_path_buf())
                .or_default()
                .add(size, alloc);
        }
        for level in 1..=opts.depth.min(f.depth.saturating_sub(1)) {
            if let Some(folder) = f.path.ancestors().nth(f.depth - level) {
                self.folders
                    .entry((level, folder.to_path_buf()))
                    .or_default()
                    .add(size, alloc);
            }
        }
        // Special folders anywhere between the file and the scan root (outermost one wins
        // inside each category, so nested node_modules are not counted twice).
        let mut outer: [Option<&Path>; Special::COUNT] = [None; Special::COUNT];
        for anc in f.path.ancestors().skip(1).take(f.depth) {
            if let Some(cat) = anc
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(Special::of_dir)
            {
                outer[cat.index()] = Some(anc);
            }
        }
        for (i, p) in outer.iter().enumerate() {
            if let Some(p) = p {
                self.special
                    .entry((Special::ALL[i], p.to_path_buf()))
                    .or_default()
                    .add(size, alloc);
            }
        }
        let name = f
            .path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let kind = kinds::kind_of(&name);
        self.kinds[kind.index()].add(size, alloc);
        let ext = if name.starts_with("._") {
            "._*".to_string()
        } else {
            match kinds::extension(&name) {
                Some(e) if e.len() <= 12 => e,
                Some(_) => "(long)".to_string(),
                None => "(none)".to_string(),
            }
        };
        self.exts.entry((kind, ext)).or_default().add(size, alloc);
        if let Some(c) = Clutter::of_file(&name) {
            self.clutter[c.index()].add(size, alloc);
            if self.clutter_samples[c.index()].len() < MAX_CLUTTER_SAMPLES {
                self.clutter_samples[c.index()].push(f.path.to_path_buf());
            }
        }
        let age = (now - f.mtime).max(0) as u64;
        self.ages[age_bucket(age)].add(size, alloc);
        if opts.top > 0 {
            self.largest
                .push((size, alloc, f.mtime, f.path.to_path_buf()));
            if self.largest.len() > opts.top * 4 {
                trim_largest(&mut self.largest, opts.top);
            }
        }
        if opts.dupes && size >= opts.min_dupe_size.max(1) {
            self.dupe_candidates.push((f.path.to_path_buf(), size));
        }
    }

    pub fn add_dir(&mut self, path: &Path, depth: usize) {
        self.dirs += 1;
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        if name == ".obsidian" {
            self.obsidian_dirs.push(path.to_path_buf());
        }
        if let Some(cat) = Special::of_dir(name) {
            // Register the folder even if it is empty, unless an outer folder of the same kind holds it.
            let nested = path.ancestors().skip(1).take(depth).any(|a| {
                a.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(Special::of_dir)
                    == Some(cat)
            });
            if !nested {
                self.special.entry((cat, path.to_path_buf())).or_default();
            }
        }
    }

    pub fn add_error(&mut self, msg: String) {
        self.unreadable += 1;
        if self.error_samples.len() < MAX_ERROR_SAMPLES {
            self.error_samples.push(msg);
        }
    }

    pub fn merge(&mut self, o: Tally) {
        self.totals.merge(&o.totals);
        self.dirs += o.dirs;
        self.symlinks += o.symlinks;
        self.other_entries += o.other_entries;
        self.hardlinks_skipped += o.hardlinks_skipped;
        self.unreadable += o.unreadable;
        for e in o.error_samples {
            if self.error_samples.len() < MAX_ERROR_SAMPLES {
                self.error_samples.push(e);
            }
        }
        for (k, v) in o.roots {
            self.roots.entry(k).or_default().merge(&v);
        }
        for (k, v) in o.folders {
            self.folders.entry(k).or_default().merge(&v);
        }
        for i in 0..self.kinds.len() {
            self.kinds[i].merge(&o.kinds[i]);
        }
        for (k, v) in o.exts {
            self.exts.entry(k).or_default().merge(&v);
        }
        self.largest.extend(o.largest);
        for i in 0..self.ages.len() {
            self.ages[i].merge(&o.ages[i]);
        }
        for (k, v) in o.special {
            self.special.entry(k).or_default().merge(&v);
        }
        for (i, samples) in o.clutter_samples.into_iter().enumerate() {
            self.clutter[i].merge(&o.clutter[i]);
            for s in samples {
                if self.clutter_samples[i].len() < MAX_CLUTTER_SAMPLES {
                    self.clutter_samples[i].push(s);
                }
            }
        }
        self.obsidian_dirs.extend(o.obsidian_dirs);
        self.dupe_candidates.extend(o.dupe_candidates);
    }
}

fn trim_largest(v: &mut Vec<(u64, u64, i64, PathBuf)>, top: usize) {
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.3.cmp(&b.3)));
    v.truncate(top);
}

// ---------------------------------------------------------------------------------------
// Report types (what goes into JSON / HTML / terminal)

#[derive(Debug, Serialize)]
pub struct RootInfo {
    pub path: String,
    pub totals: Agg,
    pub mount_point: Option<String>,
    pub fs_type: Option<String>,
    pub volume_total_bytes: Option<u64>,
    pub volume_used_bytes: Option<u64>,
    pub volume_free_bytes: Option<u64>,
    pub explain: Option<FsInfo>,
}

#[derive(Debug, Serialize)]
pub struct Totals {
    pub files: u64,
    pub folders: u64,
    pub symlinks: u64,
    pub other_entries: u64,
    pub bytes: u64,
    pub allocated_bytes: u64,
    pub hardlinks_counted_once: u64,
    pub unreadable: u64,
    pub error_samples: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct FolderStat {
    pub path: String,
    pub files: u64,
    pub bytes: u64,
    pub allocated_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct LevelList {
    pub level: usize,
    pub folders: Vec<FolderStat>,
}

#[derive(Debug, Serialize)]
pub struct ExtStat {
    pub ext: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct KindStat {
    pub kind: Kind,
    pub label: &'static str,
    pub files: u64,
    pub bytes: u64,
    pub allocated_bytes: u64,
    pub top_extensions: Vec<ExtStat>,
}

#[derive(Debug, Serialize)]
pub struct FileStat {
    pub path: String,
    pub bytes: u64,
    pub allocated_bytes: u64,
    pub modified: i64,
}

#[derive(Debug, Serialize)]
pub struct AgeStat {
    pub label: &'static str,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct SpecialStat {
    pub category: Special,
    pub label: &'static str,
    pub explain: &'static str,
    pub folders: u64,
    pub files: u64,
    pub bytes: u64,
    pub allocated_bytes: u64,
    pub biggest: Vec<FolderStat>,
}

#[derive(Debug, Serialize)]
pub struct VaultStat {
    pub path: String,
    pub files: u64,
    pub notes: u64,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct ClutterStat {
    pub kind: Clutter,
    pub label: &'static str,
    pub explain: &'static str,
    pub files: u64,
    pub bytes: u64,
    pub allocated_bytes: u64,
    pub samples: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Settings {
    pub depth: usize,
    pub top: usize,
    pub dupes: bool,
    pub min_dupe_size: u64,
    pub cross_filesystems: bool,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub tool: &'static str,
    pub version: &'static str,
    pub generated_at: i64,
    pub elapsed_secs: f64,
    pub settings: Settings,
    pub roots: Vec<RootInfo>,
    pub totals: Totals,
    pub biggest_folders: Vec<LevelList>,
    pub kinds: Vec<KindStat>,
    pub largest_files: Vec<FileStat>,
    pub ages: Vec<AgeStat>,
    pub special_folders: Vec<SpecialStat>,
    pub obsidian_vaults: Vec<VaultStat>,
    pub clutter: Vec<ClutterStat>,
    pub duplicates: Option<DupeReport>,
}

// ---------------------------------------------------------------------------------------
// The walk

struct Shared {
    opts: ScanOptions,
    now: i64,
    seen_links: Mutex<HashSet<(u64, u64)>>,
    done: Mutex<Vec<Tally>>,
    files_so_far: AtomicU64,
    bytes_so_far: AtomicU64,
}

struct Builder<'a> {
    shared: &'a Shared,
}

struct Visitor<'a> {
    shared: &'a Shared,
    tally: Tally,
}

impl<'s> ParallelVisitorBuilder<'s> for Builder<'s> {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 's> {
        Box::new(Visitor {
            shared: self.shared,
            tally: Tally::default(),
        })
    }
}

impl Drop for Visitor<'_> {
    fn drop(&mut self) {
        let t = std::mem::take(&mut self.tally);
        if let Ok(mut d) = self.shared.done.lock() {
            d.push(t);
        }
    }
}

impl ParallelVisitor for Visitor<'_> {
    fn visit(&mut self, entry: Result<DirEntry, ignore::Error>) -> WalkState {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                self.tally.add_error(e.to_string());
                return WalkState::Continue;
            }
        };
        let Some(ft) = entry.file_type() else {
            self.tally.other_entries += 1;
            return WalkState::Continue;
        };
        if ft.is_symlink() {
            self.tally.symlinks += 1;
            return WalkState::Continue;
        }
        if ft.is_dir() {
            self.tally.add_dir(entry.path(), entry.depth());
            return WalkState::Continue;
        }
        if !ft.is_file() {
            self.tally.other_entries += 1;
            return WalkState::Continue;
        }
        let md = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                self.tally
                    .add_error(format!("{}: {e}", entry.path().display()));
                return WalkState::Continue;
            }
        };
        if md.nlink() > 1 {
            let key = (md.dev(), md.ino());
            let first = self
                .shared
                .seen_links
                .lock()
                .map(|mut s| s.insert(key))
                .unwrap_or(true);
            if !first {
                self.tally.hardlinks_skipped += 1;
                return WalkState::Continue;
            }
        }
        let facts = FileFacts {
            path: entry.path(),
            depth: entry.depth(),
            size: md.len(),
            allocated: md.blocks() * 512,
            mtime: md.mtime(),
        };
        self.tally
            .add_file(&facts, &self.shared.opts, self.shared.now);
        self.shared.files_so_far.fetch_add(1, Ordering::Relaxed);
        self.shared
            .bytes_so_far
            .fetch_add(facts.size, Ordering::Relaxed);
        WalkState::Continue
    }
}

fn walker(roots: &[PathBuf], opts: &ScanOptions) -> WalkBuilder {
    let mut wb = WalkBuilder::new(&roots[0]);
    for r in &roots[1..] {
        wb.add(r);
    }
    wb.standard_filters(false)
        .hidden(false)
        .follow_links(false)
        .same_file_system(!opts.cross_filesystems)
        .threads(opts.threads);
    wb
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn is_stderr_tty() -> bool {
    // SAFETY: isatty only inspects the file descriptor.
    unsafe { libc::isatty(2) == 1 }
}

/// Scan the given roots (read-only) and build a report.
pub fn scan(paths: &[PathBuf], opts: &ScanOptions) -> Result<Report, String> {
    let start = Instant::now();
    let mut roots: Vec<PathBuf> = Vec::new();
    for p in paths {
        let c =
            std::fs::canonicalize(p).map_err(|e| format!("cannot open {}: {e}", p.display()))?;
        if !roots.contains(&c) {
            roots.push(c);
        }
    }
    if roots.is_empty() {
        return Err("no paths to scan".into());
    }
    let shared = Shared {
        opts: opts.clone(),
        now: now_secs(),
        seen_links: Mutex::new(HashSet::new()),
        done: Mutex::new(Vec::new()),
        files_so_far: AtomicU64::new(0),
        bytes_so_far: AtomicU64::new(0),
    };
    let finished = Arc::new(AtomicBool::new(false));
    std::thread::scope(|s| {
        if opts.progress && is_stderr_tty() {
            let (shared, finished) = (&shared, finished.clone());
            s.spawn(move || {
                while !finished.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(250));
                    eprint!(
                        "\r  scanning… {} files, {}   ",
                        crate::util::thousands(shared.files_so_far.load(Ordering::Relaxed)),
                        crate::util::human(shared.bytes_so_far.load(Ordering::Relaxed))
                    );
                }
                eprint!("\r{}\r", " ".repeat(60));
            });
        }
        walker(&roots, opts)
            .build_parallel()
            .visit(&mut Builder { shared: &shared });
        finished.store(true, Ordering::Relaxed);
    });
    let mut tally = Tally::default();
    for t in shared.done.into_inner().unwrap_or_default() {
        tally.merge(t);
    }
    let dupes = if opts.dupes {
        let cands = std::mem::take(&mut tally.dupe_candidates);
        Some(dupes::find_dupes(cands, opts.threads))
    } else {
        None
    };
    let vols = volumes::list(true).unwrap_or_default();
    Ok(build_report(
        tally,
        &roots,
        opts,
        &vols,
        dupes,
        start.elapsed().as_secs_f64(),
        shared.now,
    ))
}

fn folder_stat(p: &Path, a: &Agg) -> FolderStat {
    FolderStat {
        path: p.display().to_string(),
        files: a.files,
        bytes: a.bytes,
        allocated_bytes: a.allocated,
    }
}

/// Turn merged tallies into a sorted report. Pure: no disk access except for Obsidian vault sizing.
pub fn build_report(
    mut t: Tally,
    roots: &[PathBuf],
    opts: &ScanOptions,
    vols: &[volumes::Volume],
    duplicates: Option<DupeReport>,
    elapsed_secs: f64,
    now: i64,
) -> Report {
    let top = opts.top;
    let roots_info = roots
        .iter()
        .map(|r| {
            let v = volumes::volume_for(r, vols);
            RootInfo {
                path: r.display().to_string(),
                totals: t.roots.get(r).copied().unwrap_or_default(),
                mount_point: v.map(|v| v.mount_point.clone()),
                fs_type: v.map(|v| v.fs_type.clone()),
                volume_total_bytes: v.map(|v| v.total_bytes),
                volume_used_bytes: v.map(|v| v.used_bytes),
                volume_free_bytes: v.map(|v| v.free_bytes),
                explain: v.map(|v| v.explain.clone()),
            }
        })
        .collect();

    let mut levels: Vec<LevelList> = (1..=opts.depth)
        .map(|level| LevelList {
            level,
            folders: Vec::new(),
        })
        .collect();
    for ((level, p), a) in &t.folders {
        levels[level - 1].folders.push(folder_stat(p, a));
    }
    for l in &mut levels {
        l.folders
            .sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
        l.folders.truncate(top);
    }

    let kinds = Kind::ALL
        .iter()
        .map(|&k| {
            let a = t.kinds[k.index()];
            let mut exts: Vec<ExtStat> = t
                .exts
                .iter()
                .filter(|((kk, _), _)| *kk == k)
                .map(|((_, e), a)| ExtStat {
                    ext: e.clone(),
                    files: a.files,
                    bytes: a.bytes,
                })
                .collect();
            exts.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.ext.cmp(&b.ext)));
            exts.truncate(6);
            KindStat {
                kind: k,
                label: k.label(),
                files: a.files,
                bytes: a.bytes,
                allocated_bytes: a.allocated,
                top_extensions: exts,
            }
        })
        .filter(|k| k.files > 0)
        .collect::<Vec<_>>();
    let mut kinds = kinds;
    kinds.sort_by_key(|k| std::cmp::Reverse(k.bytes));

    trim_largest(&mut t.largest, top);
    let largest_files = t
        .largest
        .iter()
        .map(|(s, a, m, p)| FileStat {
            path: p.display().to_string(),
            bytes: *s,
            allocated_bytes: *a,
            modified: *m,
        })
        .collect();

    let ages = AGE_LABELS
        .iter()
        .enumerate()
        .map(|(i, l)| AgeStat {
            label: l,
            files: t.ages[i].files,
            bytes: t.ages[i].bytes,
        })
        .collect();

    let mut special_folders: Vec<SpecialStat> = Special::ALL
        .iter()
        .filter_map(|&cat| {
            let mut inst: Vec<FolderStat> = t
                .special
                .iter()
                .filter(|((c, _), _)| *c == cat)
                .map(|((_, p), a)| folder_stat(p, a))
                .collect();
            if inst.is_empty() {
                return None;
            }
            let sum = inst.iter().fold(Agg::default(), |mut acc, f| {
                acc.files += f.files;
                acc.bytes += f.bytes;
                acc.allocated += f.allocated_bytes;
                acc
            });
            let n = inst.len() as u64;
            inst.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
            inst.truncate(10);
            Some(SpecialStat {
                category: cat,
                label: cat.label(),
                explain: cat.explain(),
                folders: n,
                files: sum.files,
                bytes: sum.bytes,
                allocated_bytes: sum.allocated,
                biggest: inst,
            })
        })
        .collect();
    special_folders.sort_by_key(|s| std::cmp::Reverse(s.bytes));

    t.obsidian_dirs.sort();
    let obsidian_vaults = t
        .obsidian_dirs
        .iter()
        .filter_map(|d| d.parent())
        .map(|v| vault_stat(v, opts))
        .collect();

    let clutter = Clutter::ALL
        .iter()
        .filter(|c| t.clutter[c.index()].files > 0)
        .map(|&c| {
            let a = t.clutter[c.index()];
            ClutterStat {
                kind: c,
                label: c.label(),
                explain: c.explain(),
                files: a.files,
                bytes: a.bytes,
                allocated_bytes: a.allocated,
                samples: t.clutter_samples[c.index()]
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect(),
            }
        })
        .collect();

    Report {
        tool: "fsmap",
        version: env!("CARGO_PKG_VERSION"),
        generated_at: now,
        elapsed_secs,
        settings: Settings {
            depth: opts.depth,
            top: opts.top,
            dupes: opts.dupes,
            min_dupe_size: opts.min_dupe_size,
            cross_filesystems: opts.cross_filesystems,
        },
        roots: roots_info,
        totals: Totals {
            files: t.totals.files,
            folders: t.dirs,
            symlinks: t.symlinks,
            other_entries: t.other_entries,
            bytes: t.totals.bytes,
            allocated_bytes: t.totals.allocated,
            hardlinks_counted_once: t.hardlinks_skipped,
            unreadable: t.unreadable,
            error_samples: t.error_samples,
        },
        biggest_folders: levels,
        kinds,
        largest_files,
        ages,
        special_folders,
        obsidian_vaults,
        clutter,
        duplicates,
    }
}

/// Size of an Obsidian vault (the folder holding `.obsidian`). A small extra read-only walk.
fn vault_stat(vault: &Path, opts: &ScanOptions) -> VaultStat {
    let mut v = VaultStat {
        path: vault.display().to_string(),
        files: 0,
        notes: 0,
        bytes: 0,
    };
    let mut wb = WalkBuilder::new(vault);
    wb.standard_filters(false)
        .hidden(false)
        .follow_links(false)
        .same_file_system(!opts.cross_filesystems);
    for e in wb.build().flatten() {
        if e.file_type().is_some_and(|t| t.is_file()) {
            v.files += 1;
            if let Ok(m) = e.metadata() {
                v.bytes += m.len();
            }
            if e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("md"))
            {
                v.notes += 1;
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(t: &mut Tally, path: &str, depth: usize, size: u64, opts: &ScanOptions) {
        t.add_file(
            &FileFacts {
                path: Path::new(path),
                depth,
                size,
                allocated: size.div_ceil(4096) * 4096,
                mtime: 0,
            },
            opts,
            40 * 86_400,
        );
    }

    #[test]
    fn aggregation() {
        let opts = ScanOptions {
            depth: 2,
            top: 2,
            dupes: true,
            min_dupe_size: 100,
            ..Default::default()
        };
        let mut a = Tally::default();
        let mut b = Tally::default();
        file(&mut a, "/r/photos/2024/a.jpg", 3, 1000, &opts);
        file(&mut a, "/r/photos/b.MOV", 2, 5000, &opts);
        file(
            &mut b,
            "/r/code/node_modules/x/node_modules/y/i.js",
            6,
            50,
            &opts,
        );
        file(&mut b, "/r/top.txt", 1, 10, &opts);
        file(&mut b, "/r/._top.txt", 1, 4096, &opts);
        b.add_dir(Path::new("/r/code/node_modules"), 2);
        b.add_dir(Path::new("/r/code/node_modules/x/node_modules"), 4);
        b.add_dir(Path::new("/r/empty/.git"), 2);
        a.merge(b);
        let r = build_report(
            a,
            &[PathBuf::from("/r")],
            &opts,
            &[],
            None,
            0.0,
            40 * 86_400,
        );

        assert_eq!(r.totals.files, 5);
        assert_eq!(r.totals.bytes, 1000 + 5000 + 50 + 10 + 4096);
        assert_eq!(r.totals.folders, 3);
        assert_eq!(r.roots[0].totals.bytes, r.totals.bytes);
        // level 1: photos (6000), code (50); files directly in /r are not in any level-1 folder
        let l1 = &r.biggest_folders[0].folders;
        assert_eq!(l1[0].path, "/r/photos");
        assert_eq!(l1[0].bytes, 6000);
        assert_eq!(l1.len(), 2);
        let l2 = &r.biggest_folders[1].folders;
        assert_eq!(l2[0].path, "/r/photos/2024");
        assert_eq!(l2[1].path, "/r/code/node_modules");
        // nested node_modules counted once, in the outermost folder
        let nm = r
            .special_folders
            .iter()
            .find(|s| s.category == Special::NodeModules)
            .unwrap();
        assert_eq!(nm.folders, 1);
        assert_eq!(nm.bytes, 50);
        // empty .git still listed
        assert!(r
            .special_folders
            .iter()
            .any(|s| s.category == Special::Git && s.folders == 1 && s.bytes == 0));
        // kinds
        let video = r.kinds.iter().find(|k| k.kind == Kind::Video).unwrap();
        assert_eq!((video.files, video.bytes), (1, 5000));
        assert_eq!(video.top_extensions[0].ext, "mov");
        // largest files (top 2)
        assert_eq!(r.largest_files.len(), 2);
        assert_eq!(r.largest_files[0].bytes, 5000);
        assert_eq!(r.largest_files[1].bytes, 4096);
        // ages: mtime 0, now = 40 days -> "30 days – 1 year"
        assert_eq!(r.ages[1].files, 5);
        // clutter
        assert_eq!(r.clutter.len(), 1);
        assert_eq!(r.clutter[0].kind, Clutter::AppleDouble);
    }

    #[test]
    fn dupe_candidates_respect_min_size() {
        let opts = ScanOptions {
            dupes: true,
            min_dupe_size: 1000,
            ..Default::default()
        };
        let mut t = Tally::default();
        file(&mut t, "/r/a", 1, 999, &opts);
        file(&mut t, "/r/b", 1, 1000, &opts);
        assert_eq!(t.dupe_candidates.len(), 1);
    }

    #[test]
    fn ages() {
        assert_eq!(age_bucket(0), 0);
        assert_eq!(age_bucket(31 * 86_400), 1);
        assert_eq!(age_bucket(2 * 365 * 86_400), 2);
        assert_eq!(age_bucket(5 * 365 * 86_400), 3);
        assert_eq!(age_bucket(20 * 365 * 86_400), 4);
    }
}
