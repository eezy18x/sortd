//! Keeps the index consistent with completed file operations so the UI
//! reflects changes immediately, without requiring a rescan.

use std::path::{Path, MAIN_SEPARATOR};

use rusqlite::{params, Connection, OptionalExtension};

use crate::categorization::classify;
use crate::error::AppResult;
use crate::security::{self, LocationKind};
use crate::util::extension_of;

fn scan_location_for(conn: &Connection, path: &Path) -> AppResult<Option<i64>> {
    Ok(security::containing_location(conn, path)?
        .filter(|l| l.kind == LocationKind::Scan)
        .map(|l| l.id))
}

fn prefix_of(dir: &str) -> String {
    if dir.ends_with(MAIN_SEPARATOR) {
        dir.to_string()
    } else {
        format!("{dir}{MAIN_SEPARATOR}")
    }
}

/// Adds deltas to `start` and every indexed ancestor of it.
fn adjust_ancestors(
    conn: &Connection,
    start: Option<String>,
    dsize: i64,
    dfiles: i64,
    ddirs: i64,
    direct_file: bool,
) -> AppResult<()> {
    let mut cur = start;
    let mut first = true;
    let mut guard = 0;
    while let Some(dir) = cur {
        let (own_size, own_files) = if first && direct_file {
            (dsize, dfiles)
        } else {
            (0, 0)
        };
        conn.execute(
            "UPDATE directories SET total_size = MAX(0, total_size + ?2), total_files = MAX(0, total_files + ?3),
                total_dirs = MAX(0, total_dirs + ?4), own_size = MAX(0, own_size + ?5), own_files = MAX(0, own_files + ?6)
             WHERE path = ?1",
            params![dir, dsize, dfiles, ddirs, own_size, own_files],
        )?;
        cur = conn
            .query_row(
                "SELECT parent FROM directories WHERE path = ?1",
                [&dir],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        first = false;
        guard += 1;
        if guard > 4096 {
            break;
        }
    }
    Ok(())
}

fn parent_str(p: &str) -> Option<String> {
    Path::new(p)
        .parent()
        .and_then(|x| x.to_str())
        .map(str::to_string)
}

fn file_size(conn: &Connection, path: &str) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row("SELECT size FROM files WHERE path = ?1", [path], |r| {
            r.get(0)
        })
        .optional()?)
}

