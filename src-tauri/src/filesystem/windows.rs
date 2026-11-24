//! Windows implementation of the filesystem abstraction.

use std::ffi::OsStr;
use std::fs::Metadata;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf, Prefix};

use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, MoveFileExW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_OFFLINE,
    FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM, FILE_FLAG_OPEN_REPARSE_POINT,
};

use super::EntryInfo;

const ERROR_NOT_SAME_DEVICE: i32 = 17;
const ERROR_FILE_EXISTS: i32 = 80;
const ERROR_ALREADY_EXISTS: i32 = 183;

/// File extensions Windows will execute or interpret when "opened".
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe", "com", "bat", "cmd", "msi", "msix", "ps1", "vbs", "vbe", "js", "jse", "wsf", "wsh",
    "scr", "pif", "lnk", "url", "cpl", "hta", "reg", "jar",
];

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn fill_platform_info(meta: &Metadata, info: &mut EntryInfo) {
    let attrs = meta.file_attributes();
    info.raw_attributes = attrs;
    info.is_hidden = attrs & FILE_ATTRIBUTE_HIDDEN != 0;
    info.is_system = attrs & FILE_ATTRIBUTE_SYSTEM != 0;
    info.is_readonly = attrs & FILE_ATTRIBUTE_READONLY != 0;
    info.is_placeholder = attrs
        & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0;
    info.is_executable = false; // Determined from the extension by callers that need it.
}

pub fn is_special_reparse_dir(meta: &Metadata) -> bool {
    // Junctions and mount points are reported by std as symlinks; any other
    // reparse-point directory (except cloud placeholders, which are normal
    // folders for traversal purposes) is treated as a boundary.
    let attrs = meta.file_attributes();
    meta.is_dir()
        && attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && attrs
            & (FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
                | FILE_ATTRIBUTE_OFFLINE)
            == 0
}

pub fn rename_no_replace(src: &Path, dst: &Path) -> io::Result<()> {
    let s = wide(src.as_os_str());
    let d = wide(dst.as_os_str());
    // Flags = 0: no MOVEFILE_REPLACE_EXISTING (fail if dst exists) and no
    // MOVEFILE_COPY_ALLOWED (fail across volumes instead of copying silently).
    let ok = unsafe { MoveFileExW(s.as_ptr(), d.as_ptr(), 0) };
    if ok != 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(ERROR_FILE_EXISTS) | Some(ERROR_ALREADY_EXISTS) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        )),
        _ => Err(err),
    }
}

pub fn is_cross_device(err: &io::Error) -> bool {
    err.raw_os_error() == Some(ERROR_NOT_SAME_DEVICE) || err.kind() == io::ErrorKind::CrossesDevices
}

pub fn volume_key(path: &Path) -> Option<String> {
    let canonical = dunce::canonicalize(path).ok()?;
    match canonical.components().next()? {
        Component::Prefix(p) => Some(match p.kind() {
            Prefix::Disk(d) | Prefix::VerbatimDisk(d) => {
                format!("disk:{}", (d as char).to_ascii_uppercase())
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                "unc:{}\\{}",
                server.to_string_lossy().to_lowercase(),
                share.to_string_lossy().to_lowercase()
            ),
            other => format!("other:{}", other_prefix(&other)),
        }),
        _ => None,
    }
}

fn other_prefix(p: &Prefix<'_>) -> String {
    format!("{p:?}").to_lowercase()
}

pub fn disk_space(path: &Path) -> io::Result<super::DiskSpace> {
    let w = wide(path.as_os_str());
    let mut available: u64 = 0;
    let mut total: u64 = 0;
    let mut free: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut available, &mut total, &mut free) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(super::DiskSpace { total, available })
}

pub fn validate_file_name(name: &str) -> Result<(), String> {
    if let Some(c) = name
        .chars()
        .find(|c| matches!(c, '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*'))
    {
        return Err(format!(
            "name contains a character Windows does not allow: '{c}'"
        ));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("Windows does not allow names ending with a dot or space".into());
    }
    if super::is_windows_reserved_name(name) {
        return Err(format!("'{name}' is a reserved device name on Windows"));
    }
    Ok(())
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub fn default_excluded_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let drive = env_path("SystemDrive").unwrap_or_else(|| PathBuf::from("C:"));
    let root = PathBuf::from(format!("{}\\", drive.display()));
    for v in [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "ProgramW6432",
    ] {
        if let Some(p) = env_path(v) {
            out.push(p);
        }
    }
    for n in [
        "$Recycle.Bin",
        "System Volume Information",
        "Recovery",
        "PerfLogs",
        "Config.Msi",
    ] {
        out.push(root.join(n));
    }
    if let Some(home) = home_dir() {
        out.push(home.join("AppData"));
    }
    out.sort();
    out.dedup();
    out
}

pub fn default_excluded_names() -> &'static [&'static str] {
    &["$RECYCLE.BIN", "System Volume Information"]
}

pub fn home_dir() -> Option<PathBuf> {
    env_path("USERPROFILE")
}

pub fn open_for_read(path: &Path) -> io::Result<std::fs::File> {
    // Opening the reparse point itself prevents following a symlink that was
    // swapped in after the scan observed a regular file.
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

pub fn is_executable_name(ext: Option<&str>) -> bool {
    ext.map(|e| EXECUTABLE_EXTENSIONS.contains(&e))
        .unwrap_or(false)
}
