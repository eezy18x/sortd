//! Filesystem scanner.
//!
//! Design notes:
//! * Streaming depth-first traversal (`walkdir`); the directory tree is never
//!   materialized in memory. Only per-directory aggregates are kept, which is
//!   proportional to the number of directories, not files.
//! * Symbolic links (and Windows junctions) are recorded but never followed.
//! * Mount boundaries are respected unless the user opts in.
//! * Files are written in batched transactions; each scan stamps rows with
//!   its id so unchanged hash caches survive and vanished files are removed.
//! * Permission errors and vanished files are counted, sampled and reported;
//!   affected directories are marked incomplete so sizes are never claimed
//!   to be exact when they are not.
//! * Cancellation is cooperative and checked on every entry.
//! * The scanner is read-only: it never modifies, moves or deletes files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::categorization::{classify, Category};
use crate::error::{AppError, AppResult};
use crate::filesystem::{self, EntryInfo};
use crate::security::Location;
use crate::settings::{CategoryRule, Settings};
use crate::util::{extension_of, now, path_str};

const BATCH_SIZE: usize = 1000;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);
const MAX_ERROR_SAMPLES: usize = 50;

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub include_hidden: bool,
    pub cross_filesystems: bool,
    pub excluded_paths: Vec<PathBuf>,
    pub excluded_names: Vec<String>,
    pub category_rules: Vec<CategoryRule>,
}

impl ScanOptions {
    pub fn from_settings(s: &Settings) -> Self {
        let (excluded_paths, excluded_names) = s.exclusions();
        ScanOptions {
            include_hidden: s.include_hidden,
            cross_filesystems: s.cross_filesystems,
            excluded_paths,
            excluded_names,
            category_rules: s.category_rules.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanIssue {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub scan_id: i64,
    pub location_id: i64,
    pub root: String,
    /// "walking", "finalizing" or "done".
    pub phase: String,
    /// Terminal status once `phase == "done"`.
    pub status: Option<String>,
    pub files_seen: u64,
    pub dirs_seen: u64,
    pub bytes_seen: u64,
    pub errors: u64,
    pub skipped: u64,
    pub current_path: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub scan_id: i64,
    pub location_id: i64,
    pub status: String,
    pub files_seen: u64,
    pub dirs_seen: u64,
    pub bytes_seen: u64,
    pub files_added: u64,
    pub files_updated: u64,
    pub files_removed: u64,
    pub errors: u64,
    pub skipped: u64,
    pub issues: Vec<ScanIssue>,
    pub elapsed_ms: u64,
}

struct FileRow {
    path: String,
    parent: String,
    name: String,
    extension: Option<String>,
    info: EntryInfo,
    category: Category,
    confidence: f64,
    mime: Option<String>,
}

#[derive(Default)]
struct DirAgg {
    parent: Option<String>,
    name: String,
    depth: usize,
    own_size: u64,
    own_files: u64,
    total_size: u64,
    total_files: u64,
    total_dirs: u64,
    complete: bool,
    hidden: bool,
    modified: Option<i64>,
}

/// Marks scans left in `running` state by a crash or forced quit.
pub fn mark_interrupted_scans(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE scans SET status = 'interrupted', finished_at = ?1,
             message = COALESCE(message, 'SORTD exited before the scan finished')
         WHERE status = 'running'",
        [now()],
    )?)
}

struct Exclusions {
    paths: Vec<PathBuf>,
    names: Vec<String>,
}

impl Exclusions {
    fn new(root: &Path, opts: &ScanOptions) -> Self {
        // A system path the user explicitly picked as the scan root (or an
        // ancestor of it) is authorized, so it is not excluded.
        let paths = opts
            .excluded_paths
            .iter()
            .filter(|p| !filesystem::path_starts_with(root, p))
            .cloned()
            .collect();
        let names = opts
            .excluded_names
            .iter()
            .map(|n| filesystem::fold(n))
            .collect();
        Exclusions { paths, names }
    }