fn dir_totals(conn: &Connection, path: &str) -> AppResult<Option<(i64, i64, i64, i64)>> {
    Ok(conn
        .query_row(
            "SELECT total_size, total_files, total_dirs, depth FROM directories WHERE path = ?1",
            [path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?)
}

fn delete_subtree(conn: &Connection, dir: &str) -> AppResult<()> {
    let prefix = prefix_of(dir);
    let len = prefix.chars().count() as i64;
    conn.execute(
        "DELETE FROM files WHERE substr(path, 1, ?1) = ?2",
        params![len, prefix],
    )?;
    conn.execute(
        "DELETE FROM directories WHERE path = ?1 OR substr(path, 1, ?2) = ?3",
        params![dir, len, prefix],
    )?;
    Ok(())
}

pub fn after_move(conn: &Connection, src: &str, dst: &str, is_dir: bool) -> AppResult<()> {
    let dst_loc = scan_location_for(conn, Path::new(dst))?;
    let new_parent = parent_str(dst);
    let old_parent = parent_str(src);
    if !is_dir {
        let Some(size) = file_size(conn, src)? else {
            return Ok(());
        };
        adjust_ancestors(conn, old_parent, -size, -1, 0, true)?;
        // Remove any stale row already occupying the destination path.
        conn.execute("DELETE FROM files WHERE path = ?1", [dst])?;
        match dst_loc {
            None => {
                conn.execute("DELETE FROM files WHERE path = ?1", [src])?;
            }
            Some(loc) => {
                let name = Path::new(dst)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let ext = extension_of(Path::new(dst));
                let c = classify(&name, ext.as_deref(), &[]);
                conn.execute(
                    "UPDATE files SET path = ?2, parent = ?3, name = ?4, extension = ?5, location_id = ?6,
                        category = ?7, category_confidence = ?8, mime = ?9
                     WHERE path = ?1",
                    params![src, dst, new_parent.clone().unwrap_or_default(), name, ext, loc, c.category.as_str(), c.confidence, c.mime_hint],
                )?;
                adjust_ancestors(conn, new_parent, size, 1, 0, true)?;
            }
        }
        return Ok(());
    }

    // Directory move: rewrite every path below it.
    let totals = dir_totals(conn, src)?;
    if let Some((size, files, dirs, _)) = totals {
        adjust_ancestors(conn, old_parent.clone(), -size, -files, -(dirs + 1), false)?;
    }
    delete_subtree(conn, dst)?;
    let Some(loc) = dst_loc else {
        delete_subtree(conn, src)?;
        return Ok(());
    };
    let old_prefix = prefix_of(src);
    let new_prefix = prefix_of(dst);
    let old_len = old_prefix.chars().count() as i64;
    let new_depth: Option<i64> = match &new_parent {
        Some(p) => conn
            .query_row(
                "SELECT depth + 1 FROM directories WHERE path = ?1",
                [p],
                |r| r.get(0),
            )
            .optional()?,
        None => None,
    };
    let depth_delta = match (new_depth, totals) {
        (Some(nd), Some((_, _, _, od))) => nd - od,
        _ => 0,
    };
    conn.execute(
        "UPDATE files SET path = ?3 || substr(path, ?2 + 1),
            parent = CASE WHEN parent = ?4 THEN ?5 ELSE ?3 || substr(parent, ?2 + 1) END,
            location_id = ?6
         WHERE substr(path, 1, ?2) = ?1",
        params![old_prefix, old_len, new_prefix, src, dst, loc],
    )?;
    conn.execute(
        "UPDATE directories SET path = ?3 || substr(path, ?2 + 1),
            parent = CASE WHEN parent = ?4 THEN ?5 ELSE ?3 || substr(parent, ?2 + 1) END,
            depth = depth + ?7, location_id = ?6
         WHERE substr(path, 1, ?2) = ?1",
        params![old_prefix, old_len, new_prefix, src, dst, loc, depth_delta],
    )?;
    let name = Path::new(dst)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    conn.execute(
        "UPDATE directories SET path = ?2, parent = ?3, name = ?4, depth = depth + ?5, location_id = ?6 WHERE path = ?1",
        params![src, dst, new_parent, name, depth_delta, loc],
    )?;
    if let Some((size, files, dirs, _)) = totals {
        adjust_ancestors(conn, new_parent, size, files, dirs + 1, false)?;
    }
    Ok(())
}

pub fn after_copy(conn: &Connection, src: &str, dst: &str) -> AppResult<()> {
    let Some(loc) = scan_location_for(conn, Path::new(dst))? else {
        return Ok(());
    };
    let parent = parent_str(dst).unwrap_or_default();
    let name = Path::new(dst)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    conn.execute("DELETE FROM files WHERE path = ?1", [dst])?;
    let n = conn.execute(
        "INSERT INTO files (location_id, path, parent, name, extension, size, created_at, modified_at, accessed_at,
            is_hidden, is_readonly, is_system, is_symlink, is_executable, is_placeholder, attributes, mime, category,
            category_confidence, category_override, partial_hash, full_hash, hashed_size, hashed_mtime,
            last_seen_scan, first_seen_scan, changed_scan, indexed_at)
         SELECT ?2, ?3, ?4, ?5, extension, size, strftime('%s','now'), modified_at, accessed_at,
            is_hidden, is_readonly, is_system, is_symlink, is_executable, 0, attributes, mime, category,
            category_confidence, category_override, partial_hash, full_hash, hashed_size, hashed_mtime,
            last_seen_scan, last_seen_scan, last_seen_scan, strftime('%s','now')
         FROM files WHERE path = ?1",
        params![src, loc, dst, parent, name],
    )?;
    if n > 0 {
        if let Some(size) = file_size(conn, dst)? {
            adjust_ancestors(conn, Some(parent), size, 1, 0, true)?;
        }
    }
    Ok(())
}

pub fn after_trash(conn: &Connection, src: &str, is_dir: bool) -> AppResult<()> {
    let parent = parent_str(src);
    if is_dir {
        if let Some((size, files, dirs, _)) = dir_totals(conn, src)? {
            adjust_ancestors(conn, parent, -size, -files, -(dirs + 1), false)?;
        }
        delete_subtree(conn, src)?;
    } else if let Some(size) = file_size(conn, src)? {
        adjust_ancestors(conn, parent, -size, -1, 0, true)?;
        conn.execute("DELETE FROM files WHERE path = ?1", [src])?;
    }
    Ok(())
}

pub fn after_mkdir(conn: &Connection, dst: &str) -> AppResult<()> {
    let Some(loc) = scan_location_for(conn, Path::new(dst))? else {
        return Ok(());
    };
    let Some(parent) = parent_str(dst) else {
        return Ok(());
    };
    let Some(depth): Option<i64> = conn
        .query_row(
            "SELECT depth + 1 FROM directories WHERE path = ?1",
            [&parent],
            |r| r.get(0),
        )
        .optional()?
    else {
        return Ok(());
    };
    let name = Path::new(dst)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let last_seen: i64 = conn.query_row(
        "SELECT last_seen_scan FROM directories WHERE path = ?1",
        [&parent],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO directories (location_id, path, parent, name, depth, complete, modified_at, last_seen_scan)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, strftime('%s','now'), ?6)",
        params![loc, dst, parent, name, depth, last_seen],
    )?;
    adjust_ancestors(conn, Some(parent), 0, 0, 1, false)?;
    Ok(())
}

pub fn after_rmdir(conn: &Connection, dst: &str) -> AppResult<()> {
    if conn.execute("DELETE FROM directories WHERE path = ?1", [dst])? > 0 {
        adjust_ancestors(conn, parent_str(dst), 0, 0, -1, false)?;
    }
    Ok(())
}
