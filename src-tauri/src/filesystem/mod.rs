//! Cross-platform filesystem abstraction.
//!
//! Everything that behaves differently between Windows, macOS and Linux is
//! isolated here behind a single interface. The rest of the backend never
//! uses `cfg(target_os)` directly.
//!
//! Notable platform differences handled here (see docs/PLATFORM_SUPPORT.md):
//! * Hidden files: dot-prefix on Unix, `FILE_ATTRIBUTE_HIDDEN` on Windows,
//!   `UF_HIDDEN` on macOS.
//! * Cloud placeholders (OneDrive Files-On-Demand, iCloud "dataless" files):
//!   reading them triggers a download, so they are never hashed or sniffed.
//! * No-replace rename: `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING` on
//!   Windows, `renameat2(RENAME_NOREPLACE)` on Linux, `renamex_np(RENAME_EXCL)`
//!   on macOS. `std::fs::rename` silently overwrites on every platform and is
//!   therefore never used directly for user files.
//! * Volume identity: device id on Unix, path prefix (drive / UNC share) on
//!   Windows.
//! * Path comparison is case-insensitive on Windows and macOS.

use std::fs::Metadata;
use std::io;
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

use crate::util::to_unix;

/// Metadata SORTD records for every indexed entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryInfo {
    pub size: u64,
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub accessed: Option<i64>,
    pub is_hidden: bool,
    pub is_readonly: bool,
    pub is_system: bool,
    pub is_symlink: bool,
    pub is_executable: bool,
    /// The file's data is not present locally (cloud placeholder). Reading it
    /// would trigger a download, so SORTD never reads its contents.
    pub is_placeholder: bool,
    /// Raw platform attribute bits (Windows attributes / Unix mode).
    pub raw_attributes: u32,
}

/// Extracts [`EntryInfo`] from metadata obtained with `symlink_metadata`.
pub fn entry_info(path: &Path, meta: &Metadata) -> EntryInfo {
    let mut info = EntryInfo {
        size: if meta.is_file() { meta.len() } else { 0 },
        created: meta.created().ok().and_then(to_unix),
        modified: meta.modified().ok().and_then(to_unix),
        accessed: meta.accessed().ok().and_then(to_unix),
        is_readonly: meta.permissions().readonly(),
        is_symlink: meta.file_type().is_symlink(),
        ..Default::default()
    };
    let name_hidden = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.') && n != "." && n != "..")
        .unwrap_or(false);
    imp::fill_platform_info(meta, &mut info);
    info.is_hidden |= name_hidden;
    info
}

/// True when the entry must not be descended into even though it looks like a
/// directory (e.g. reparse points other than symlinks on Windows).
pub fn is_traversal_boundary(meta: &Metadata) -> bool {
    meta.file_type().is_symlink() || imp::is_special_reparse_dir(meta)
}

/// Renames `src` to `dst`, failing with `AlreadyExists` instead of
/// overwriting an existing destination.
///
/// Atomic where the platform supports it. On filesystems that do not support
/// the no-replace primitive (some network and FUSE filesystems on Linux) this
/// falls back to check-then-rename, which has a small race window; this is
/// documented in SECURITY.md.
pub fn rename_no_replace(src: &Path, dst: &Path) -> io::Result<()> {
    imp::rename_no_replace(src, dst)
}

/// True when the error means "source and destination are on different
/// volumes", i.e. a plain rename is impossible.
pub fn is_cross_device(err: &io::Error) -> bool {
    imp::is_cross_device(err)
}

/// Identifies the volume a path lives on. Two paths with the same key can be
/// renamed into each other without copying.
pub fn volume_key(path: &Path) -> Option<String> {
    imp::volume_key(path)
}

