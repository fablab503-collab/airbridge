//! List mounted filesystems (read-only: only asks the OS, never touches the disks).

use crate::fstypes::{self, FsInfo};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
pub struct Volume {
    pub mount_point: String,
    pub device: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub read_only: bool,
    /// True for system / pseudo volumes that are hidden unless `--all`.
    pub system: bool,
    pub explain: FsInfo,
}

/// One parsed line of `/proc/self/mountinfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountEntry {
    pub mount_point: String,
    pub device: String,
    pub fs_type: String,
    pub read_only: bool,
}

/// Decode the octal escapes the kernel uses in mountinfo (`\040` = space, `\011` = tab,
/// `\012` = newline, `\134` = backslash).
pub fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            let oct = &b[i + 1..(i + 4).min(b.len())];
            if oct.len() == 3 && oct.iter().all(|c| (b'0'..=b'7').contains(c)) {
                let v = (oct[0] - b'0') as u32 * 64
                    + (oct[1] - b'0') as u32 * 8
                    + (oct[2] - b'0') as u32;
                if v <= 255 {
                    out.push(v as u8);
                    i += 4;
                    continue;
                }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse the text of `/proc/self/mountinfo`.
pub fn parse_mountinfo(text: &str) -> Vec<MountEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        let Some(sep) = fields.iter().position(|f| *f == "-") else {
            continue;
        };
        if fields.len() < 6 || fields.len() < sep + 3 {
            continue;
        }
        let mount_opts = fields[5];
        let super_opts = fields.get(sep + 3).copied().unwrap_or("");
        let ro = |opts: &str| opts.split(',').any(|o| o == "ro");
        out.push(MountEntry {
            mount_point: unescape_mount(fields[4]),
            fs_type: unescape_mount(fields[sep + 1]),
            device: unescape_mount(fields[sep + 2]),
            read_only: ro(mount_opts) || ro(super_opts),
        });
    }
    out
}

/// List volumes. Without `all`, system / pseudo volumes are left out.
pub fn list(all: bool) -> std::io::Result<Vec<Volume>> {
    let mut v = list_os()?;
    if !all {
        v.retain(|x| !x.system);
    }
    v.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    Ok(v)
}

/// The volume whose mount point is the longest prefix of `path`.
pub fn volume_for<'a>(path: &Path, vols: &'a [Volume]) -> Option<&'a Volume> {
    vols.iter()
        .filter(|v| path.starts_with(&v.mount_point))
        .max_by_key(|v| v.mount_point.len())
}

#[cfg(target_os = "linux")]
fn list_os() -> std::io::Result<Vec<Volume>> {
    let text = std::fs::read_to_string("/proc/self/mountinfo")?;
    Ok(parse_mountinfo(&text)
        .into_iter()
        .map(|m| {
            let (total, used, free, ro_flag) = statvfs(&m.mount_point).unwrap_or((0, 0, 0, false));
            Volume {
                system: fstypes::is_pseudo(&m.fs_type),
                explain: fstypes::explain(&m.fs_type),
                read_only: m.read_only || ro_flag,
                total_bytes: total,
                used_bytes: used,
                free_bytes: free,
                mount_point: m.mount_point,
                device: m.device,
                fs_type: m.fs_type,
            }
        })
        .collect())
}

#[cfg(target_os = "linux")]
fn statvfs(path: &str) -> Option<(u64, u64, u64, bool)> {
    use std::ffi::CString;
    let c = CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid C string and `s` is a valid out-pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let f = s.f_frsize as u64;
    let total = s.f_blocks as u64 * f;
    let used = (s.f_blocks as u64).saturating_sub(s.f_bfree as u64) * f;
    let free = s.f_bavail as u64 * f;
    Some((total, used, free, (s.f_flag & libc::ST_RDONLY) != 0))
}

