//! Safe file operation engine.
//!
//! Lifecycle of every change SORTD makes to the filesystem:
//!
//! 1. **Plan** ([`plan`]) — requests are validated, authorized, expanded
//!    (missing destination folders become explicit `mkdir` items), checked
//!    for conflicts and persisted as a batch in the operation journal. A
//!    plan never touches the filesystem. The UI shows it as a preview.
//! 2. **Dry run** ([`execute::dry_run`]) — re-runs every pre-execution check
//!    against the live filesystem without changing anything.
//! 3. **Execute** ([`execute::execute`]) — only after explicit approval.
//!    Each item is marked `running` in the journal *before* it executes, is
//!    re-validated immediately before execution, and is verified after.
//!    Failures are isolated per item.
//! 4. **Undo** ([`execute::undo`]) — reverses completed items in reverse
//!    order when they are still in the state SORTD left them in.
//! 5. **Recovery** ([`execute::recover`]) — on startup, reconciles items
//!    that were `running` when SORTD exited unexpectedly.
//!
//! Guarantees: no silent overwrite (no-replace renames everywhere), copies
//! are verified by BLAKE3 before they become visible, cross-volume moves
//! require explicit authorization and remove the source only after a
//! verified copy. Filesystem operations are *not* transactional; the journal
//! records exactly what happened so nothing is ever ambiguous.

pub mod execute;
mod fsops;
mod index_sync;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::filesystem;
use crate::security;
use crate::util::{now, path_str};