    fn is_excluded(&self, path: &Path) -> bool {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if self.names.iter().any(|n| *n == filesystem::fold(name)) {
                return true;
            }
        }
        self.paths
            .iter()
            .any(|p| filesystem::path_starts_with(path, p))
    }
}

/// Scans one location. Blocking; run it on a background thread.
pub fn run_scan(
    conn: &mut Connection,
    location: &Location,
    opts: &ScanOptions,
    cancel: &AtomicBool,
    on_progress: &dyn Fn(&ScanProgress),
) -> AppResult<ScanSummary> {
    let root = PathBuf::from(&location.path);
    let started = Instant::now();
    let meta = std::fs::metadata(&root)
        .map_err(|e| AppError::NotFound(format!("{} is not accessible: {e}", root.display())))?;
    if !meta.is_dir() {
        return Err(AppError::invalid(format!(
            "{} is not a folder",
            root.display()
        )));
    }

    conn.execute(
        "INSERT INTO scans(location_id, root, started_at, status) VALUES (?1, ?2, ?3, 'running')",
        params![location.id, location.path, now()],
    )?;
    let scan_id = conn.last_insert_rowid();

    let mut progress = ScanProgress {
        scan_id,
        location_id: location.id,
        root: location.path.clone(),
        phase: "walking".into(),
        ..Default::default()
    };
    on_progress(&progress);

    let result = walk(
        conn,
        location,
        &root,
        opts,
        cancel,
        scan_id,
        &mut progress,
        started,
        on_progress,
    );

    let (status, issues, message) = match &result {
        Ok((status, issues)) => (status.clone(), issues.clone(), None),
        Err(e) => ("failed".to_string(), Vec::new(), Some(e.to_string())),
    };

    let mut summary = ScanSummary {
        scan_id,
        location_id: location.id,
        status: status.clone(),
        files_seen: progress.files_seen,
        dirs_seen: progress.dirs_seen,
        bytes_seen: progress.bytes_seen,
        files_added: 0,
        files_updated: 0,
        files_removed: 0,
        errors: progress.errors,
        skipped: progress.skipped,
        issues,
        elapsed_ms: started.elapsed().as_millis() as u64,
    };

    if status == "completed" {
        progress.phase = "finalizing".into();
        progress.current_path = None;
        on_progress(&progress);
        let (added, updated, removed) = finalize(conn, location.id, scan_id)?;
        summary.files_added = added;
        summary.files_updated = updated;
        summary.files_removed = removed;
    } else if status == "cancelled" {
        // Rows written so far are accurate; unchanged/removed detection is
        // skipped because the traversal did not see everything.
        let added: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE location_id = ?1 AND first_seen_scan = ?2",
            params![location.id, scan_id],
            |r| r.get(0),
        )?;
        summary.files_added = added as u64;
    }

    let message = message.or_else(|| {
        if summary.issues.is_empty() {
            None
        } else {
            serde_json::to_string(&summary.issues).ok()
        }
    });
    conn.execute(
        "UPDATE scans SET status = ?2, finished_at = ?3, files_seen = ?4, dirs_seen = ?5, bytes_seen = ?6,
            files_added = ?7, files_updated = ?8, files_removed = ?9, errors = ?10, skipped = ?11, message = ?12
         WHERE id = ?1",
        params![
            scan_id,
            status,
            now(),
            summary.files_seen as i64,
            summary.dirs_seen as i64,
            summary.bytes_seen as i64,
            summary.files_added as i64,
            summary.files_updated as i64,
            summary.files_removed as i64,
            summary.errors as i64,
            summary.skipped as i64,
            message
        ],
    )?;
    conn.execute(
        "UPDATE locations SET last_scan_id = ?1 WHERE id = ?2",
        params![scan_id, location.id],
    )?;

    progress.phase = "done".into();
    progress.status = Some(status);
    progress.elapsed_ms = summary.elapsed_ms;
    on_progress(&progress);

    result.map(|_| summary)
}