/// Whether two existing paths are on the same volume. Unknown is treated as
/// "different", which routes the operation through the safer copy path.
pub fn same_volume(a: &Path, b: &Path) -> bool {
    match (volume_key(a), volume_key(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Total and available bytes on the volume containing `path`.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpace {
    pub total: u64,
    pub available: u64,
}

pub fn disk_space(path: &Path) -> io::Result<DiskSpace> {
    imp::disk_space(path)
}

/// Name of the current platform, for diagnostics and UI.
pub fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

/// Whether path comparisons should ignore case on this platform.
pub const CASE_INSENSITIVE: bool = cfg!(any(windows, target_os = "macos"));

/// Case-folds a path component according to the platform's rules.
pub fn fold(s: &str) -> String {
    if CASE_INSENSITIVE {
        s.to_lowercase()
    } else {
        s.to_string()
    }
}

/// Component-wise `starts_with` that honours the platform's case rules.
pub fn path_starts_with(path: &Path, prefix: &Path) -> bool {
    let mut p = path.components();
    for pc in prefix.components() {
        match p.next() {
            Some(c) => {
                let a = fold(&c.as_os_str().to_string_lossy());
                let b = fold(&pc.as_os_str().to_string_lossy());
                if a != b {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// Component-wise equality that honours the platform's case rules.
pub fn paths_equal(a: &Path, b: &Path) -> bool {
    path_starts_with(a, b) && path_starts_with(b, a)
}

/// Lexically normalizes a path: removes `.` components and rejects `..`.
/// Returns `None` for relative paths or paths containing `..`.
pub fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => return None,
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// Resolves a path to its canonical form without the Windows `\\?\` prefix.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// Validates a single file or directory name for the current platform.
/// Returns a human-readable reason when the name is not acceptable.
pub fn validate_file_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name is empty".into());
    }
    if name == "." || name == ".." {
        return Err("name cannot be '.' or '..'".into());
    }
    if name.len() > 255 {
        return Err("name is longer than 255 bytes".into());
    }
    if name.contains('/') || name.contains('\0') {
        return Err("name contains a path separator or NUL character".into());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("name contains control characters".into());
    }
    imp::validate_file_name(name)
}

/// Absolute paths excluded from scans by default (system locations).
pub fn default_excluded_paths() -> Vec<PathBuf> {
    imp::default_excluded_paths()
}

/// Directory names excluded at any depth by default.
pub fn default_excluded_names() -> &'static [&'static str] {
    imp::default_excluded_names()
}

/// The current user's home directory.
pub fn home_dir() -> Option<PathBuf> {
    imp::home_dir()
}

/// Opens a file for reading without following a final symlink where the
/// platform allows it. Used for hashing and content sniffing.
pub fn open_for_read(path: &Path) -> io::Result<std::fs::File> {
    imp::open_for_read(path)
}

/// Whether opening this file with the system default handler could execute
/// code. SORTD never "opens" such files; it only reveals them in the system
/// file manager.
pub fn is_unsafe_to_open(ext: Option<&str>, info: &EntryInfo) -> bool {
    info.is_executable || imp::is_executable_name(ext)
}

/// Windows reserved device names (checked case-insensitively, with or
/// without an extension). Exposed for tests on every platform.
pub fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit())
        || upper == "COM¹"
        || upper == "COM²"
        || upper == "COM³"
        || upper == "LPT¹"
        || upper == "LPT²"
        || upper == "LPT³"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_names() {
        assert!(is_windows_reserved_name("CON"));
        assert!(is_windows_reserved_name("con.txt"));
        assert!(is_windows_reserved_name("LPT1"));
        assert!(is_windows_reserved_name("com9.log"));
        assert!(!is_windows_reserved_name("console.txt"));
        assert!(!is_windows_reserved_name("COM"));
        assert!(!is_windows_reserved_name("LPTX"));
    }

    #[test]
    fn rejects_bad_names() {
        assert!(validate_file_name("").is_err());
        assert!(validate_file_name("..").is_err());
        assert!(validate_file_name("a/b").is_err());
        assert!(validate_file_name("a\0b").is_err());
        assert!(validate_file_name("bad\nname").is_err());
        assert!(validate_file_name("report.pdf").is_ok());
        assert!(validate_file_name("日本語のファイル.txt").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_specific_names() {
        assert!(validate_file_name("a:b").is_err());
        assert!(validate_file_name("what?").is_err());
        assert!(validate_file_name("trailing.").is_err());
        assert!(validate_file_name("trailing ").is_err());
        assert!(validate_file_name("NUL.txt").is_err());
        assert!(validate_file_name("a\\b").is_err());
    }

    #[test]
    fn normalize_rejects_parent_components() {
        let base = std::env::temp_dir();
        assert!(normalize_absolute(&base.join("a").join("..").join("b")).is_none());
        assert!(normalize_absolute(Path::new("relative/path")).is_none());
        let n = normalize_absolute(&base.join(".").join("x")).unwrap();
        assert_eq!(n, base.join("x"));
    }

    #[test]
    fn starts_with_is_component_wise() {
        let base = std::env::temp_dir();
        assert!(path_starts_with(&base.join("a").join("b"), &base.join("a")));
        assert!(!path_starts_with(&base.join("ab"), &base.join("a")));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn comparison_ignores_case_where_platform_does() {
        let base = std::env::temp_dir();
        assert!(path_starts_with(
            &base.join("Docs").join("x"),
            &base.join("docs")
        ));
    }

    #[test]
    fn no_replace_rename_refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, b"source").unwrap();
        std::fs::write(&b, b"destination").unwrap();
        let err = rename_no_replace(&a, &b).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&b).unwrap(), b"destination");
        assert!(a.exists());

        let c = dir.path().join("c.txt");
        rename_no_replace(&a, &c).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read(&c).unwrap(), b"source");
    }

    #[test]
    fn volume_key_is_stable_within_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        assert!(same_volume(dir.path(), &f));
    }

    #[test]
    fn disk_space_reports_nonzero_total() {
        let dir = tempfile::tempdir().unwrap();
        let s = disk_space(dir.path()).unwrap();
        assert!(s.total > 0);
        assert!(s.available <= s.total);
    }

    #[test]
    fn dotfiles_are_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join(".secret");
        std::fs::write(&f, b"x").unwrap();
        let meta = std::fs::symlink_metadata(&f).unwrap();
        assert!(entry_info(&f, &meta).is_hidden);
    }
}
