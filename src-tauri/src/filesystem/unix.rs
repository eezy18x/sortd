//! Unix (Linux and macOS) implementation of the filesystem abstraction.

use std::ffi::CString;
use std::fs::Metadata;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::EntryInfo;

/// Extensions that a desktop environment may execute or interpret on open.
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "sh", "bash", "zsh", "command", "desktop", "appimage", "run", "bin", "app", "pkg", "jar", "py",
    "pl", "rb", "scpt", "workflow", "terminal",
];

fn cstr(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL byte"))
}

pub fn fill_platform_info(meta: &Metadata, info: &mut EntryInfo) {
    let mode = meta.permissions().mode();
    info.raw_attributes = mode;
    info.is_executable = meta.is_file() && mode & 0o111 != 0;
    info.is_readonly = mode & 0o222 == 0;
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt as MacExt;
        const UF_HIDDEN: u32 = 0x0000_8000;
        const SF_DATALESS: u32 = 0x4000_0000;
        let flags = meta.st_flags();
        info.is_hidden = flags & UF_HIDDEN != 0;
        info.is_placeholder = flags & SF_DATALESS != 0;
    }
}

pub fn is_special_reparse_dir(_meta: &Metadata) -> bool {
    false
}

fn check_then_rename(src: &Path, dst: &Path) -> io::Result<()> {
    // Fallback for filesystems without an atomic no-replace primitive. There
    // is a small race window between the check and the rename.
    if std::fs::symlink_metadata(dst).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    std::fs::rename(src, dst)
}

#[cfg(target_os = "linux")]
pub fn rename_no_replace(src: &Path, dst: &Path) -> io::Result<()> {
    let s = cstr(src)?;
    let d = cstr(dst)?;
    // Use the raw syscall so the binary does not require glibc >= 2.28.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            s.as_ptr(),
            libc::AT_FDCWD,
            d.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::EEXIST) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        )),
        Some(libc::EINVAL) | Some(libc::ENOSYS) | Some(libc::ENOTSUP) => {
            check_then_rename(src, dst)
        }
        _ => Err(err),
    }
}

#[cfg(target_os = "macos")]
pub fn rename_no_replace(src: &Path, dst: &Path) -> io::Result<()> {
    let s = cstr(src)?;
    let d = cstr(dst)?;
    let rc = unsafe { libc::renamex_np(s.as_ptr(), d.as_ptr(), libc::RENAME_EXCL) };
    if rc == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::EEXIST) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        )),
        Some(libc::ENOTSUP) | Some(libc::EINVAL) => check_then_rename(src, dst),
        _ => Err(err),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn rename_no_replace(src: &Path, dst: &Path) -> io::Result<()> {
    let _ = cstr;
    check_then_rename(src, dst)
}

pub fn is_cross_device(err: &io::Error) -> bool {
    err.raw_os_error() == Some(libc::EXDEV) || err.kind() == io::ErrorKind::CrossesDevices
}

pub fn volume_key(path: &Path) -> Option<String> {
    std::fs::metadata(path)
        .ok()
        .map(|m| format!("dev:{}", m.dev()))
}

pub fn disk_space(path: &Path) -> io::Result<super::DiskSpace> {
    let c = cstr(path)?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    let frsize = st.f_frsize as u64;
    Ok(super::DiskSpace {
        total: (st.f_blocks as u64).saturating_mul(frsize),
        available: (st.f_bavail as u64).saturating_mul(frsize),
    })
}

pub fn validate_file_name(_name: &str) -> Result<(), String> {
    // '/' and NUL are already rejected by the common validator.
    Ok(())
}

pub fn default_excluded_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        for p in [
            "/System",
            "/Library",
            "/private",
            "/dev",
            "/cores",
            "/bin",
            "/sbin",
            "/usr",
            "/opt/homebrew",
        ] {
            out.push(PathBuf::from(p));
        }
        if let Some(home) = home_dir() {
            out.push(home.join("Library"));
            out.push(home.join(".Trash"));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        for p in [
            "/proc",
            "/sys",
            "/dev",
            "/run",
            "/boot",
            "/usr",
            "/bin",
            "/sbin",
            "/lib",
            "/lib32",
            "/lib64",
            "/etc",
            "/var",
            "/snap",
            "/tmp",
            "/lost+found",
        ] {
            out.push(PathBuf::from(p));
        }
        if let Some(home) = home_dir() {
            out.push(home.join(".cache"));
            out.push(home.join(".local").join("share").join("Trash"));
        }
    }
    out
}

pub fn default_excluded_names() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &[
            ".Trashes",
            ".Spotlight-V100",
            ".fseventsd",
            ".DocumentRevisions-V100",
            ".TemporaryItems",
        ]
    }
    #[cfg(not(target_os = "macos"))]
    {
        &["lost+found", ".Trash-1000"]
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub fn open_for_read(path: &Path) -> io::Result<std::fs::File> {
    // O_NOFOLLOW: refuse to open if the final component became a symlink.
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

pub fn is_executable_name(ext: Option<&str>) -> bool {
    ext.map(|e| EXECUTABLE_EXTENSIONS.contains(&e))
        .unwrap_or(false)
}
