//! Execution, dry run, undo and crash recovery for planned batches.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{params, Connection};
use serde::Serialize;

use super::{fsops, get_batch, index_sync, Batch, Op};
use crate::activity::{self, kind};
use crate::error::{AppError, AppResult};
use crate::filesystem;
use crate::security;
use crate::util::{now, now_millis, to_unix};

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgress {
    pub batch_id: i64,
    /// "running", "undoing" or "done".
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub current: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
struct ItemRow {
    id: i64,
    op: Op,
    source: Option<PathBuf>,
    destination: Option<PathBuf>,
    cross_volume: bool,
    is_dir: bool,
    size: Option<i64>,
    mtime: Option<i64>,
    content_hash: Option<String>,
}

fn load_items(
    conn: &Connection,
    batch_id: i64,
    status: &str,
    reverse: bool,
) -> AppResult<Vec<ItemRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT id, op, source, destination, cross_volume, is_dir, source_size, source_mtime, content_hash
         FROM operation_items WHERE batch_id = ?1 AND status = ?2 ORDER BY seq {}",
        if reverse { "DESC" } else { "ASC" }
    ))?;
    let rows = stmt
        .query_map(params![batch_id, status], |r| {
            Ok(ItemRow {
                id: r.get(0)?,
                op: Op::parse(&r.get::<_, String>(1)?).unwrap_or(Op::Move),
                source: r.get::<_, Option<String>>(2)?.map(PathBuf::from),
                destination: r.get::<_, Option<String>>(3)?.map(PathBuf::from),
                cross_volume: r.get(4)?,
                is_dir: r.get(5)?,
                size: r.get(6)?,
                mtime: r.get(7)?,
                content_hash: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn batch_meta(conn: &Connection, batch_id: i64) -> AppResult<(String, bool, String)> {
    conn.query_row(
        "SELECT status, allow_cross_volume, label FROM operation_batches WHERE id = ?1",
        [batch_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .map_err(|_| AppError::NotFound(format!("operation batch {batch_id} not found")))
}

/// Verifies that a file still matches the snapshot taken at plan time.
fn matches_snapshot(path: &Path, size: Option<i64>, mtime: Option<i64>) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("{} is no longer accessible: {e}", path.display()))?;
    if let Some(s) = size {
        if meta.len() as i64 != s {
            return Err(format!(
                "{} changed since the preview (size differs)",
                path.display()
            ));
        }
    }
    if let Some(m) = mtime {
        if meta.modified().ok().and_then(to_unix) != Some(m) {
            return Err(format!(
                "{} changed since the preview (modified time differs)",
                path.display()
            ));
        }
    }
    Ok(())
}

/// The pre-execution check, run immediately before each item (and by the
/// dry run). `virtual_dirs` holds folders that earlier items will create.
fn precheck(
    conn: &Connection,
    it: &ItemRow,
    allow_cross_volume: bool,
    virtual_dirs: &HashSet<String>,
) -> Result<(), String> {
    if let Some(src) = &it.source {
        security::authorize_path(conn, src).map_err(|e| e.to_string())?;
        matches_snapshot(src, if it.is_dir { None } else { it.size }, it.mtime)?;
    }
    if let Some(dst) = &it.destination {
        security::authorize_path(conn, dst).map_err(|e| e.to_string())?;
        let case_only = it
            .source
            .as_deref()
            .map(|s| s != dst.as_path() && filesystem::paths_equal(s, dst))
            .unwrap_or(false);
        if !case_only && std::fs::symlink_metadata(dst).is_ok() {
            return Err(format!(
                "{} already exists (it appeared after the preview)",
                dst.display()
            ));
        }
        let parent = dst.parent().ok_or("invalid destination")?;
        let parent_ok = std::fs::metadata(parent)
            .map(|m| m.is_dir())
            .unwrap_or(false)
            || virtual_dirs.contains(&filesystem::fold(&parent.to_string_lossy()));
        if !parent_ok {
            return Err(format!(
                "destination folder {} does not exist",
                parent.display()
            ));
        }
    }
    if it.op == Op::Move && it.cross_volume && !allow_cross_volume {
        return Err("cross-drive moves were not authorized for this batch".into());
    }
    Ok(())
}

fn perform(
    it: &ItemRow,
    batch_id: i64,
    allow_cross_volume: bool,
) -> std::io::Result<Option<String>> {
    let src = it.source.as_deref();
    let dst = it.destination.as_deref();
    let temp = |d: &Path| fsops::temp_path(d, batch_id, it.id);
    match (it.op, src, dst) {
        (Op::Mkdir, _, Some(d)) => {
            std::fs::create_dir(d)?;
            Ok(None)
        }
        (Op::Rename, Some(s), Some(d)) => {
            fsops::rename(s, d, &temp(d))?;
            Ok(None)
        }
        (Op::Move, Some(s), Some(d)) => {
            if !it.cross_volume {
                match filesystem::rename_no_replace(s, d) {
                    Ok(()) => return Ok(None),
                    Err(e)
                        if filesystem::is_cross_device(&e) && allow_cross_volume && !it.is_dir => {}
                    Err(e) if filesystem::is_cross_device(&e) => {
                        return Err(std::io::Error::other(
                            "source and destination are on different drives",
                        ))
                    }
                    Err(e) => return Err(e),
                }
            }
            cross_volume_move(s, d, &temp(d), it.size, it.mtime).map(Some)
        }
        (Op::Copy, Some(s), Some(d)) => fsops::verified_copy(s, d, &temp(d)).map(Some),
        (Op::Trash, Some(s), _) => {
            fsops::trash(s)?;
            Ok(None)
        }
        _ => Err(std::io::Error::other("malformed operation")),
    }
}

/// Verified copy, then removal of the original. If the original cannot be
/// removed, the copy is removed again so the user is left with exactly one
/// file, as before.
pub(crate) fn cross_volume_move(
    src: &Path,
    dst: &Path,
    temp: &Path,
    size: Option<i64>,
    mtime: Option<i64>,
) -> std::io::Result<String> {
    let hash = fsops::verified_copy(src, dst, temp)?;
    let rollback = |e: String| {
        let _ = std::fs::remove_file(dst);
        std::io::Error::other(e)
    };
    if let Err(e) = matches_snapshot(src, size, mtime) {
        return Err(rollback(format!(
            "the original changed during the copy; copy discarded ({e})"
        )));
    }
    if let Err(e) = std::fs::remove_file(src) {
        return Err(rollback(format!(
            "the copy was verified but the original could not be removed ({e}); copy discarded"
        )));
    }
    Ok(hash)
}

fn verify_after(it: &ItemRow) -> Result<(), String> {
    match it.op {
        Op::Mkdir => {
            let d = it.destination.as_deref().ok_or("missing destination")?;
            if !d.is_dir() {
                return Err("the folder was not created".into());
            }
        }
        Op::Move | Op::Rename | Op::Copy => {
            let d = it.destination.as_deref().ok_or("missing destination")?;
            let meta = std::fs::symlink_metadata(d)
                .map_err(|_| "the destination is missing after the operation".to_string())?;
            if !it.is_dir {
                if let Some(s) = it.size {
                    if meta.is_file() && meta.len() as i64 != s {
                        return Err("the destination size does not match the original".into());
                    }
                }
            }
            if it.op != Op::Copy {
                let s = it.source.as_deref().ok_or("missing source")?;
                let case_only = filesystem::paths_equal(s, d);
                if !case_only && std::fs::symlink_metadata(s).is_ok() {
                    return Err("the original is still present at its old location".into());
                }
            }
        }
        Op::Trash => {}
    }
    Ok(())
}

fn item_activity(op: Op) -> &'static str {
    match op {
        Op::Move => kind::FILE_MOVED,
        Op::Rename => kind::FILE_RENAMED,
        Op::Copy => kind::FILE_COPIED,
        Op::Trash => kind::FILE_TRASHED,
        Op::Mkdir => kind::DIRECTORY_CREATED,
    }
}

fn describe(it: &ItemRow) -> String {
    let name = |p: &Option<PathBuf>| {
        p.as_deref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    match it.op {
        Op::Move => format!("Moved {}", name(&it.source)),
        Op::Rename => format!("Renamed {} to {}", name(&it.source), name(&it.destination)),
        Op::Copy => format!("Copied {}", name(&it.source)),
        Op::Trash => format!("Moved {} to the trash", name(&it.source)),
        Op::Mkdir => format!("Created folder {}", name(&it.destination)),
    }
}

fn paths_json(it: &ItemRow) -> serde_json::Value {
    serde_json::json!({
        "op": it.op.as_str(),
        "source": it.source.as_deref().map(|p| p.display().to_string()),
        "destination": it.destination.as_deref().map(|p| p.display().to_string()),
    })
}

fn sync_index(conn: &Connection, it: &ItemRow) -> AppResult<()> {
    let s = it.source.as_deref().and_then(|p| p.to_str());
    let d = it.destination.as_deref().and_then(|p| p.to_str());
    match (it.op, s, d) {
        (Op::Move | Op::Rename, Some(s), Some(d)) => index_sync::after_move(conn, s, d, it.is_dir),
        (Op::Copy, Some(s), Some(d)) => index_sync::after_copy(conn, s, d),
        (Op::Trash, Some(s), _) => index_sync::after_trash(conn, s, it.is_dir),
        (Op::Mkdir, _, Some(d)) => index_sync::after_mkdir(conn, d),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunItem {
    pub item_id: i64,
    pub ok: bool,
    pub message: Option<String>,
}

/// Runs every pre-execution check without changing anything.
pub fn dry_run(conn: &Connection, batch_id: i64) -> AppResult<Vec<DryRunItem>> {
    let (status, allow, label) = batch_meta(conn, batch_id)?;
    if status != "planned" {
        return Err(AppError::Conflict(
            "only plans that have not been executed can be dry-run".into(),
        ));
    }
    let mut virtual_dirs = HashSet::new();
    let mut out = Vec::new();
    for it in load_items(conn, batch_id, "pending", false)? {
        let r = precheck(conn, &it, allow, &virtual_dirs);
        if r.is_ok() && it.op == Op::Mkdir {
            if let Some(d) = &it.destination {
                virtual_dirs.insert(filesystem::fold(&d.to_string_lossy()));
            }
        }
        out.push(DryRunItem {
            item_id: it.id,
            ok: r.is_ok(),
            message: r.err(),
        });
    }
    let failures = out.iter().filter(|i| !i.ok).count();
    activity::record(
        conn,
        kind::OPERATION_DRY_RUN,
        &format!(
            "Dry run of “{label}”: {} ready, {failures} would fail",
            out.len() - failures
        ),
        Some(serde_json::json!({ "batchId": batch_id, "items": out.len(), "failures": failures })),
        Some(batch_id),
    )?;
    Ok(out)
}

/// Executes a planned batch after explicit user approval.
pub fn execute(
    conn: &mut Connection,
    batch_id: i64,
    cancel: &AtomicBool,
    on_progress: &dyn Fn(&OperationProgress),
) -> AppResult<Batch> {
    let (_, allow, label) = batch_meta(conn, batch_id)?;
    // Atomically claim the batch so it can never run twice.
    let claimed = conn.execute(
        "UPDATE operation_batches SET status = 'running', executed_at = ?2 WHERE id = ?1 AND status = 'planned'",
        params![batch_id, now()],
    )?;
    if claimed == 0 {
        return Err(AppError::Conflict(
            "this plan has already been executed or discarded".into(),
        ));
    }
    let items = load_items(conn, batch_id, "pending", false)?;
    let mut p = OperationProgress {
        batch_id,
        phase: "running".into(),
        total: items.len() as u64,
        ..Default::default()
    };
    on_progress(&p);

    let none = HashSet::new();
    for it in &items {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        p.current = it
            .source
            .as_deref()
            .or(it.destination.as_deref())
            .map(|x| x.display().to_string());
        on_progress(&p);

        // Journal first: if SORTD dies mid-operation, recovery knows.
        conn.execute(
            "UPDATE operation_items SET status = 'running', started_ms = ?2 WHERE id = ?1",
            params![it.id, now_millis()],
        )?;
        let outcome = precheck(conn, it, allow, &none)
            .and_then(|()| perform(it, batch_id, allow).map_err(|e| e.to_string()))
            .and_then(|hash| verify_after(it).map(|()| hash));
        match outcome {
            Ok(hash) => {
                let tx = conn.transaction()?;
                tx.execute(
                    "UPDATE operation_items SET status = 'done', verified = 1, content_hash = ?2, finished_ms = ?3 WHERE id = ?1",
                    params![it.id, hash, now_millis()],
                )?;
                if let Err(e) = sync_index(&tx, it) {
                    // The filesystem change succeeded; an index hiccup must
                    // not misreport it. A rescan will reconcile.
                    tx.execute(
                        "UPDATE operation_items SET issue = ?2 WHERE id = ?1",
                        params![it.id, format!("index update failed: {e}")],
                    )?;
                }
                activity::record(
                    &tx,
                    item_activity(it.op),
                    &describe(it),
                    Some(paths_json(it)),
                    Some(batch_id),
                )?;
                tx.commit()?;
            }
            Err(msg) => {
                conn.execute(
                    "UPDATE operation_items SET status = 'failed', error = ?2, finished_ms = ?3 WHERE id = ?1",
                    params![it.id, msg, now_millis()],
                )?;
                let mut details = paths_json(it);
                details["error"] = serde_json::Value::String(msg.clone());
                activity::record(
                    conn,
                    kind::OPERATION_FAILED,
                    &format!("Failed: {}", describe(it)),
                    Some(details),
                    Some(batch_id),
                )?;
            }
        }
        p.done += 1;
    }

    // Anything not reached (cancellation) is recorded as skipped.
    conn.execute(
        "UPDATE operation_items SET status = 'skipped', issue = 'cancelled before execution' WHERE batch_id = ?1 AND status = 'pending'",
        [batch_id],
    )?;
    let status = finalize_counts(conn, batch_id)?;
    let b = get_batch(conn, batch_id)?;
    activity::record(
        conn,
        kind::OPERATION_EXECUTED,
        &format!(
            "Executed “{label}”: {} succeeded, {} failed, {} skipped",
            b.summary.succeeded, b.summary.failed, b.summary.skipped
        ),
        Some(serde_json::json!({ "batchId": batch_id, "status": status })),
        Some(batch_id),
    )?;
    p.phase = "done".into();
    p.status = Some(status);
    p.current = None;
    on_progress(&p);
    Ok(b)
}

fn finalize_counts(conn: &Connection, batch_id: i64) -> AppResult<String> {
    let (done, failed, skipped, total): (i64, i64, i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(status = 'done'), 0), COALESCE(SUM(status IN ('failed', 'interrupted')), 0),
                COALESCE(SUM(status = 'skipped'), 0), COUNT(*)
         FROM operation_items WHERE batch_id = ?1",
        [batch_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let status = if failed == 0 && done + skipped == total {
        "completed"
    } else if done == 0 {
        "failed"
    } else {
        "partial"
    };
    conn.execute(
        "UPDATE operation_batches SET status = ?2, succeeded = ?3, failed = ?4, skipped = ?5, finished_at = ?6 WHERE id = ?1",
        params![batch_id, status, done, failed, skipped, now()],
    )?;
    Ok(status.into())
}

/// Reverses completed items, newest first, when they are still exactly as
/// SORTD left them. Items moved to the trash must be restored from the
/// system trash and are reported, not reversed.
pub fn undo(
    conn: &mut Connection,
    batch_id: i64,
    on_progress: &dyn Fn(&OperationProgress),
) -> AppResult<Batch> {
    let (status, _, label) = batch_meta(conn, batch_id)?;
    if !matches!(status.as_str(), "completed" | "partial" | "interrupted") {
        return Err(AppError::Conflict("this batch cannot be undone".into()));
    }
    let items: Vec<ItemRow> = load_items(conn, batch_id, "done", true)?
        .into_iter()
        .filter(|i| i.op != Op::Trash)
        .collect();
    if items.is_empty() {
        return Err(AppError::Conflict(
            "there is nothing in this batch that can be undone".into(),
        ));
    }
    let mut p = OperationProgress {
        batch_id,
        phase: "undoing".into(),
        total: items.len() as u64,
        ..Default::default()
    };
    let (mut undone, mut failed) = (0u64, 0u64);
    for it in &items {
        p.current = it.destination.as_deref().map(|x| x.display().to_string());
        on_progress(&p);
        let result = undo_item(conn, it, batch_id);
        let tx = conn.transaction()?;
        match &result {
            Ok(()) => {
                undone += 1;
                tx.execute(
                    "UPDATE operation_items SET status = 'undone', finished_ms = ?2 WHERE id = ?1",
                    params![it.id, now_millis()],
                )?;
                let reversed = ItemRow {
                    source: it.destination.clone(),
                    destination: it.source.clone(),
                    ..it.clone()
                };
                let sync = match it.op {
                    Op::Move | Op::Rename => sync_index(&tx, &reversed),
                    Op::Copy => index_sync::after_trash(
                        &tx,
                        it.destination
                            .as_deref()
                            .and_then(|p| p.to_str())
                            .unwrap_or_default(),
                        false,
                    ),
                    Op::Mkdir => index_sync::after_rmdir(
                        &tx,
                        it.destination
                            .as_deref()
                            .and_then(|p| p.to_str())
                            .unwrap_or_default(),
                    ),
                    Op::Trash => Ok(()),
                };
                if let Err(e) = sync {
                    tx.execute(
                        "UPDATE operation_items SET issue = ?2 WHERE id = ?1",
                        params![it.id, format!("index update failed: {e}")],
                    )?;
                }
            }
            Err(msg) => {
                failed += 1;
                tx.execute(
                    "UPDATE operation_items SET status = 'undo_failed', error = ?2 WHERE id = ?1",
                    params![it.id, msg],
                )?;
            }
        }
        tx.commit()?;
        p.done += 1;
    }
    if failed == 0 {
        conn.execute(
            "UPDATE operation_batches SET status = 'undone' WHERE id = ?1",
            [batch_id],
        )?;
    }
    activity::record(
        conn,
        kind::UNDO,
        &format!("Undid “{label}”: {undone} reversed, {failed} could not be reversed"),
        Some(serde_json::json!({ "batchId": batch_id, "undone": undone, "failed": failed })),
        Some(batch_id),
    )?;
    p.phase = "done".into();
    p.status = Some(if failed == 0 {
        "undone".into()
    } else {
        "partial".into()
    });
    on_progress(&p);
    get_batch(conn, batch_id)
}

fn undo_item(conn: &Connection, it: &ItemRow, batch_id: i64) -> Result<(), String> {
    let dst = it.destination.as_deref().ok_or("missing destination")?;
    security::authorize_path(conn, dst).map_err(|e| e.to_string())?;
    match it.op {
        Op::Mkdir => std::fs::remove_dir(dst)
            .map_err(|e| format!("the folder could not be removed (is it empty?): {e}")),
        Op::Copy => {
            let expected = it
                .content_hash
                .as_deref()
                .ok_or("no verification hash recorded")?;
            let actual =
                fsops::hash(dst).map_err(|e| format!("the copy is not accessible: {e}"))?;
            if actual != expected {
                return Err("the copy was modified after it was made; it was left in place".into());
            }
            fsops::trash(dst).map_err(|e| e.to_string())
        }
        Op::Move | Op::Rename => {
            let src = it.source.as_deref().ok_or("missing source")?;
            security::authorize_path(conn, src).map_err(|e| e.to_string())?;
            matches_snapshot(
                dst,
                if it.is_dir { None } else { it.size },
                if it.is_dir { None } else { it.mtime },
            )
            .map_err(|e| format!("{e}; it was left in place"))?;
            let case_only = filesystem::paths_equal(src, dst);
            if !case_only && std::fs::symlink_metadata(src).is_ok() {
                return Err(format!(
                    "{} is occupied again; nothing was changed",
                    src.display()
                ));
            }
            let parent = src.parent().ok_or("invalid original location")?;
            if !parent.is_dir() {
                return Err(format!(
                    "the original folder {} no longer exists",
                    parent.display()
                ));
            }
            let temp = fsops::temp_path(src, batch_id, -it.id);
            if it.cross_volume {
                cross_volume_move(dst, src, &temp, it.size, it.mtime)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            } else {
                fsops::rename(dst, src, &temp).map_err(|e| e.to_string())
            }
        }
        Op::Trash => Err("restore items from the system trash".into()),
    }
}

/// Reconciles batches and items that were running when SORTD last exited.
/// Returns the number of items reconciled.
pub fn recover(conn: &mut Connection) -> AppResult<usize> {
    // Plans never survive a restart: the filesystem may have changed since
    // they were previewed, so they must be previewed again.
    conn.execute(
        "UPDATE operation_batches SET status = 'discarded' WHERE status = 'planned'",
        [],
    )?;
    let batches: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM operation_batches WHERE status = 'running'")?;
        let rows = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut reconciled = 0;
    for batch_id in batches {
        for it in load_items(conn, batch_id, "running", false)? {
            reconciled += 1;
            let exists = |p: &Option<PathBuf>| {
                p.as_deref()
                    .map(|p| std::fs::symlink_metadata(p).is_ok())
                    .unwrap_or(false)
            };
            // Remove any leftover partial copy.
            if let Some(d) = &it.destination {
                let _ = std::fs::remove_file(fsops::temp_path(d, batch_id, it.id));
            }
            let (status, note) = match it.op {
                Op::Mkdir if exists(&it.destination) => ("done", "completed before SORTD exited"),
                Op::Copy if exists(&it.destination) => (
                    "done",
                    "completed before SORTD exited (copy verified before it became visible)",
                ),
                Op::Move | Op::Rename if exists(&it.destination) && !exists(&it.source) => {
                    ("done", "completed before SORTD exited")
                }
                Op::Move if exists(&it.destination) && exists(&it.source) => (
                    "interrupted",
                    "interrupted after copying; both the original and the copy were kept",
                ),
                Op::Trash if !exists(&it.source) => ("done", "completed before SORTD exited"),
                _ => (
                    "interrupted",
                    "interrupted before it took effect; nothing was changed",
                ),
            };
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE operation_items SET status = ?2, issue = ?3, finished_ms = ?4 WHERE id = ?1",
                params![it.id, status, note, now_millis()],
            )?;
            if status == "done" {
                let _ = sync_index(&tx, &it);
            }
            tx.commit()?;
        }
        conn.execute(
            "UPDATE operation_items SET status = 'skipped', issue = 'not started: SORTD exited' WHERE batch_id = ?1 AND status = 'pending'",
            [batch_id],
        )?;
        finalize_counts(conn, batch_id)?;
        conn.execute(
            "UPDATE operation_batches SET status = 'interrupted' WHERE id = ?1",
            [batch_id],
        )?;
        activity::record(
            conn,
            kind::RECOVERY,
            "Recovered an operation batch interrupted when SORTD exited",
            Some(serde_json::json!({ "batchId": batch_id })),
            Some(batch_id),
        )?;
    }
    Ok(reconciled)
}
