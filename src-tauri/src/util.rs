//! Small shared helpers.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Current time as Unix seconds.
pub fn now() -> i64 {
    to_unix(SystemTime::now()).unwrap_or(0)
}

/// Current time as Unix milliseconds (used for ordering-sensitive records).
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Converts a `SystemTime` to Unix seconds, handling pre-epoch timestamps.
pub fn to_unix(t: SystemTime) -> Option<i64> {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).ok(),
        Err(e) => i64::try_from(e.duration().as_secs()).ok().map(|s| -s),
    }
}

/// Returns the path as UTF-8, or `None` when the platform path cannot be
/// represented losslessly. SORTD never stores lossy paths because a lossy
/// path would point at a different (or nonexistent) file during operations.
pub fn path_str(p: &Path) -> Option<&str> {
    p.to_str()
}

/// Lowercased extension without the leading dot.
pub fn extension_of(p: &Path) -> Option<String> {
    p.extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
}

/// Escapes `%`, `_` and the escape character for SQL `LIKE ... ESCAPE '\'`.
pub fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A `LIKE` pattern matching every path strictly below `dir`.
pub fn descendants_pattern(dir: &str) -> String {
    let sep = std::path::MAIN_SEPARATOR;
    let mut base = escape_like(dir);
    if !dir.ends_with(sep) {
        base.push(sep);
    }
    base.push('%');
    base
}

/// Human-readable byte size using binary units (KiB shown as "KB").
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }

    #[test]
    fn escape_like_escapes_wildcards() {
        assert_eq!(escape_like("a_b%c\\d"), "a\\_b\\%c\\\\d");
    }

    #[test]
    fn extension_is_lowercased() {
        assert_eq!(extension_of(Path::new("photo.JPG")).as_deref(), Some("jpg"));
        assert_eq!(extension_of(Path::new("README")), None);
        assert_eq!(extension_of(Path::new(".bashrc")), None);
    }

    #[test]
    fn pre_epoch_times_are_negative() {
        let t = UNIX_EPOCH - std::time::Duration::from_secs(10);
        assert_eq!(to_unix(t), Some(-10));
    }
}