#[cfg(target_os = "macos")]
fn list_os() -> std::io::Result<Vec<Volume>> {
    use std::ffi::CStr;
    const MNT_NOWAIT: libc::c_int = 2;
    const MNT_RDONLY: u32 = 0x0000_0001;
    const MNT_DONTBROWSE: u32 = 0x0010_0000;

    fn cstr(buf: &[libc::c_char]) -> String {
        // SAFETY: the kernel NUL-terminates these fixed-size buffers.
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    let mut buf: *mut libc::statfs = std::ptr::null_mut();
    // SAFETY: getmntinfo fills `buf` with a pointer to an array it owns (valid until the next call).
    let n = unsafe { libc::getmntinfo(&mut buf, MNT_NOWAIT) };
    if n <= 0 || buf.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `n` entries were returned.
    let mounts = unsafe { std::slice::from_raw_parts(buf, n as usize) };
    Ok(mounts
        .iter()
        .map(|s| {
            let fs_type = cstr(&s.f_fstypename);
            let bsize = s.f_bsize as u64;
            let flags = s.f_flags;
            Volume {
                mount_point: cstr(&s.f_mntonname),
                device: cstr(&s.f_mntfromname),
                total_bytes: s.f_blocks * bsize,
                used_bytes: s.f_blocks.saturating_sub(s.f_bfree) * bsize,
                free_bytes: s.f_bavail * bsize,
                read_only: flags & MNT_RDONLY != 0,
                system: flags & MNT_DONTBROWSE != 0 || fstypes::is_pseudo(&fs_type),
                explain: fstypes::explain(&fs_type),
                fs_type,
            }
        })
        .collect())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn list_os() -> std::io::Result<Vec<Volume>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "fsmap volumes supports macOS and Linux",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
22 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw,errors=remount-ro
25 22 0:22 / /proc rw,nosuid,nodev,noexec,relatime shared:12 - proc proc rw
40 22 0:40 / /media/daniel/My\\040Passport rw,nosuid,nodev,relatime shared:200 - exfat /dev/sdb1 rw,fmask=0022
41 22 0:41 / /mnt/nas ro,relatime - cifs //synology/Volume1 rw,vers=3.0
42 22 7:0 / /snap/core/1 ro,nodev,relatime shared:30 master:2 - squashfs /dev/loop0 ro
bad line without separator
";

    #[test]
    fn mountinfo_parsing() {
        let m = parse_mountinfo(SAMPLE);
        assert_eq!(m.len(), 5);
        assert_eq!(
            m[0],
            MountEntry {
                mount_point: "/".into(),
                device: "/dev/sda2".into(),
                fs_type: "ext4".into(),
                read_only: false
            }
        );
        assert_eq!(m[1].fs_type, "proc");
        assert_eq!(m[2].mount_point, "/media/daniel/My Passport");
        assert_eq!(m[2].fs_type, "exfat");
        assert!(m[3].read_only);
        assert_eq!(m[3].device, "//synology/Volume1");
        assert_eq!(m[4].fs_type, "squashfs");
        assert!(
            m[4].read_only,
            "optional fields (master:2) must not shift columns"
        );
    }

    #[test]
    fn unescape() {
        assert_eq!(unescape_mount("a\\040b"), "a b");
        assert_eq!(unescape_mount("tab\\011x\\134y"), "tab\tx\\y");
        assert_eq!(unescape_mount("trailing\\04"), "trailing\\04");
        assert_eq!(unescape_mount("plain"), "plain");
    }

    #[test]
    fn longest_prefix() {
        let mk = |p: &str| Volume {
            mount_point: p.into(),
            device: String::new(),
            fs_type: "ext4".into(),
            total_bytes: 0,
            used_bytes: 0,
            free_bytes: 0,
            read_only: false,
            system: false,
            explain: fstypes::explain("ext4"),
        };
        let vols = vec![mk("/"), mk("/Volumes/Volume1"), mk("/Volumes")];
        assert_eq!(
            volume_for(Path::new("/Volumes/Volume1/SecondBrain"), &vols)
                .unwrap()
                .mount_point,
            "/Volumes/Volume1"
        );
        assert_eq!(
            volume_for(Path::new("/Volumes/Volume10"), &vols)
                .unwrap()
                .mount_point,
            "/Volumes"
        );
        assert_eq!(
            volume_for(Path::new("/usr"), &vols).unwrap().mount_point,
            "/"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn lists_real_mounts() {
        let all = list(true).unwrap();
        assert!(all.iter().any(|v| v.mount_point == "/"));
    }
}
