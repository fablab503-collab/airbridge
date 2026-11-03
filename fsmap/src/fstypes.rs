//! Plain-words explanations of filesystem types, with practical notes.

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FsInfo {
    /// Short family name, e.g. "APFS" or "SMB network share".
    pub family: &'static str,
    /// One plain sentence: what this is.
    pub plain: &'static str,
    /// Practical things to know.
    pub notes: Vec<&'static str>,
}

fn info(family: &'static str, plain: &'static str, notes: &[&'static str]) -> FsInfo {
    FsInfo {
        family,
        plain,
        notes: notes.to_vec(),
    }
}

/// Explain a filesystem type name as reported by macOS (`f_fstypename`) or Linux (mountinfo).
pub fn explain(fstype: &str) -> FsInfo {
    let t = fstype.to_ascii_lowercase();
    match t.as_str() {
        "apfs" => info("APFS", "Apple's modern filesystem, used by every Mac since 2017 (internal SSDs and most new external disks).", &[
            "Copies made in Finder are often 'clones': both files share the same blocks until one is changed, so duplicates may not use extra space.",
            "Snapshots (Time Machine, system updates) keep old blocks alive, so freeing files may not free space right away.",
            "'Used' can be less than the sum of all file sizes because of clones and sparse files.",
            "Several volumes share one container, so they all show the same total size and free space.",
        ]),
        "hfs" | "hfsplus" => info("HFS+ (Mac OS Extended)", "Apple's older filesystem; common on spinning hard drives and older Time Machine disks.", &[
            "Journaled, so it survives a sudden unplug better than exFAT, but still eject first.",
            "Only Macs read and write it natively; Windows needs extra software.",
            "Fine for hard drives; APFS is better for SSDs.",
        ]),
        "exfat" => info("exFAT", "A simple filesystem that Mac, Windows, Linux, cameras and TVs can all read and write — popular on USB sticks and SD cards.", &[
            "No journal: always eject before unplugging, or the disk can be damaged.",
            "Macs leave '._' files and '.DS_Store' files on it (extra metadata). They are small and safe to ignore.",
            "No file permissions and no hard links.",
        ]),
        "msdos" | "vfat" | "fat" | "fat32" | "fat16" | "fat12" => info("FAT32 (MS-DOS)", "A very old, very compatible filesystem; common on small USB sticks, SD cards and EFI boot partitions.", &[
            "A single file cannot be bigger than 4 GB — big videos or disk images will not fit.",
            "No journal: always eject before unplugging.",
            "Macs leave '._' and '.DS_Store' files on it.",
        ]),
        "ntfs" | "ntfs3" | "ntfs-3g" => info("NTFS (Windows)", "Windows' main filesystem.", &[
            "A Mac can read NTFS but cannot write to it without extra software.",
            "To share a disk between Mac and Windows, exFAT is the easy choice.",
        ]),
        "smbfs" | "cifs" | "smb3" | "smb2" => info("SMB network share", "A folder on another computer or a NAS (such as the Synology), reached over the network.", &[
            "Speed depends on Wi-Fi/Ethernet; a big scan can take much longer than on a local disk.",
            "'@eaDir' folders are Synology's thumbnails and indexes; '#recycle' is the share's recycle bin (deleted files still use space there).",
            "Total/free space is what the NAS reports for the whole volume, not just this share.",
            "'On disk' sizes are the server's estimate.",
        ]),
        "afpfs" => info("AFP network share", "Apple's old file-sharing protocol (Apple Filing Protocol), used by older NAS boxes and Time Capsules.", &[
            "Apple has deprecated AFP; SMB is preferred today.",
            "Synology can serve AFP too; '@eaDir' and '#recycle' have the same meaning as on SMB.",
        ]),
        "nfs" | "nfs4" => info("NFS network share", "A Unix-style network filesystem, common between Linux servers and NAS boxes.", &[
            "File owners are numbers (user IDs); they may not match the names on this computer.",
            "Speed depends on the network.",
        ]),
        "webdav" | "davfs" | "fuse.davfs2" => info("WebDAV share", "Files reached over the web (HTTP), for example a cloud drive or a NAS web share.", &[
            "Usually slow for scanning many small files.",
            "Sizes and free space may be estimates from the server.",
        ]),
        "btrfs" => info("Btrfs", "A modern Linux filesystem with snapshots and copy-on-write; Synology uses it on many NAS volumes.", &[
            "Snapshots keep old data, so deleting files may not free space until old snapshots are removed.",
            "Copies can share blocks (like APFS clones).",
        ]),
        "ext4" | "ext3" | "ext2" => info("ext4 (Linux)", "The standard Linux filesystem; also used by some NAS volumes.", &[
            "A Mac cannot read it without extra software.",
            "Reserves about 5% of space for the system, so 'free' may look smaller than total minus used.",
        ]),
        "xfs" => info("XFS (Linux)", "A fast Linux filesystem for big files and big disks.", &[
            "A Mac cannot read it without extra software.",
        ]),
        "zfs" => info("ZFS", "A powerful filesystem with snapshots, checksums and compression (TrueNAS, some Linux and Mac setups).", &[
            "Compression means 'on disk' can be much smaller than file sizes.",
            "Snapshots keep old data alive.",
        ]),
        "ufs" => info("UFS", "The classic Unix File System (BSD); rare on modern Macs.", &[]),
        "iso9660" | "cd9660" => info("CD/DVD (ISO 9660)", "The filesystem of CDs, DVDs and many .iso disk images.", &[
            "Read-only.",
        ]),
        "udf" => info("UDF", "The filesystem of DVDs and Blu-ray discs, sometimes used on USB disks.", &[
            "Often read-only on disc media.",
        ]),
        "tmpfs" | "devtmpfs" | "ramfs" => info("RAM disk (tmpfs)", "A filesystem that lives in memory; its contents vanish at restart.", &[
            "Uses RAM, not disk space.",
        ]),
        "overlay" | "overlayfs" | "aufs" => info("Overlay (containers)", "Layers stacked on top of each other — used by Docker and other containers.", &[
            "What you see is the top layer plus the read-only layers below it.",
        ]),
        "devfs" => info("devfs", "A special system folder listing hardware devices; not real files.", &[]),
        "autofs" => info("autofs", "A placeholder that mounts network shares automatically when you open them.", &[]),
        "squashfs" => info("SquashFS", "A compressed, read-only filesystem (Linux snaps, live USB images).", &["Read-only."]),
        "nullfs" | "bindfs" => info("Bind/null mount", "A second view of a folder that already exists somewhere else.", &[
            "Do not count it twice when adding up disks.",
        ]),
        _ if t.starts_with("fuse") || t.starts_with("macfuse") || t.starts_with("osxfuse") => {
            let mut fi = info("FUSE", "A filesystem run by a helper program (FUSE) — for example sshfs, a cloud drive, or NTFS/exFAT drivers.", &[
                "Speed and features depend on the helper program.",
            ]);
            if t.contains("sshfs") {
                fi.notes.push("This one is sshfs: files on another computer reached over SSH.");
            }
            if t == "fuseblk" {
                fi.notes.push("'fuseblk' is usually an NTFS or exFAT disk handled by a helper driver.");
            }
            fi
        }
        _ => info("Other", "A filesystem type fsmap does not know yet.", &[
            "fsmap still reports its size and free space; look up the type name for details.",
        ]),
    }
}

/// Linux pseudo filesystems that are hidden from `fsmap volumes` unless `--all`.
pub fn is_pseudo(fstype: &str) -> bool {
    matches!(
        fstype,
        "proc"
            | "sysfs"
            | "cgroup"
            | "cgroup2"
            | "devpts"
            | "mqueue"
            | "securityfs"
            | "debugfs"
            | "tracefs"
            | "pstore"
            | "bpf"
            | "configfs"
            | "fusectl"
            | "hugetlbfs"
            | "autofs"
            | "binfmt_misc"
            | "efivarfs"
            | "rpc_pipefs"
            | "nsfs"
            | "selinuxfs"
            | "devtmpfs"
            | "ramfs"
            | "sockfs"
            | "pipefs"
            | "devfs"
            | "nfsd"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explanations() {
        assert_eq!(explain("apfs").family, "APFS");
        assert!(explain("apfs").notes.iter().any(|n| n.contains("clone")));
        assert!(explain("exfat").notes.iter().any(|n| n.contains("eject")));
        assert!(explain("exfat").notes.iter().any(|n| n.contains("._")));
        assert!(explain("msdos").notes.iter().any(|n| n.contains("4 GB")));
        assert!(explain("vfat").notes.iter().any(|n| n.contains("4 GB")));
        assert!(explain("ntfs")
            .notes
            .iter()
            .any(|n| n.contains("cannot write")));
        let smb = explain("smbfs");
        assert!(smb
            .notes
            .iter()
            .any(|n| n.contains("@eaDir") && n.contains("#recycle")));
        assert_eq!(explain("cifs").family, smb.family);
        assert_eq!(explain("HFS").family, explain("hfs").family);
        for t in [
            "afpfs", "nfs", "nfs4", "webdav", "btrfs", "ext4", "xfs", "zfs", "ufs", "iso9660",
            "cd9660", "udf", "tmpfs", "overlay",
        ] {
            assert_ne!(explain(t).family, "Other", "{t} should be explained");
        }
        assert_eq!(explain("fuse.sshfs").family, "FUSE");
        assert!(explain("fuse.sshfs")
            .notes
            .iter()
            .any(|n| n.contains("sshfs")));
        assert_eq!(explain("weirdfs").family, "Other");
        assert!(is_pseudo("proc") && is_pseudo("cgroup2") && !is_pseudo("ext4"));
    }
}