#[allow(clippy::too_many_arguments)]
fn walk(
    conn: &mut Connection,
    location: &Location,
    root: &Path,
    opts: &ScanOptions,
    cancel: &AtomicBool,
    scan_id: i64,
    progress: &mut ScanProgress,
    started: Instant,
    on_progress: &dyn Fn(&ScanProgress),
) -> AppResult<(String, Vec<ScanIssue>)> {
    let exclusions = Exclusions::new(root, opts);
    let mut issues: Vec<ScanIssue> = Vec::new();
    let mut dirs: HashMap<String, DirAgg> = HashMap::new();
    let mut batch: Vec<FileRow> = Vec::with_capacity(BATCH_SIZE);
    let mut last_emit = Instant::now();
    let include_hidden = opts.include_hidden;

    let note_issue = |issues: &mut Vec<ScanIssue>, path: &Path, error: String| {
        if issues.len() < MAX_ERROR_SAMPLES {
            issues.push(ScanIssue {
                path: path.display().to_string(),
                error,
            });
        }
    };

    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .same_file_system(!opts.cross_filesystems)
        .into_iter();
    let mut it = walker.filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        if exclusions.is_excluded(e.path()) {
            return false;
        }
        if !include_hidden {
            if let Ok(meta) = e.metadata() {
                if filesystem::entry_info(e.path(), &meta).is_hidden {
                    return false;
                }
            }
        }
        true
    });

    let mark_incomplete = |dirs: &mut HashMap<String, DirAgg>, path: &Path| {
        // Walk up to the nearest known directory.
        let mut p = Some(path);
        while let Some(cur) = p {
            if let Some(s) = path_str(cur) {
                if let Some(d) = dirs.get_mut(s) {
                    d.complete = false;
                    return;
                }
            }
            p = cur.parent();
        }
    };

    loop {
        if cancel.load(Ordering::Relaxed) {
            flush(conn, location.id, scan_id, &mut batch)?;
            return Ok(("cancelled".into(), issues));
        }
        let Some(next) = it.next() else { break };
        let entry = match next {
            Ok(e) => e,
            Err(err) => {
                let path = err
                    .path()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| root.to_path_buf());
                let kind = err.io_error().map(|io| io.kind());
                if kind == Some(std::io::ErrorKind::NotFound) {
                    // Disappeared during the scan: not an error.
                    progress.skipped += 1;
                    continue;
                }
                progress.errors += 1;
                let msg = if kind == Some(std::io::ErrorKind::PermissionDenied) {
                    "permission denied".to_string()
                } else {
                    err.to_string()
                };
                mark_incomplete(&mut dirs, &path);
                note_issue(&mut issues, &path, msg);
                continue;
            }
        };

        let path = entry.path();
        let Some(path_s) = path_str(path) else {
            progress.skipped += 1;
            mark_incomplete(&mut dirs, path.parent().unwrap_or(root));
            note_issue(
                &mut issues,
                path,
                "name is not valid Unicode; skipped".into(),
            );
            if entry.file_type().is_dir() {
                it.skip_current_dir();
            }
            continue;
        };

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(err) => {
                let vanished = err
                    .io_error()
                    .map(|e| e.kind() == std::io::ErrorKind::NotFound)
                    .unwrap_or(false);
                if vanished {
                    progress.skipped += 1;
                } else {
                    progress.errors += 1;
                    mark_incomplete(&mut dirs, path.parent().unwrap_or(root));
                    note_issue(&mut issues, path, err.to_string());
                }
                continue;
            }
        };
        let info = filesystem::entry_info(path, &meta);
        let parent_s = path.parent().and_then(path_str).map(str::to_string);

        if meta.is_dir() && !info.is_symlink {
            if filesystem::is_traversal_boundary(&meta) {
                it.skip_current_dir();
                progress.skipped += 1;
                continue;
            }
            progress.dirs_seen += 1;
            dirs.insert(
                path_s.to_string(),
                DirAgg {
                    parent: if entry.depth() == 0 { None } else { parent_s },
                    name: entry.file_name().to_string_lossy().into_owned(),
                    depth: entry.depth(),
                    complete: true,
                    hidden: info.is_hidden,
                    modified: info.modified,
                    ..Default::default()
                },
            );
        } else {
            // Regular files and symlinks (recorded, never followed).
            let name = entry.file_name().to_string_lossy().into_owned();
            let extension = extension_of(path);
            let c = classify(&name, extension.as_deref(), &opts.category_rules);
            progress.files_seen += 1;
            progress.bytes_seen += info.size;
            if let Some(parent) = parent_s.as_deref().and_then(|p| dirs.get_mut(p)) {
                parent.own_size += info.size;
                parent.own_files += 1;
            }
            batch.push(FileRow {
                path: path_s.to_string(),
                parent: parent_s.unwrap_or_default(),
                name,
                extension,
                info,
                category: c.category,
                confidence: c.confidence,
                mime: c.mime_hint,
            });
            if batch.len() >= BATCH_SIZE {
                flush(conn, location.id, scan_id, &mut batch)?;
            }
        }

        if last_emit.elapsed() >= PROGRESS_INTERVAL {
            progress.current_path = Some(path_s.to_string());
            progress.elapsed_ms = started.elapsed().as_millis() as u64;
            on_progress(progress);
            last_emit = Instant::now();
        }
    }
    flush(conn, location.id, scan_id, &mut batch)?;
    write_directories(conn, location.id, scan_id, dirs)?;
    Ok(("completed".into(), issues))
}