pub const MAX_PLAN_ITEMS: usize = 10_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum OperationRequest {
    Move {
        source: String,
        destination_dir: String,
    },
    Rename {
        source: String,
        new_name: String,
    },
    Copy {
        source: String,
        destination_dir: String,
    },
    CreateDirectory {
        parent: String,
        name: String,
    },
    Trash {
        source: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConflictPolicy {
    /// Leave the item out of the batch.
    #[default]
    Skip,
    /// Pick a free name such as "report (1).pdf".
    Rename,
}

impl ConflictPolicy {
    fn as_str(self) -> &'static str {
        match self {
            ConflictPolicy::Skip => "skip",
            ConflictPolicy::Rename => "rename",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRequest {
    pub label: String,
    /// "manual", "organization" or "duplicates".
    pub origin: String,
    pub operations: Vec<OperationRequest>,
    #[serde(default)]
    pub conflict_policy: ConflictPolicy,
    #[serde(default)]
    pub allow_cross_volume: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Move,
    Rename,
    Copy,
    Mkdir,
    Trash,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Move => "move",
            Op::Rename => "rename",
            Op::Copy => "copy",
            Op::Mkdir => "mkdir",
            Op::Trash => "trash",
        }
    }
    pub fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "move" => Op::Move,
            "rename" => Op::Rename,
            "copy" => Op::Copy,
            "mkdir" => Op::Mkdir,
            "trash" => Op::Trash,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationItem {
    pub id: i64,
    pub seq: i64,
    pub op: Op,
    pub source: Option<String>,
    pub destination: Option<String>,
    pub status: String,
    /// Non-fatal note or the reason an item is skipped.
    pub issue: Option<String>,
    pub error: Option<String>,
    pub cross_volume: bool,
    pub is_directory: bool,
    pub size: Option<i64>,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    pub id: i64,
    pub label: String,
    pub origin: String,
    pub status: String,
    pub created_at: i64,
    pub executed_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub conflict_policy: String,
    pub allow_cross_volume: bool,
    pub total: i64,
    pub succeeded: i64,
    pub failed: i64,
    pub skipped: i64,
    pub can_undo: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub summary: BatchSummary,
    pub items: Vec<OperationItem>,
    pub pending: i64,
    pub conflicts: i64,
    pub cross_volume_items: i64,
    pub total_bytes: i64,
}

/// A fully resolved item, before it is written to the journal.
struct PlannedItem {
    op: Op,
    source: Option<PathBuf>,
    destination: Option<PathBuf>,
    status: &'static str,
    issue: Option<String>,
    cross_volume: bool,
    is_dir: bool,
    size: Option<i64>,
    mtime: Option<i64>,
}

impl PlannedItem {
    fn skipped(
        op: Op,
        source: Option<PathBuf>,
        destination: Option<PathBuf>,
        issue: impl Into<String>,
    ) -> Self {
        PlannedItem {
            op,
            source,
            destination,
            status: "skipped",
            issue: Some(issue.into()),
            cross_volume: false,
            is_dir: false,
            size: None,
            mtime: None,
        }
    }
}

/// Returns "name (n).ext" variants until one is free on disk and unclaimed in
/// this plan.
fn free_name(dir: &Path, name: &str, claimed: &HashSet<String>) -> Option<PathBuf> {
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (1..10_000)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|c| {
            std::fs::symlink_metadata(c).is_err()
                && !claimed.contains(&filesystem::fold(&c.to_string_lossy()))
        })
}

struct Planner<'a> {
    conn: &'a Connection,
    policy: ConflictPolicy,
    allow_cross_volume: bool,
    items: Vec<PlannedItem>,
    /// Destinations claimed by earlier items in this plan (case-folded).
    claimed: HashSet<String>,
    /// Directories that will be created by earlier mkdir items.
    planned_dirs: HashSet<String>,
}

impl<'a> Planner<'a> {
    fn key(p: &Path) -> String {
        filesystem::fold(&p.to_string_lossy())
    }

    fn exists_or_planned(&self, p: &Path) -> bool {
        std::fs::symlink_metadata(p).is_ok() || self.claimed.contains(&Self::key(p))
    }

    /// Ensures `dir` exists or is created by earlier items. Returns an error
    /// message when the directory cannot be created.
    fn ensure_dir(&mut self, dir: &Path) -> Result<(), String> {
        if self.planned_dirs.contains(&Self::key(dir)) {
            return Ok(());
        }
        match std::fs::symlink_metadata(dir) {
            Ok(m) if m.is_dir() => return Ok(()),
            Ok(_) => return Err(format!("{} exists and is not a folder", dir.display())),
            Err(_) => {}
        }
        security::authorize_path(self.conn, dir).map_err(|e| e.to_string())?;
        let parent = dir.parent().ok_or("cannot create a filesystem root")?;
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("invalid folder name")?;
        filesystem::validate_file_name(name)?;
        self.ensure_dir(parent)?;
        self.planned_dirs.insert(Self::key(dir));
        self.claimed.insert(Self::key(dir));
        self.items.push(PlannedItem {
            op: Op::Mkdir,
            source: None,
            destination: Some(dir.to_path_buf()),
            status: "pending",
            issue: Some("Created because the destination folder does not exist".into()),
            cross_volume: false,
            is_dir: true,
            size: None,
            mtime: None,
        });
        Ok(())
    }

    /// Resolves a destination, applying the conflict policy.
    fn resolve_destination(
        &mut self,
        dir: &Path,
        name: &str,
    ) -> Result<(PathBuf, Option<String>), String> {
        let dst = dir.join(name);
        if !self.exists_or_planned(&dst) {
            return Ok((dst, None));
        }
        match self.policy {
            ConflictPolicy::Skip => Err(format!("{} already exists", dst.display())),
            ConflictPolicy::Rename => {
                let alt = free_name(dir, name, &self.claimed).ok_or("no free name available")?;
                let note = format!(
                    "{} already exists; will be saved as {}",
                    name,
                    alt.file_name().unwrap_or_default().to_string_lossy()
                );
                Ok((alt, Some(note)))
            }
        }
    }

    #[allow(clippy::result_large_err)] // Err carries a fully-formed skipped item by design.
    fn source(&self, raw: &str, op: Op) -> Result<(PathBuf, std::fs::Metadata), PlannedItem> {
        let path = security::parse_untrusted_path(raw)
            .map_err(|e| PlannedItem::skipped(op, None, None, e.to_string()))?;
        let fail = |msg: String| PlannedItem::skipped(op, Some(path.clone()), None, msg);
        security::authorize_path(self.conn, &path).map_err(|e| fail(e.to_string()))?;
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| fail(format!("source is not accessible: {e}")))?;
        if self.claimed.contains(&Self::key(&path)) {
            return Err(fail("this item is already part of the plan".into()));
        }
        Ok((path, meta))
    }

    fn add(&mut self, req: &OperationRequest) {
        let item = match self.build(req) {
            Ok(Some(item)) | Err(item) => item,
            Ok(None) => return,
        };
        if item.status == "pending" {
            if let Some(d) = &item.destination {
                self.claimed.insert(Self::key(d));
            }
            if let Some(s) = &item.source {
                self.claimed.insert(Self::key(s));
            }
        }
        self.items.push(item);
    }

    #[allow(clippy::result_large_err)]
    fn build(&mut self, req: &OperationRequest) -> Result<Option<PlannedItem>, PlannedItem> {
        match req {
            OperationRequest::Move {
                source,
                destination_dir,
            }
            | OperationRequest::Copy {
                source,
                destination_dir,
            } => {
                let op = if matches!(req, OperationRequest::Move { .. }) {
                    Op::Move
                } else {
                    Op::Copy
                };
                let (src, meta) = self.source(source, op)?;
                let skip = |msg: String| PlannedItem::skipped(op, Some(src.clone()), None, msg);
                let dir = security::parse_untrusted_path(destination_dir)
                    .map_err(|e| skip(e.to_string()))?;
                security::authorize_path(self.conn, &dir).map_err(|e| skip(e.to_string()))?;
                let is_dir = meta.is_dir();
                if meta.file_type().is_symlink() && op == Op::Copy {
                    return Err(skip("symbolic links are not copied".into()));
                }
                if is_dir && op == Op::Copy {
                    return Err(skip("copying folders is not supported yet".into()));
                }
                if is_dir && filesystem::path_starts_with(&dir, &src) {
                    return Err(skip("a folder cannot be moved into itself".into()));
                }
                if op == Op::Move
                    && src
                        .parent()
                        .map(|p| filesystem::paths_equal(p, &dir))
                        .unwrap_or(false)
                {
                    return Err(skip("already in the destination folder".into()));
                }
                let name = src
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| skip("invalid file name".into()))?
                    .to_string();
                self.ensure_dir(&dir).map_err(skip)?;
                let (dst, note) = self.resolve_destination(&dir, &name).map_err(skip)?;
                let probe = nearest_existing(&dir);
                let cross_volume = !filesystem::same_volume(&src, &probe);
                if op == Op::Move && cross_volume {
                    if is_dir {
                        return Err(skip(
                            "moving folders between drives is not supported yet".into(),
                        ));
                    }
                    if !self.allow_cross_volume {
                        let mut it = skip("on a different drive: enable cross-drive moves to copy, verify and then remove the original".into());
                        it.destination = Some(dst);
                        it.cross_volume = true;
                        return Err(it);
                    }
                }
                Ok(Some(PlannedItem {
                    op,
                    source: Some(src),
                    destination: Some(dst),
                    status: "pending",
                    issue: note,
                    cross_volume,
                    is_dir,
                    size: (!is_dir).then_some(meta.len() as i64),
                    mtime: meta.modified().ok().and_then(crate::util::to_unix),
                }))
            }
            OperationRequest::Rename { source, new_name } => {
                let (src, meta) = self.source(source, Op::Rename)?;
                let skip =
                    |msg: String| PlannedItem::skipped(Op::Rename, Some(src.clone()), None, msg);
                let new_name = new_name.trim();
                filesystem::validate_file_name(new_name).map_err(skip)?;
                let parent = src
                    .parent()
                    .ok_or_else(|| skip("cannot rename a filesystem root".into()))?
                    .to_path_buf();
                let dst = parent.join(new_name);
                if dst == src {
                    return Err(skip("the name is unchanged".into()));
                }
                // A case-only rename on a case-insensitive filesystem refers
                // to the same file and is not a conflict.
                let case_only = filesystem::paths_equal(&dst, &src);
                if !case_only && self.exists_or_planned(&dst) {
                    return Err(skip(format!("{new_name} already exists in this folder")));
                }
                Ok(Some(PlannedItem {
                    op: Op::Rename,
                    source: Some(src),
                    destination: Some(dst),
                    status: "pending",
                    issue: None,
                    cross_volume: false,
                    is_dir: meta.is_dir(),
                    size: meta.is_file().then_some(meta.len() as i64),
                    mtime: meta.modified().ok().and_then(crate::util::to_unix),
                }))
            }
            OperationRequest::CreateDirectory { parent, name } => {
                let skip = |msg: String| PlannedItem::skipped(Op::Mkdir, None, None, msg);
                let parent =
                    security::parse_untrusted_path(parent).map_err(|e| skip(e.to_string()))?;
                let name = name.trim();
                filesystem::validate_file_name(name).map_err(skip)?;
                let dst = parent.join(name);
                security::authorize_path(self.conn, &dst).map_err(|e| skip(e.to_string()))?;
                if self.exists_or_planned(&dst) {
                    return Err(skip(format!("{name} already exists")));
                }
                self.ensure_dir(&parent).map_err(skip)?;
                Ok(Some(PlannedItem {
                    op: Op::Mkdir,
                    source: None,
                    destination: Some(dst),
                    status: "pending",
                    issue: None,
                    cross_volume: false,
                    is_dir: true,
                    size: None,
                    mtime: None,
                }))
            }
            OperationRequest::Trash { source } => {
                let (src, meta) = self.source(source, Op::Trash)?;
                // Never trash an authorized root itself.
                if security::list_locations(self.conn)
                    .map(|ls| {
                        ls.iter()
                            .any(|l| filesystem::paths_equal(Path::new(&l.path), &src))
                    })
                    .unwrap_or(true)
                {
                    return Err(PlannedItem::skipped(
                        Op::Trash,
                        Some(src),
                        None,
                        "an authorized location itself cannot be moved to the trash",
                    ));
                }
                Ok(Some(PlannedItem {
                    op: Op::Trash,
                    source: Some(src),
                    destination: None,
                    status: "pending",
                    issue: Some(if meta.is_dir() {
                        "Folder and all its contents".into()
                    } else {
                        "Moved to the system trash".into()
                    }),
                    cross_volume: false,
                    is_dir: meta.is_dir(),
                    size: meta.is_file().then_some(meta.len() as i64),
                    mtime: meta.modified().ok().and_then(crate::util::to_unix),
                }))
            }
        }
    }
}

