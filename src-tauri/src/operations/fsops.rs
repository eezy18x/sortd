//! Low-level filesystem primitives used by the operation engine.

use std::fs::{File, FileTimes, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::filesystem;

const BUF: usize = 1024 * 1024;

/// Deterministic temporary name for an in-progress copy, so that crash
/// recovery can find and remove leftovers.
pub fn temp_path(dst: &Path, batch_id: i64, item_id: i64) -> PathBuf {
    let name = dst
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Keep the temp name within the 255-byte component limit.
    let short: String = name.chars().take(100).collect();
    dst.with_file_name(format!(".{short}.sortd-{batch_id}-{item_id}.partial"))
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut f = filesystem::open_for_read(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; BUF];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

pub fn hash(path: &Path) -> io::Result<String> {
    hash_file(path)
}

/// Copies `src` to `dst` without ever exposing a partial file at `dst`:
///
/// 1. stream `src` into a new temp file next to `dst` (create-new, never
///    overwrite), hashing the bytes read;
/// 2. fsync, copy permissions and timestamps;
/// 3. re-read the temp file and compare its BLAKE3 hash with the source's;
/// 4. no-replace rename the temp file to `dst`.
///
/// Returns the verified content hash. On any failure the temp file is removed
/// and `dst` is left untouched.
pub fn verified_copy(src: &Path, dst: &Path, temp: &Path) -> io::Result<String> {
    let result = (|| -> io::Result<String> {
        let src_meta = std::fs::symlink_metadata(src)?;
        if !src_meta.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source is not a regular file",
            ));
        }
        let mut input = filesystem::open_for_read(src)?;
        let mut out: File = OpenOptions::new().write(true).create_new(true).open(temp)?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; BUF];
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        out.flush()?;
        out.sync_all()?;
        let mut times = FileTimes::new();
        if let Ok(m) = src_meta.modified() {
            times = times.set_modified(m);
        }
        if let Ok(a) = src_meta.accessed() {
            times = times.set_accessed(a);
        }
        let _ = out.set_times(times);
        drop(out);
        let _ = std::fs::set_permissions(temp, src_meta.permissions());

        let expected = hasher.finalize().to_hex().to_string();
        let actual = hash_file(temp)?;
        if expected != actual {
            return Err(io::Error::other(
                "verification failed: the copy does not match the original",
            ));
        }
        filesystem::rename_no_replace(temp, dst)?;
        Ok(expected)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

/// Renames, supporting case-only renames on case-insensitive filesystems
/// (where the destination "exists" because it is the same file).
pub fn rename(src: &Path, dst: &Path, temp: &Path) -> io::Result<()> {
    let case_only = src != dst && filesystem::paths_equal(src, dst);
    if case_only {
        filesystem::rename_no_replace(src, temp)?;
        if let Err(e) = filesystem::rename_no_replace(temp, dst) {
            // Put it back under its original name.
            let _ = filesystem::rename_no_replace(temp, src);
            return Err(e);
        }
        return Ok(());
    }
    filesystem::rename_no_replace(src, dst)
}

/// Moves `src` to the platform trash.
#[cfg(not(test))]
pub fn trash(src: &Path) -> io::Result<()> {
    trash::delete(src).map_err(|e| io::Error::other(e.to_string()))?;
    if std::fs::symlink_metadata(src).is_ok() {
        return Err(io::Error::other(
            "the item is still present after moving it to the trash",
        ));
    }
    Ok(())
}

/// Test builds never touch the real system trash: items are moved into an
/// isolated folder under the OS temp directory instead.
#[cfg(test)]
pub fn trash(src: &Path) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join("sortd-test-trash").join(format!(
        "{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir)?;
    let name = src
        .file_name()
        .ok_or_else(|| io::Error::other("no file name"))?;
    std::fs::rename(src, dir.join(name))?;
    if std::fs::symlink_metadata(src).is_ok() {
        return Err(io::Error::other(
            "the item is still present after moving it to the trash",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_copy_preserves_content_and_mtime() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src.bin");
        let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 253) as u8).collect();
        std::fs::write(&src, &data).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let dst = d.path().join("dst.bin");
        let h = verified_copy(&src, &dst, &temp_path(&dst, 1, 1)).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), data);
        assert_eq!(h, blake3::hash(&data).to_hex().to_string());
        assert_eq!(std::fs::metadata(&dst).unwrap().modified().unwrap(), old);
        assert!(!temp_path(&dst, 1, 1).exists());
    }

    #[test]
    fn verified_copy_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a");
        let dst = d.path().join("b");
        std::fs::write(&src, b"new").unwrap();
        std::fs::write(&dst, b"precious").unwrap();
        let err = verified_copy(&src, &dst, &temp_path(&dst, 1, 2)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&dst).unwrap(), b"precious");
        assert!(
            !temp_path(&dst, 1, 2).exists(),
            "temp file must be cleaned up"
        );
    }

    #[test]
    fn case_only_rename() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("report.txt");
        std::fs::write(&src, b"x").unwrap();
        let dst = d.path().join("Report.txt");
        rename(&src, &dst, &temp_path(&dst, 1, 3)).unwrap();
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("Report.txt")]);
    }
}
