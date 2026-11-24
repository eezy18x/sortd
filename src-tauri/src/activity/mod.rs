//! Persistent activity history.
//!
//! Entries record *what happened* (kind, summary, paths and counts). File
//! contents are never written to the log.

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::AppResult;
use crate::util::now_millis;

/// Activity kinds. Kept as string constants so the frontend can filter.
pub mod kind {
    pub const LOCATION_ADDED: &str = "location_added";
    pub const LOCATION_REMOVED: &str = "location_removed";
    pub const SCAN_STARTED: &str = "scan_started";
    pub const SCAN_COMPLETED: &str = "scan_completed";
    pub const SCAN_CANCELLED: &str = "scan_cancelled";
    pub const SCAN_FAILED: &str = "scan_failed";
    pub const DUPLICATES_COMPLETED: &str = "duplicates_completed";
    pub const RECOMMENDATIONS_GENERATED: &str = "recommendations_generated";
    pub const OPERATION_PREVIEWED: &str = "operation_previewed";
    pub const OPERATION_EXECUTED: &str = "operation_executed";
    pub const OPERATION_DRY_RUN: &str = "operation_dry_run";
    pub const FILE_MOVED: &str = "file_moved";
    pub const FILE_RENAMED: &str = "file_renamed";
    pub const FILE_COPIED: &str = "file_copied";
    pub const FILE_TRASHED: &str = "file_trashed";
    pub const DIRECTORY_CREATED: &str = "directory_created";
    pub const OPERATION_FAILED: &str = "operation_failed";
    pub const UNDO: &str = "undo";
    pub const CATEGORY_OVERRIDDEN: &str = "category_overridden";
    pub const SETTINGS_CHANGED: &str = "settings_changed";
    pub const RECOVERY: &str = "recovery";
    pub const INDEX_RESET: &str = "index_reset";
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: i64,
    pub ts_ms: i64,
    pub kind: String,
    pub summary: String,
    pub details: Option<serde_json::Value>,
    pub batch_id: Option<i64>,
}

pub fn record(
    conn: &Connection,
    kind: &str,
    summary: &str,
    details: Option<serde_json::Value>,
    batch_id: Option<i64>,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO activity(ts_ms, kind, summary, details, batch_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            now_millis(),
            kind,
            summary,
            details.map(|d| d.to_string()),
            batch_id
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(
    conn: &Connection,
    kinds: &[String],
    limit: u32,
    offset: u32,
) -> AppResult<(Vec<ActivityEntry>, i64)> {
    let filter = if kinds.is_empty() {
        String::new()
    } else {
        format!("WHERE kind IN ({})", vec!["?"; kinds.len()].join(","))
    };
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM activity {filter}"),
        rusqlite::params_from_iter(kinds.iter()),
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(&format!(
        "SELECT id, ts_ms, kind, summary, details, batch_id FROM activity {filter}
         ORDER BY ts_ms DESC, id DESC LIMIT {} OFFSET {}",
        limit.min(500),
        offset
    ))?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(kinds.iter()), |r| {
            let details: Option<String> = r.get(4)?;
            Ok(ActivityEntry {
                id: r.get(0)?,
                ts_ms: r.get(1)?,
                kind: r.get(2)?,
                summary: r.get(3)?,
                details: details.and_then(|d| serde_json::from_str(&d).ok()),
                batch_id: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((rows, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_db;

    #[test]
    fn records_and_filters() {
        let (_d, db) = test_db();
        db.with(|c| {
            record(c, kind::SCAN_STARTED, "Scan started", None, None)?;
            record(
                c,
                kind::FILE_MOVED,
                "Moved a.txt",
                Some(serde_json::json!({"from": "/a", "to": "/b"})),
                Some(1),
            )?;
            let (all, total) = list(c, &[], 50, 0)?;
            assert_eq!(total, 2);
            assert_eq!(all[0].kind, kind::FILE_MOVED);
            let (moved, t) = list(c, &[kind::FILE_MOVED.to_string()], 50, 0)?;
            assert_eq!(t, 1);
            assert_eq!(moved[0].details.as_ref().unwrap()["to"], "/b");
            Ok(())
        })
        .unwrap();
    }
}
