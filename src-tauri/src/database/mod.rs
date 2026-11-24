//! SQLite persistence: connection management, pragmas and migrations.
//!
//! The database runs in WAL mode so the UI can read while a scan or hashing
//! job writes. Each background job opens its own connection; IPC commands
//! borrow connections from a small pool.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Ordered list of migrations. Index + 1 is the schema version it produces.
const MIGRATIONS: &[&str] = &[include_str!("../../migrations/0001_initial.sql")];

pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

const POOL_MAX_IDLE: usize = 4;

pub struct Database {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
}

impl Database {
    /// Opens (creating if necessary) the database at `path` and applies any
    /// pending migrations.
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = open_connection(path)?;
        migrate(&conn)?;
        Ok(Database {
            path: path.to_path_buf(),
            idle: Mutex::new(vec![conn]),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens a fresh, dedicated connection (for long-running jobs).
    pub fn connect(&self) -> AppResult<Connection> {
        open_connection(&self.path)
    }

    /// Borrows a pooled connection for the duration of `f`.
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> AppResult<T>) -> AppResult<T> {
        let conn = self.idle.lock().map_err(|_| poisoned())?.pop();
        let mut conn = match conn {
            Some(c) => c,
            None => self.connect()?,
        };
        let result = f(&mut conn);
        // Never return a connection with an open transaction to the pool.
        if conn.is_autocommit() {
            if let Ok(mut idle) = self.idle.lock() {
                if idle.len() < POOL_MAX_IDLE {
                    idle.push(conn);
                }
            }
        }
        result
    }
}

fn poisoned() -> AppError {
    AppError::Internal("database pool lock poisoned".into())
}

/// Opens a connection with SORTD's standard pragmas.
pub fn open_connection(path: &Path) -> AppResult<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    Ok(conn)
}

pub fn configure(conn: &Connection) -> AppResult<()> {
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(())
}

/// Applies pending migrations inside a single transaction each.
pub fn migrate(conn: &Connection) -> AppResult<u32> {
    let current: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(AppError::Internal(format!(
            "the index was created by a newer version of SORTD (schema {current}, this build supports {SCHEMA_VERSION}); \
             update SORTD or reset the index in Settings → Database"
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = i as u32 + 1;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        tx.commit()?;
    }
    Ok(SCHEMA_VERSION)
}

/// Runs SQLite's integrity check. Returns an empty list when healthy.
pub fn integrity_check(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare("PRAGMA integrity_check")?;
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut problems: Vec<String> = rows.into_iter().filter(|r| r != "ok").collect();
    // Also verify the full-text index matches its content table.
    if let Err(e) = conn.execute(
        "INSERT INTO files_fts(files_fts) VALUES ('integrity-check')",
        [],
    ) {
        problems.push(format!("search index: {e}"));
    }
    Ok(problems)
}

/// Rebuilds the full-text index from the files table.
pub fn rebuild_search_index(conn: &Connection) -> AppResult<()> {
    conn.execute("INSERT INTO files_fts(files_fts) VALUES ('rebuild')", [])?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn test_db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join("sortd.db")).unwrap();
    (dir, db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let (_dir, db) = test_db();
        db.with(|c| {
            let v: u32 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            assert_eq!(v, SCHEMA_VERSION);
            assert_eq!(migrate(c)?, SCHEMA_VERSION);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn wal_mode_and_foreign_keys_enabled() {
        let (_dir, db) = test_db();
        db.with(|c| {
            let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            assert_eq!(mode.to_lowercase(), "wal");
            let fk: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
            assert_eq!(fk, 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn refuses_newer_schema() {
        let (_dir, db) = test_db();
        db.with(|c| {
            c.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION + 1))?;
            assert!(migrate(c).is_err());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn fresh_database_passes_integrity_check() {
        let (_dir, db) = test_db();
        db.with(|c| {
            assert!(integrity_check(c)?.is_empty());
            Ok(())
        })
        .unwrap();
    }
}
