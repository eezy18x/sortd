//! Authorized locations and path validation.
//!
//! SORTD only reads or modifies paths inside locations the user explicitly
//! authorized through a native folder-picker dialog opened by the backend.
//! The webview can never add a location by sending a path string.
//!
//! Every path that arrives from the frontend, and every operation target
//! immediately before execution, goes through [`authorize_path`]:
//!
//! 1. Must be valid UTF-8, absolute, and free of NUL bytes and `..`.
//! 2. Must lie lexically inside an authorized location.
//! 3. Its nearest existing ancestor, once symlinks are resolved, must still
//!    lie inside that location (defeats symlinked-directory escapes).

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::filesystem;
use crate::util::now;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LocationKind {
    Scan,
    Destination,
}

impl LocationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LocationKind::Scan => "scan",
            LocationKind::Destination => "destination",
        }
    }
    fn parse(s: &str) -> LocationKind {
        if s == "destination" {
            LocationKind::Destination
        } else {
            LocationKind::Scan
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub id: i64,
    pub path: String,
    pub kind: LocationKind,
    pub added_at: i64,
    pub last_scan_id: Option<i64>,
    pub last_scan_at: Option<i64>,
    pub last_scan_status: Option<String>,
    pub file_count: i64,
    pub total_size: i64,
    pub exists: bool,
}

fn row_to_location(r: &rusqlite::Row<'_>) -> rusqlite::Result<Location> {
    let path: String = r.get(1)?;
    let exists = Path::new(&path).is_dir();
    Ok(Location {
        id: r.get(0)?,
        kind: LocationKind::parse(&r.get::<_, String>(2)?),
        added_at: r.get(3)?,
        last_scan_id: r.get(4)?,
        last_scan_at: r.get(5)?,
        last_scan_status: r.get(6)?,
        file_count: r.get(7)?,
        total_size: r.get(8)?,
        exists,
        path,
    })
}

const LOCATION_SELECT: &str = "
    SELECT l.id, l.path, l.kind, l.added_at, l.last_scan_id,
           s.finished_at, s.status,
           COALESCE(d.total_files, 0), COALESCE(d.total_size, 0)
    FROM locations l
    LEFT JOIN scans s ON s.id = l.last_scan_id
    LEFT JOIN directories d ON d.path = l.path";

pub fn list_locations(conn: &Connection) -> AppResult<Vec<Location>> {
    let mut stmt = conn.prepare(&format!("{LOCATION_SELECT} ORDER BY l.kind, l.path"))?;
    let rows = stmt
        .query_map([], row_to_location)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_location(conn: &Connection, id: i64) -> AppResult<Location> {
    conn.query_row(
        &format!("{LOCATION_SELECT} WHERE l.id = ?1"),
        [id],
        row_to_location,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("location {id} not found")))
}

/// Registers a folder the user picked in a native dialog.
///
/// * Adding a folder already covered by a scan location is rejected.
/// * Adding a scan location that contains existing scan locations merges
///   them: their index data is reassigned to the new, broader location.
pub fn add_location(
    conn: &mut Connection,
    picked: &Path,
    kind: LocationKind,
) -> AppResult<Location> {
    let canonical = filesystem::canonicalize(picked)
        .map_err(|e| AppError::invalid(format!("cannot access the selected folder: {e}")))?;
    if !canonical.is_dir() {
        return Err(AppError::invalid("the selected path is not a folder"));
    }
    let path_str = canonical
        .to_str()
        .ok_or_else(|| {
            AppError::invalid("folder paths that are not valid Unicode are not supported")
        })?
        .to_string();

    let existing = list_locations(conn)?;
    for loc in &existing {
        let lp = Path::new(&loc.path);
        if filesystem::paths_equal(lp, &canonical) {
            if kind == LocationKind::Scan && loc.kind == LocationKind::Destination {
                conn.execute("UPDATE locations SET kind = 'scan' WHERE id = ?1", [loc.id])?;
            }
            return get_location(conn, loc.id);
        }
        if loc.kind == LocationKind::Scan && filesystem::path_starts_with(&canonical, lp) {
            if kind == LocationKind::Destination {
                // Already authorized through the enclosing scan location.
                return get_location(conn, loc.id);
            }
            return Err(AppError::Conflict(format!(
                "this folder is already included in {}",
                loc.path
            )));
        }
    }

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO locations(path, kind, added_at) VALUES (?1, ?2, ?3)",
        params![path_str, kind.as_str(), now()],
    )?;
    let id = tx.last_insert_rowid();
    // Merge nested locations into the new one.
    for loc in &existing {
        let nested = filesystem::path_starts_with(Path::new(&loc.path), &canonical);
        if nested && (kind == LocationKind::Scan || loc.kind == LocationKind::Destination) {
            for table in ["files", "directories", "scans"] {
                tx.execute(
                    &format!("UPDATE {table} SET location_id = ?1 WHERE location_id = ?2"),
                    params![id, loc.id],
                )?;
            }
            tx.execute("DELETE FROM locations WHERE id = ?1", [loc.id])?;
        }
    }
    tx.commit()?;
    get_location(conn, id)
}

