//! File kinds by extension.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Photo,
    RawPhoto,
    Video,
    Audio,
    Document,
    Note,
    Code,
    Archive,
    DiskImage,
    AiModel,
    Database,
    Font,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::Photo,
        Kind::RawPhoto,
        Kind::Video,
        Kind::Audio,
        Kind::Document,
        Kind::Note,
        Kind::Code,
        Kind::Archive,
        Kind::DiskImage,
        Kind::AiModel,
        Kind::Database,
        Kind::Font,
        Kind::Other,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Photo => "Photos",
            Kind::RawPhoto => "RAW photos",
            Kind::Video => "Videos",
            Kind::Audio => "Audio",
            Kind::Document => "Documents",
            Kind::Note => "Notes / Markdown",
            Kind::Code => "Code",
            Kind::Archive => "Archives",
            Kind::DiskImage => "Disk images",
            Kind::AiModel => "AI models",
            Kind::Database => "Databases",
            Kind::Font => "Fonts",
            Kind::Other => "Other",
        }
    }
}

/// Lower-case extension of a file name ("Photo.JPG" -> "jpg"), if any.
pub fn extension(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    if dot == 0 || dot + 1 == name.len() {
        return None; // ".bashrc" or "file."
    }
    Some(name[dot + 1..].to_ascii_lowercase())
}

pub fn kind_of_ext(ext: &str) -> Kind {
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "heic" | "heif" | "webp" | "tif" | "tiff" | "bmp"
        | "svg" | "avif" | "psd" | "jxl" => Kind::Photo,
        "cr2" | "cr3" | "nef" | "nrw" | "arw" | "srf" | "sr2" | "dng" | "raf" | "orf" | "rw2"
        | "pef" | "srw" | "x3f" | "3fr" | "iiq" | "erf" | "mrw" | "raw" => Kind::RawPhoto,
        "mp4" | "mov" | "m4v" | "mkv" | "avi" | "wmv" | "flv" | "webm" | "mts" | "m2ts" | "mpg"
        | "mpeg" | "3gp" | "braw" | "r3d" | "mxf" | "insv" | "lrv" => Kind::Video,
        "mp3" | "m4a" | "aac" | "wav" | "aif" | "aiff" | "flac" | "ogg" | "opus" | "alac"
        | "wma" | "caf" | "mid" | "midi" | "logicx" | "band" => Kind::Audio,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "pages" | "numbers" | "key"
        | "odt" | "ods" | "odp" | "rtf" | "csv" | "epub" | "tex" => Kind::Document,
        "md" | "markdown" | "txt" | "org" | "canvas" | "excalidraw" => Kind::Note,
        "rs" | "py" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "swift" | "c" | "h" | "cc"
        | "cpp" | "hpp" | "m" | "mm" | "go" | "java" | "kt" | "rb" | "php" | "sh" | "zsh"
        | "bash" | "fish" | "pl" | "lua" | "cs" | "scala" | "html" | "htm" | "css" | "scss"
        | "json" | "toml" | "yaml" | "yml" | "xml" | "ipynb" | "sql" | "r" | "jl" | "zig"
        | "dart" | "vue" | "svelte" => Kind::Code,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "lz4" | "lzma"
        | "cpio" | "pkg" | "deb" | "rpm" | "whl" | "jar" | "xip" => Kind::Archive,
        "dmg" | "iso" | "img" | "sparseimage" | "sparsebundle" | "vmdk" | "vdi" | "vhd"
        | "vhdx" | "qcow2" | "toast" | "cdr" | "ipsw" | "utm" => Kind::DiskImage,
        "gguf" | "ggml" | "safetensors" | "onnx" | "pt" | "pth" | "ckpt" | "mlmodel"
        | "mlpackage" | "tflite" | "h5" | "npz" | "mlx" => Kind::AiModel,
        "db" | "sqlite" | "sqlite3" | "sqlitedb" | "realm" | "mdb" | "accdb" | "duckdb"
        | "parquet" | "ldb" | "wal" => Kind::Database,
        "ttf" | "otf" | "ttc" | "woff" | "woff2" | "dfont" => Kind::Font,
        _ => Kind::Other,
    }
}

pub fn kind_of(name: &str) -> Kind {
    if name.starts_with("._") {
        return Kind::Other; // AppleDouble side files are counted as clutter, not photos.
    }
    extension(name).map_or(Kind::Other, |e| kind_of_ext(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(kind_of("IMG_0001.HEIC"), Kind::Photo);
        assert_eq!(kind_of("DSC0001.ARW"), Kind::RawPhoto);
        assert_eq!(kind_of("clip.MOV"), Kind::Video);
        assert_eq!(kind_of("song.flac"), Kind::Audio);
        assert_eq!(kind_of("tax.pdf"), Kind::Document);
        assert_eq!(kind_of("Daily Note.md"), Kind::Note);
        assert_eq!(kind_of("main.rs"), Kind::Code);
        assert_eq!(kind_of("backup.tar.gz"), Kind::Archive);
        assert_eq!(kind_of("macOS.dmg"), Kind::DiskImage);
        assert_eq!(kind_of("llama-3-8b.Q4_K_M.gguf"), Kind::AiModel);
        assert_eq!(kind_of("model.safetensors"), Kind::AiModel);
        assert_eq!(kind_of("Photos.sqlite"), Kind::Database);
        assert_eq!(kind_of("Inter.woff2"), Kind::Font);
        assert_eq!(kind_of("README"), Kind::Other);
        assert_eq!(kind_of(".bashrc"), Kind::Other);
        assert_eq!(kind_of("._IMG_0001.jpg"), Kind::Other);
        assert_eq!(Kind::ALL.len(), Kind::Other.index() + 1);
        for (i, k) in Kind::ALL.iter().enumerate() {
            assert_eq!(k.index(), i);
        }
    }
}
