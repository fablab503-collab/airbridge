# fsmap — design note

`fsmap` answers two questions for Daniel's drives: **"what kind of disk is this?"**
(`fsmap volumes`) and **"what is filling it up?"** (`fsmap scan`). It must be safe to
point at irreplaceable data (SecondBrain vault, NAS volumes, old hard drives), so the
first rule is: **it only ever reads**. The only files it writes are the `--json` /
`--html` reports the user names.

## Options considered

| Option | Good | Bad |
|---|---|---|
| A. Shell script around `du`, `df`, `find`, `mount` | nothing to build | `du`/`find` flags differ between macOS (BSD) and Linux; no file kinds, no duplicates, no HTML; slow single-threaded walk on a 4 TB disk; quoting bugs with odd file names |
| B. Python script | quick to write | needs a Python on every Mac; slow on millions of files; no static binary to copy onto a USB stick |
| C. Existing tools (`ncdu`, `dust`, `dua`, `fdupes`, `czkawka`) | mature | each does one slice; must be installed via Homebrew; none explains filesystem types in plain words or knows Synology/Time Machine/Obsidian folders; some can delete files (we do not want a delete button anywhere near the NAS) |
| **D. Small Rust crate (chosen)** | one static binary, cross-compiles to both Mac CPUs; parallel walk via the `ignore` crate (the engine behind ripgrep); one pass collects everything; can be made read-only by construction | more code to write and test |

## Why D wins

* **Read-only by construction.** The code never calls remove/rename/write on scanned
  paths. Files are opened read-only, and only when `--dupes` needs to hash them.
  (Reading a file can update its "last accessed" time on some systems; that is the only
  side effect, and it does not change any content.)
* **One pass, many answers.** Every file is looked at once (`lstat`), and each worker
  thread keeps its own tallies (totals, folder sizes, kinds, ages, special folders,
  clutter, largest files). Tallies are merged at the end, so threads never fight over a
  lock — except a tiny shared set used only for files with more than one hard link.
* **Correct numbers.** Hard links are counted once by `(device, inode)`. Symlinks are
  counted but never followed, so loops cannot trap the walk. By default the walk stays
  on one filesystem, so scanning `/` does not wander into `/Volumes/…` or a NAS mount.
  Both the "logical" size (what Finder calls size) and the "allocated" size
  (`st_blocks × 512`, what Finder calls "on disk") are kept.
* **Cheap duplicate finding.** Size first (free), then a hash of the first and last
  64 KiB, then the full file — so most files are never read in full. xxh3-128 is fast
  and has negligible accidental-collision risk for this job (it is not a security hash,
  and it does not need to be).
* **Portable.** Pure Rust plus `libc`; macOS uses `getmntinfo`, Linux reads
  `/proc/self/mountinfo` + `statvfs`. No C toolchain needed for cross-checks.

## Known limits (stated in the reports too)

* APFS clones and snapshots: two "duplicate" files may share the same blocks, so
  deleting one may free nothing. `used` on an APFS volume can also be smaller than the
  sum of its files.
* Python venvs are detected by folder name (`.venv`, `venv`, `virtualenv`) plus
  `__pycache__`; a venv with an unusual name is counted as normal files.
* Network shares report whatever size the server returns; "on disk" on SMB is a guess
  by the server.