fn flush(
    conn: &mut Connection,
    location_id: i64,
    scan_id: i64,
    batch: &mut Vec<FileRow>,
) -> AppResult<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let ts = now();
    let tx = conn.transaction()?;
    {
        // Hash caches are kept only when size and mtime are unchanged.
        let mut stmt = tx.prepare_cached(
            "INSERT INTO files (location_id, path, parent, name, extension, size, created_at, modified_at,
                accessed_at, is_hidden, is_readonly, is_system, is_symlink, is_executable, is_placeholder,
                attributes, mime, category, category_confidence, last_seen_scan, first_seen_scan,
                changed_scan, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?20, ?20, ?21)
             ON CONFLICT(path) DO UPDATE SET
                location_id = excluded.location_id,
                parent = excluded.parent,
                name = excluded.name,
                extension = excluded.extension,
                created_at = excluded.created_at,
                accessed_at = excluded.accessed_at,
                is_hidden = excluded.is_hidden,
                is_readonly = excluded.is_readonly,
                is_system = excluded.is_system,
                is_symlink = excluded.is_symlink,
                is_executable = excluded.is_executable,
                is_placeholder = excluded.is_placeholder,
                attributes = excluded.attributes,
                mime = excluded.mime,
                category = excluded.category,
                category_confidence = excluded.category_confidence,
                last_seen_scan = excluded.last_seen_scan,
                changed_scan = CASE WHEN files.size = excluded.size AND files.modified_at IS excluded.modified_at
                                    THEN files.changed_scan ELSE excluded.changed_scan END,
                partial_hash = CASE WHEN files.size = excluded.size AND files.modified_at IS excluded.modified_at
                                    THEN files.partial_hash ELSE NULL END,
                full_hash = CASE WHEN files.size = excluded.size AND files.modified_at IS excluded.modified_at
                                 THEN files.full_hash ELSE NULL END,
                indexed_at = CASE WHEN files.size = excluded.size AND files.modified_at IS excluded.modified_at
                                  THEN files.indexed_at ELSE excluded.indexed_at END,
                size = excluded.size,
                modified_at = excluded.modified_at",
        )?;
        for f in batch.iter() {
            stmt.execute(params![
                location_id,
                f.path,
                f.parent,
                f.name,
                f.extension,
                f.info.size as i64,
                f.info.created,
                f.info.modified,
                f.info.accessed,
                f.info.is_hidden,
                f.info.is_readonly,
                f.info.is_system,
                f.info.is_symlink,
                f.info.is_executable,
                f.info.is_placeholder,
                f.info.raw_attributes,
                f.mime,
                f.category.as_str(),
                f.confidence,
                scan_id,
                ts,
            ])?;
        }
    }
    tx.commit()?;
    batch.clear();
    Ok(())
}