/// Removes a location and its index data. Never touches files on disk.
pub fn remove_location(conn: &mut Connection, id: i64) -> AppResult<Location> {
    let loc = get_location(conn, id)?;
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM recommendations WHERE file_id IN (SELECT id FROM files WHERE location_id = ?1)", [id])?;
    tx.execute("DELETE FROM files WHERE location_id = ?1", [id])?;
    tx.execute("DELETE FROM directories WHERE location_id = ?1", [id])?;
    tx.execute("DELETE FROM scans WHERE location_id = ?1", [id])?;
    tx.execute("DELETE FROM locations WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(loc)
}

/// Parses and lexically validates an untrusted path string.
pub fn parse_untrusted_path(raw: &str) -> AppResult<PathBuf> {
    if raw.is_empty() || raw.len() > 32_767 {
        return Err(AppError::invalid("path is empty or too long"));
    }
    if raw.contains('\0') {
        return Err(AppError::invalid("path contains a NUL character"));
    }
    filesystem::normalize_absolute(Path::new(raw))
        .ok_or_else(|| AppError::invalid("path must be absolute and must not contain '..'"))
}

/// Returns the authorized location containing `path`, if any.
pub fn containing_location(conn: &Connection, path: &Path) -> AppResult<Option<Location>> {
    let locs = list_locations(conn)?;
    // Prefer the most specific (longest) match.
    Ok(locs
        .into_iter()
        .filter(|l| filesystem::path_starts_with(path, Path::new(&l.path)))
        .max_by_key(|l| l.path.len()))
}

/// Full authorization check for a path that SORTD is about to read or modify.
pub fn authorize_path(conn: &Connection, path: &Path) -> AppResult<Location> {
    let normalized = filesystem::normalize_absolute(path)
        .ok_or_else(|| AppError::invalid("path must be absolute and must not contain '..'"))?;
    let loc = containing_location(conn, &normalized)?.ok_or_else(|| {
        AppError::PermissionDenied(format!(
            "{} is outside the folders you have authorized SORTD to manage",
            normalized.display()
        ))
    })?;
    let root = Path::new(&loc.path);

    // Resolve the nearest existing ancestor (the path itself may not exist
    // yet, e.g. a move destination) and verify it is still inside the root.
    let mut probe: &Path = &normalized;
    loop {
        if std::fs::symlink_metadata(probe).is_ok() {
            break;
        }
        match probe.parent() {
            Some(p) => probe = p,
            None => break,
        }
    }
    // The probe itself may legitimately be a symlink (e.g. the file being
    // moved); resolve its parent so the link itself is what gets operated on.
    let resolve_target = if probe == normalized && probe != root {
        probe.parent().unwrap_or(probe)
    } else {
        probe
    };
    let resolved = filesystem::canonicalize(resolve_target).map_err(|e| {
        AppError::invalid(format!("cannot resolve {}: {e}", resolve_target.display()))
    })?;
    let resolved_root = filesystem::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if !filesystem::path_starts_with(&resolved, &resolved_root) {
        return Err(AppError::PermissionDenied(format!(
            "{} resolves outside the authorized folder {} (symbolic link?)",
            normalized.display(),
            loc.path
        )));
    }
    Ok(loc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_db;

    #[test]
    fn add_list_remove_locations() {
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        db.with(|c| {
            let loc = add_location(c, root.path(), LocationKind::Scan)?;
            assert_eq!(list_locations(c)?.len(), 1);
            // Adding again returns the same location.
            assert_eq!(add_location(c, root.path(), LocationKind::Scan)?.id, loc.id);
            remove_location(c, loc.id)?;
            assert!(list_locations(c)?.is_empty());
            assert!(
                root.path().exists(),
                "removing a location must never touch the disk"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn nested_locations_are_rejected_or_merged() {
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();
        db.with(|c| {
            let child_loc = add_location(c, &child, LocationKind::Scan)?;
            let parent_loc = add_location(c, root.path(), LocationKind::Scan)?;
            let all = list_locations(c)?;
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].id, parent_loc.id);
            assert_ne!(child_loc.id, parent_loc.id);
            assert!(matches!(
                add_location(c, &child, LocationKind::Scan),
                Err(AppError::Conflict(_))
            ));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn rejects_traversal_and_relative_paths() {
        assert!(parse_untrusted_path("relative/x").is_err());
        assert!(parse_untrusted_path("").is_err());
        let base = std::env::temp_dir();
        let evil = format!(
            "{}{}..{}etc",
            base.display(),
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        assert!(parse_untrusted_path(&evil).is_err());
    }

    #[test]
    fn authorize_inside_and_outside() {
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        db.with(|c| {
            let loc = add_location(c, root.path(), LocationKind::Scan)?;
            let base = PathBuf::from(&loc.path);
            assert!(authorize_path(c, &base.join("new").join("file.txt")).is_ok());
            let out = filesystem::canonicalize(outside.path())?;
            assert!(matches!(
                authorize_path(c, &out.join("x")),
                Err(AppError::PermissionDenied(_))
            ));
            Ok(())
        })
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_escape_is_blocked() {
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        db.with(|c| {
            let loc = add_location(c, root.path(), LocationKind::Scan)?;
            let target = PathBuf::from(&loc.path).join("escape").join("victim.txt");
            assert!(matches!(
                authorize_path(c, &target),
                Err(AppError::PermissionDenied(_))
            ));
            Ok(())
        })
        .unwrap();
    }
}