fn nearest_existing(p: &Path) -> PathBuf {
    let mut cur = p;
    loop {
        if cur.exists() {
            return cur.to_path_buf();
        }
        match cur.parent() {
            Some(parent) => cur = parent,
            None => return p.to_path_buf(),
        }
    }
}

/// Validates, expands and persists a plan. Touches nothing on disk.
pub fn plan(conn: &mut Connection, req: &PlanRequest) -> AppResult<Batch> {
    if req.operations.is_empty() {
        return Err(AppError::invalid("nothing to do"));
    }
    if req.operations.len() > MAX_PLAN_ITEMS {
        return Err(AppError::invalid(format!(
            "a single batch is limited to {MAX_PLAN_ITEMS} operations"
        )));
    }
    if !matches!(
        req.origin.as_str(),
        "manual" | "organization" | "duplicates"
    ) {
        return Err(AppError::invalid("invalid origin"));
    }
    let label: String = req.label.chars().take(200).collect();

    let items = {
        let mut planner = Planner {
            conn,
            policy: req.conflict_policy,
            allow_cross_volume: req.allow_cross_volume,
            items: Vec::new(),
            claimed: HashSet::new(),
            planned_dirs: HashSet::new(),
        };
        for op in &req.operations {
            planner.add(op);
        }
        planner.items
    };

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO operation_batches(created_at, label, origin, status, conflict_policy, allow_cross_volume, total, skipped)
         VALUES (?1, ?2, ?3, 'planned', ?4, ?5, ?6, ?7)",
        params![
            now(),
            label,
            req.origin,
            req.conflict_policy.as_str(),
            req.allow_cross_volume,
            items.len() as i64,
            items.iter().filter(|i| i.status == "skipped").count() as i64
        ],
    )?;
    let batch_id = tx.last_insert_rowid();
    {
        let mut stmt = tx.prepare(
            "INSERT INTO operation_items(batch_id, seq, op, source, destination, status, issue, cross_volume, is_dir, source_size, source_mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        for (seq, it) in items.iter().enumerate() {
            stmt.execute(params![
                batch_id,
                seq as i64,
                it.op.as_str(),
                it.source.as_deref().and_then(path_str),
                it.destination.as_deref().and_then(path_str),
                it.status,
                it.issue,
                it.cross_volume,
                it.is_dir,
                it.size,
                it.mtime
            ])?;
        }
    }
    crate::activity::record(
        &tx,
        crate::activity::kind::OPERATION_PREVIEWED,
        &format!("Previewed “{label}” ({} items)", items.len()),
        Some(serde_json::json!({ "batchId": batch_id, "items": items.len() })),
        Some(batch_id),
    )?;
    tx.commit()?;
    get_batch(conn, batch_id)
}

