//! Special folders and clutter files that deserve a plain explanation.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Special {
    Git,
    NodeModules,
    AppBundle,
    PhotosLibrary,
    TimeMachine,
    SynologyThumbs,
    SynologyRecycle,
    Trash,
    PythonEnv,
    XcodeDerivedData,
    MacSystemIndex,
}

impl Special {
    pub const COUNT: usize = 11;
    pub const ALL: [Special; Special::COUNT] = [
        Special::Git,
        Special::NodeModules,
        Special::AppBundle,
        Special::PhotosLibrary,
        Special::TimeMachine,
        Special::SynologyThumbs,
        Special::SynologyRecycle,
        Special::Trash,
        Special::PythonEnv,
        Special::XcodeDerivedData,
        Special::MacSystemIndex,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Special::Git => ".git (code history)",
            Special::NodeModules => "node_modules",
            Special::AppBundle => "Apps (*.app)",
            Special::PhotosLibrary => "Photos libraries",
            Special::TimeMachine => "Time Machine / sparse bundles",
            Special::SynologyThumbs => "@eaDir (Synology thumbnails)",
            Special::SynologyRecycle => "#recycle (Synology recycle bin)",
            Special::Trash => "Trash folders",
            Special::PythonEnv => "Python venvs / __pycache__",
            Special::XcodeDerivedData => "Xcode DerivedData",
            Special::MacSystemIndex => "macOS index folders",
        }
    }

    pub fn explain(self) -> &'static str {
        match self {
            Special::Git => "The full history of a code project. Needed by git; safe only if the project is pushed somewhere else.",
            Special::NodeModules => "Downloaded JavaScript packages. Can be rebuilt with 'npm install', so it is not precious.",
            Special::AppBundle => "A Mac app is really a folder. Big apps are normal.",
            Special::PhotosLibrary => "An Apple Photos library. Never edit inside it by hand; open it with Photos.",
            Special::TimeMachine => "Time Machine backups or disk-image bundles. Manage them with Time Machine / Disk Utility, not Finder.",
            Special::SynologyThumbs => "Thumbnails and indexes made by the Synology NAS. It can rebuild them.",
            Special::SynologyRecycle => "Files deleted from a Synology share wait here and still use space until the recycle bin is emptied in DSM.",
            Special::Trash => "Deleted files waiting in the Trash. They still use space until the Trash is emptied.",
            Special::PythonEnv => "Python environments and byte-code caches. They can be recreated.",
            Special::XcodeDerivedData => "Xcode build leftovers. Xcode rebuilds them; safe to clear from Xcode settings.",
            Special::MacSystemIndex => "Spotlight search index, file-event log and document versions that macOS keeps on each disk.",
        }
    }

    /// Does a folder with this name belong to a special category?
    pub fn of_dir(name: &str) -> Option<Special> {
        Some(match name {
            ".git" => Special::Git,
            "node_modules" => Special::NodeModules,
            "Backups.backupdb" => Special::TimeMachine,
            "@eaDir" => Special::SynologyThumbs,
            "#recycle" => Special::SynologyRecycle,
            ".Trash" | ".Trashes" | "$RECYCLE.BIN" => Special::Trash,
            ".venv" | "venv" | "virtualenv" | ".virtualenvs" | "__pycache__" => Special::PythonEnv,
            "DerivedData" => Special::XcodeDerivedData,
            ".Spotlight-V100" | ".fseventsd" | ".DocumentRevisions-V100" | ".TemporaryItems" => {
                Special::MacSystemIndex
            }
            n if n.ends_with(".app") && n.len() > 4 => Special::AppBundle,
            n if n.ends_with(".photoslibrary") => Special::PhotosLibrary,
            n if n.ends_with(".sparsebundle") || n.ends_with(".backupbundle") => {
                Special::TimeMachine
            }
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Clutter {
    AppleDouble,
    DsStore,
    ThumbsDb,
    DesktopIni,
}

impl Clutter {
    pub const ALL: [Clutter; 4] = [
        Clutter::AppleDouble,
        Clutter::DsStore,
        Clutter::ThumbsDb,
        Clutter::DesktopIni,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn of_file(name: &str) -> Option<Clutter> {
        match name {
            ".DS_Store" => Some(Clutter::DsStore),
            "Thumbs.db" | "thumbs.db" | "ehthumbs.db" => Some(Clutter::ThumbsDb),
            "desktop.ini" | "Desktop.ini" => Some(Clutter::DesktopIni),
            n if n.starts_with("._") => Some(Clutter::AppleDouble),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Clutter::AppleDouble => "._* (AppleDouble)",
            Clutter::DsStore => ".DS_Store",
            Clutter::ThumbsDb => "Thumbs.db",
            Clutter::DesktopIni => "desktop.ini",
        }
    }

    pub fn explain(self) -> &'static str {
        match self {
            Clutter::AppleDouble => "Mac metadata (labels, resource forks) saved next to files on disks that cannot store it (exFAT, FAT32, some NAS shares).",
            Clutter::DsStore => "Finder's memory of icon positions and view settings for a folder.",
            Clutter::ThumbsDb => "Windows' thumbnail cache for a folder.",
            Clutter::DesktopIni => "Windows' folder-appearance settings.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_dirs() {
        assert_eq!(Special::of_dir(".git"), Some(Special::Git));
        assert_eq!(Special::of_dir("Safari.app"), Some(Special::AppBundle));
        assert_eq!(Special::of_dir(".app"), None);
        assert_eq!(
            Special::of_dir("Photos Library.photoslibrary"),
            Some(Special::PhotosLibrary)
        );
        assert_eq!(
            Special::of_dir("Mac.sparsebundle"),
            Some(Special::TimeMachine)
        );
        assert_eq!(Special::of_dir("@eaDir"), Some(Special::SynologyThumbs));
        assert_eq!(Special::of_dir("#recycle"), Some(Special::SynologyRecycle));
        assert_eq!(Special::of_dir("__pycache__"), Some(Special::PythonEnv));
        assert_eq!(Special::of_dir(".fseventsd"), Some(Special::MacSystemIndex));
        assert_eq!(Special::of_dir("Documents"), None);
        for (i, s) in Special::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
        }
        assert_eq!(Clutter::of_file("._IMG.jpg"), Some(Clutter::AppleDouble));
        assert_eq!(Clutter::of_file(".DS_Store"), Some(Clutter::DsStore));
        assert_eq!(Clutter::of_file("Thumbs.db"), Some(Clutter::ThumbsDb));
        assert_eq!(Clutter::of_file("notes.md"), None);
    }
}