fn write_directories(
    conn: &mut Connection,
    location_id: i64,
    scan_id: i64,
    mut dirs: HashMap<String, DirAgg>,
) -> AppResult<()> {
    // Propagate totals bottom-up: deepest directories first.
    let mut order: Vec<(usize, String)> = dirs.iter().map(|(k, v)| (v.depth, k.clone())).collect();
    order.sort_by_key(|o| std::cmp::Reverse(o.0));
    for (_, key) in &order {
        let (parent, size, files, sub_dirs, complete) = {
            let d = dirs.get_mut(key).expect("present");
            d.total_size += d.own_size;
            d.total_files += d.own_files;
            (
                d.parent.clone(),
                d.total_size,
                d.total_files,
                d.total_dirs,
                d.complete,
            )
        };
        if let Some(p) = parent.and_then(|p| dirs.get_mut(&p)) {
            p.total_size += size;
            p.total_files += files;
            p.total_dirs += sub_dirs + 1;
            p.complete &= complete;
        }
    }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO directories (location_id, path, parent, name, depth, own_size, own_files, total_size,
                total_files, total_dirs, complete, is_hidden, modified_at, last_seen_scan)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(path) DO UPDATE SET
                location_id = excluded.location_id, parent = excluded.parent, name = excluded.name,
                depth = excluded.depth, own_size = excluded.own_size, own_files = excluded.own_files,
                total_size = excluded.total_size, total_files = excluded.total_files,
                total_dirs = excluded.total_dirs, complete = excluded.complete,
                is_hidden = excluded.is_hidden, modified_at = excluded.modified_at,
                last_seen_scan = excluded.last_seen_scan",
        )?;
        for (path, d) in &dirs {
            stmt.execute(params![
                location_id,
                path,
                d.parent,
                d.name,
                d.depth as i64,
                d.own_size as i64,
                d.own_files as i64,
                d.total_size as i64,
                d.total_files as i64,
                d.total_dirs as i64,
                d.complete,
                d.hidden,
                d.modified,
                scan_id
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Removes index rows for files and directories not seen by a completed
/// scan, and computes add/update/remove counts.
fn finalize(conn: &mut Connection, location_id: i64, scan_id: i64) -> AppResult<(u64, u64, u64)> {
    let tx = conn.transaction()?;
    let removed = tx.execute(
        "DELETE FROM files WHERE location_id = ?1 AND last_seen_scan != ?2",
        params![location_id, scan_id],
    )?;
    tx.execute(
        "DELETE FROM directories WHERE location_id = ?1 AND last_seen_scan != ?2",
        params![location_id, scan_id],
    )?;
    let added: i64 = tx.query_row(
        "SELECT COUNT(*) FROM files WHERE location_id = ?1 AND first_seen_scan = ?2",
        params![location_id, scan_id],
        |r| r.get(0),
    )?;
    let updated: i64 = tx.query_row(
        "SELECT COUNT(*) FROM files WHERE location_id = ?1 AND changed_scan = ?2 AND first_seen_scan != ?2",
        params![location_id, scan_id],
        |r| r.get(0),
    )?;
    tx.commit()?;
    Ok((added as u64, updated as u64, removed as u64))
}

#[cfg(test)]
mod tests;