const BATCH_COLUMNS: &str = "b.id, b.label, b.origin, b.status, b.created_at, b.executed_at, b.finished_at,
    b.conflict_policy, b.allow_cross_volume, b.total, b.succeeded, b.failed, b.skipped,
    EXISTS (SELECT 1 FROM operation_items i WHERE i.batch_id = b.id AND i.status = 'done' AND i.op != 'trash')";

fn summary_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<BatchSummary> {
    let status: String = r.get(3)?;
    let undoable: bool = r.get(13)?;
    Ok(BatchSummary {
        id: r.get(0)?,
        label: r.get(1)?,
        origin: r.get(2)?,
        can_undo: undoable && matches!(status.as_str(), "completed" | "partial" | "interrupted"),
        status,
        created_at: r.get(4)?,
        executed_at: r.get(5)?,
        finished_at: r.get(6)?,
        conflict_policy: r.get(7)?,
        allow_cross_volume: r.get(8)?,
        total: r.get(9)?,
        succeeded: r.get(10)?,
        failed: r.get(11)?,
        skipped: r.get(12)?,
    })
}

pub fn get_batch(conn: &Connection, id: i64) -> AppResult<Batch> {
    let summary = conn
        .query_row(
            &format!("SELECT {BATCH_COLUMNS} FROM operation_batches b WHERE b.id = ?1"),
            [id],
            summary_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("operation batch {id} not found")))?;
    let mut stmt = conn.prepare(
        "SELECT id, seq, op, source, destination, status, issue, error, cross_volume, source_size, verified, is_dir
         FROM operation_items WHERE batch_id = ?1 ORDER BY seq",
    )?;
    let items = stmt
        .query_map([id], |r| {
            let op: String = r.get(2)?;
            let size: Option<i64> = r.get(9)?;
            let op = Op::parse(&op).unwrap_or(Op::Move);
            Ok(OperationItem {
                id: r.get(0)?,
                seq: r.get(1)?,
                op,
                source: r.get(3)?,
                destination: r.get(4)?,
                status: r.get(5)?,
                issue: r.get(6)?,
                error: r.get(7)?,
                cross_volume: r.get(8)?,
                is_directory: r.get(11)?,
                size,
                verified: r.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let pending = items.iter().filter(|i| i.status == "pending").count() as i64;
    let conflicts = items
        .iter()
        .filter(|i| {
            i.issue
                .as_deref()
                .map(|s| s.contains("already exists"))
                .unwrap_or(false)
        })
        .count() as i64;
    let cross_volume_items = items.iter().filter(|i| i.cross_volume).count() as i64;
    let total_bytes = items
        .iter()
        .filter(|i| i.status == "pending")
        .filter_map(|i| i.size)
        .sum();
    Ok(Batch {
        summary,
        items,
        pending,
        conflicts,
        cross_volume_items,
        total_bytes,
    })
}

pub fn list_batches(
    conn: &Connection,
    limit: u32,
    offset: u32,
) -> AppResult<(Vec<BatchSummary>, i64)> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM operation_batches WHERE status NOT IN ('discarded', 'planned')",
        [],
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {BATCH_COLUMNS} FROM operation_batches b WHERE b.status NOT IN ('discarded', 'planned')
         ORDER BY b.id DESC LIMIT {} OFFSET {offset}",
        limit.clamp(1, 200)
    ))?;
    let rows = stmt
        .query_map([], summary_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((rows, total))
}

/// Discards a plan that was previewed but not executed.
pub fn discard(conn: &Connection, id: i64) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE operation_batches SET status = 'discarded' WHERE id = ?1 AND status = 'planned'",
        [id],
    )?;
    if n == 0 {
        return Err(AppError::Conflict(
            "only plans that have not been executed can be discarded".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
