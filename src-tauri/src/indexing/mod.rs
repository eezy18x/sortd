//! Read-side queries over the persistent index: file records, directory
//! listings, filtered file queries (large/recent/category views) and the
//! overview dashboard.

use std::path::Path;

use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::categorization::Category;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    pub id: i64,
    pub location_id: i64,
    pub path: String,
    pub parent: String,
    pub name: String,
    pub extension: Option<String>,
    pub size: i64,
    pub created_at: Option<i64>,
    pub modified_at: Option<i64>,
    pub accessed_at: Option<i64>,
    pub is_hidden: bool,
    pub is_readonly: bool,
    pub is_symlink: bool,
    pub is_placeholder: bool,
    pub mime: Option<String>,
    /// Effective category (user override if set).
    pub category: String,
    pub category_overridden: bool,
    pub category_confidence: f64,
    pub full_hash: Option<String>,
}

pub const FILE_COLUMNS: &str = "f.id, f.location_id, f.path, f.parent, f.name, f.extension, f.size, f.created_at,
    f.modified_at, f.accessed_at, f.is_hidden, f.is_readonly, f.is_symlink, f.is_placeholder, f.mime,
    COALESCE(f.category_override, f.category), f.category_override IS NOT NULL, f.category_confidence, f.full_hash";

pub fn file_from_row(r: &Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: r.get(0)?,
        location_id: r.get(1)?,
        path: r.get(2)?,
        parent: r.get(3)?,
        name: r.get(4)?,
        extension: r.get(5)?,
        size: r.get(6)?,
        created_at: r.get(7)?,
        modified_at: r.get(8)?,
        accessed_at: r.get(9)?,
        is_hidden: r.get(10)?,
        is_readonly: r.get(11)?,
        is_symlink: r.get(12)?,
        is_placeholder: r.get(13)?,
        mime: r.get(14)?,
        category: r.get(15)?,
        category_overridden: r.get(16)?,
        category_confidence: r.get(17)?,
        full_hash: r.get(18)?,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub offset: u32,
    pub limit: u32,
}

pub fn get_file(conn: &Connection, id: i64) -> AppResult<FileRecord> {
    conn.query_row(
        &format!("SELECT {FILE_COLUMNS} FROM files f WHERE f.id = ?1"),
        [id],
        file_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("file {id} is not in the index")))
}

pub fn get_file_by_path(conn: &Connection, path: &str) -> AppResult<Option<FileRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {FILE_COLUMNS} FROM files f WHERE f.path = ?1"),
            [path],
            file_from_row,
        )
        .optional()?)
}

// ---------------------------------------------------------------------------
// Filtered queries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileFilter {
    pub min_size: Option<i64>,
    pub max_size: Option<i64>,
    pub categories: Vec<String>,
    pub extensions: Vec<String>,
    pub location_id: Option<i64>,
    /// Restrict to files below this directory (recursive).
    pub directory: Option<String>,
    pub modified_after: Option<i64>,
    pub modified_before: Option<i64>,
    pub created_after: Option<i64>,
    pub created_before: Option<i64>,
    pub include_hidden: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SortField {
    Name,
    #[default]
    Size,
    Modified,
    Created,
    Relevance,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    #[default]
    Desc,
}

/// Accumulates SQL `WHERE` clauses with bound parameters.
#[derive(Default)]
pub struct Where {
    pub clauses: Vec<String>,
    pub params: Vec<Value>,
}

impl Where {
    pub fn push(&mut self, clause: impl Into<String>, values: impl IntoIterator<Item = Value>) {
        self.clauses.push(clause.into());
        self.params.extend(values);
    }

    pub fn sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.clauses.join(" AND "))
        }
    }
}

impl FileFilter {
    pub fn validate(&self) -> AppResult<()> {
        for c in &self.categories {
            if Category::parse(c).is_none() {
                return Err(AppError::invalid(format!("unknown category: {c}")));
            }
        }
        if self.categories.len() > 20 || self.extensions.len() > 100 {
            return Err(AppError::invalid("too many filter values"));
        }
        Ok(())
    }

    pub fn apply(&self, w: &mut Where) {
        if let Some(v) = self.min_size {
            w.push("f.size >= ?", [Value::Integer(v)]);
        }
        if let Some(v) = self.max_size {
            w.push("f.size <= ?", [Value::Integer(v)]);
        }
        if !self.categories.is_empty() {
            let marks = vec!["?"; self.categories.len()].join(",");
            w.push(
                format!("COALESCE(f.category_override, f.category) IN ({marks})"),
                self.categories.iter().map(|c| Value::Text(c.clone())),
            );
        }
        if !self.extensions.is_empty() {
            let marks = vec!["?"; self.extensions.len()].join(",");
            w.push(
                format!("f.extension IN ({marks})"),
                self.extensions
                    .iter()
                    .map(|e| Value::Text(e.trim_start_matches('.').to_ascii_lowercase())),
            );
        }
        if let Some(id) = self.location_id {
            w.push("f.location_id = ?", [Value::Integer(id)]);
        }
        if let Some(dir) = &self.directory {
            push_under_directory(w, "f.path", dir);
        }
        if let Some(v) = self.modified_after {
            w.push("f.modified_at >= ?", [Value::Integer(v)]);
        }
        if let Some(v) = self.modified_before {
            w.push("f.modified_at <= ?", [Value::Integer(v)]);
        }
        if let Some(v) = self.created_after {
            w.push("f.created_at >= ?", [Value::Integer(v)]);
        }
        if let Some(v) = self.created_before {
            w.push("f.created_at <= ?", [Value::Integer(v)]);
        }
        if self.include_hidden == Some(false) {
            w.push("f.is_hidden = 0", []);
        }
    }
}

/// Adds an exact (case-sensitive, wildcard-free) "path is below `dir`" clause.
pub fn push_under_directory(w: &mut Where, column: &str, dir: &str) {
    let sep = std::path::MAIN_SEPARATOR;
    let mut prefix = dir.to_string();
    if !prefix.ends_with(sep) {
        prefix.push(sep);
    }
    let len = prefix.chars().count() as i64;
    w.push(
        format!("substr({column}, 1, ?) = ?"),
        [Value::Integer(len), Value::Text(prefix)],
    );
}

pub fn order_by(sort: SortField, order: SortOrder) -> &'static str {
    use SortField::*;
    use SortOrder::*;
    match (sort, order) {
        (Name, Asc) => "f.name COLLATE NOCASE ASC",
        (Name, Desc) => "f.name COLLATE NOCASE DESC",
        (Size, Asc) => "f.size ASC, f.id",
        (Size, Desc) | (Relevance, _) => "f.size DESC, f.id",
        (Modified, Asc) => "f.modified_at ASC, f.id",
        (Modified, Desc) => "f.modified_at DESC, f.id",
        (Created, Asc) => "f.created_at ASC, f.id",
        (Created, Desc) => "f.created_at DESC, f.id",
    }
}

pub fn clamp_limit(limit: u32) -> u32 {
    limit.clamp(1, 1000)
}

pub fn query_files(
    conn: &Connection,
    filter: &FileFilter,
    sort: SortField,
    order: SortOrder,
    limit: u32,
    offset: u32,
) -> AppResult<Page<FileRecord>> {
    filter.validate()?;
    let mut w = Where::default();
    filter.apply(&mut w);
    let limit = clamp_limit(limit);
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM files f {}", w.sql()),
        rusqlite::params_from_iter(w.params.iter()),
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {FILE_COLUMNS} FROM files f {} ORDER BY {} LIMIT {limit} OFFSET {offset}",
        w.sql(),
        order_by(sort, order)
    ))?;
    let items = stmt
        .query_map(rusqlite::params_from_iter(w.params.iter()), file_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Page {
        items,
        total,
        offset,
        limit,
    })
}

// ---------------------------------------------------------------------------
// Directory listing (explorer)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryRecord {
    pub path: String,
    pub name: String,
    pub parent: Option<String>,
    pub location_id: i64,
    pub depth: i64,
    pub own_size: i64,
    pub own_files: i64,
    pub total_size: i64,
    pub total_files: i64,
    pub total_dirs: i64,
    pub complete: bool,
    pub is_hidden: bool,
    pub modified_at: Option<i64>,
}

const DIR_COLUMNS: &str =
    "d.path, d.name, d.parent, d.location_id, d.depth, d.own_size, d.own_files, d.total_size,
    d.total_files, d.total_dirs, d.complete, d.is_hidden, d.modified_at";

fn dir_from_row(r: &Row<'_>) -> rusqlite::Result<DirectoryRecord> {
    Ok(DirectoryRecord {
        path: r.get(0)?,
        name: r.get(1)?,
        parent: r.get(2)?,
        location_id: r.get(3)?,
        depth: r.get(4)?,
        own_size: r.get(5)?,
        own_files: r.get(6)?,
        total_size: r.get(7)?,
        total_files: r.get(8)?,
        total_dirs: r.get(9)?,
        complete: r.get(10)?,
        is_hidden: r.get(11)?,
        modified_at: r.get(12)?,
    })
}

pub fn get_directory(conn: &Connection, path: &str) -> AppResult<Option<DirectoryRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {DIR_COLUMNS} FROM directories d WHERE d.path = ?1"),
            [path],
            dir_from_row,
        )
        .optional()?)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breadcrumb {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub directory: DirectoryRecord,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub subdirectories: Vec<DirectoryRecord>,
    pub files: Page<FileRecord>,
}

pub fn list_directory(
    conn: &Connection,
    path: &str,
    sort: SortField,
    order: SortOrder,
    limit: u32,
    offset: u32,
) -> AppResult<DirectoryListing> {
    let directory = get_directory(conn, path)?.ok_or_else(|| {
        AppError::NotFound("this folder is not in the index; scan its location first".into())
    })?;

    // Breadcrumbs from the location root down to this directory.
    let mut breadcrumbs = Vec::new();
    let mut cur = Some(directory.clone());
    while let Some(d) = cur {
        breadcrumbs.push(Breadcrumb {
            name: d.name.clone(),
            path: d.path.clone(),
        });
        cur = match &d.parent {
            Some(p) => get_directory(conn, p)?,
            None => None,
        };
        if breadcrumbs.len() > 512 {
            break;
        }
    }
    breadcrumbs.reverse();
    if let Some(first) = breadcrumbs.first_mut() {
        first.name = directory_display_name(&first.path);
    }

    let dir_order = match (sort, order) {
        (SortField::Name, SortOrder::Asc) => "d.name COLLATE NOCASE ASC",
        (SortField::Name, SortOrder::Desc) => "d.name COLLATE NOCASE DESC",
        (SortField::Modified, SortOrder::Asc) => "d.modified_at ASC",
        (SortField::Modified, SortOrder::Desc) => "d.modified_at DESC",
        (_, SortOrder::Asc) => "d.total_size ASC",
        _ => "d.total_size DESC",
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT {DIR_COLUMNS} FROM directories d WHERE d.parent = ?1 ORDER BY {dir_order} LIMIT 5000"
    ))?;
    let subdirectories = stmt
        .query_map([path], dir_from_row)?
        .collect::<Result<Vec<_>, _>>()?;

    let limit = clamp_limit(limit);
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files f WHERE f.parent = ?1",
        [path],
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {FILE_COLUMNS} FROM files f WHERE f.parent = ?1 ORDER BY {} LIMIT {limit} OFFSET {offset}",
        order_by(sort, order)
    ))?;
    let items = stmt
        .query_map(params![path], file_from_row)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DirectoryListing {
        directory,
        breadcrumbs,
        subdirectories,
        files: Page {
            items,
            total,
            offset,
            limit,
        },
    })
}

/// A friendly name for a location root ("C:\" has no file name).
pub fn directory_display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryStat {
    pub category: String,
    pub files: i64,
    pub size: i64,
}

pub fn category_stats(conn: &Connection, location_id: Option<i64>) -> AppResult<Vec<CategoryStat>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(category_override, category) AS c, COUNT(*), COALESCE(SUM(size), 0)
         FROM files WHERE (?1 IS NULL OR location_id = ?1) GROUP BY c ORDER BY 3 DESC",
    )?;
    let rows = stmt
        .query_map([location_id], |r| {
            Ok(CategoryStat {
                category: r.get(0)?,
                files: r.get(1)?,
                size: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn set_category_override(
    conn: &Connection,
    file_id: i64,
    category: Option<Category>,
) -> AppResult<FileRecord> {
    let changed = conn.execute(
        "UPDATE files SET category_override = ?2 WHERE id = ?1",
        params![file_id, category.map(|c| c.as_str())],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound(format!(
            "file {file_id} is not in the index"
        )));
    }
    get_file(conn, file_id)
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecord {
    pub id: i64,
    pub location_id: i64,
    pub root: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: String,
    pub files_seen: i64,
    pub dirs_seen: i64,
    pub bytes_seen: i64,
    pub files_added: i64,
    pub files_updated: i64,
    pub files_removed: i64,
    pub errors: i64,
    pub skipped: i64,
    pub message: Option<String>,
}

pub fn list_scans(conn: &Connection, limit: u32) -> AppResult<Vec<ScanRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT id, location_id, root, started_at, finished_at, status, files_seen, dirs_seen, bytes_seen,
                files_added, files_updated, files_removed, errors, skipped, message
         FROM scans ORDER BY id DESC LIMIT {}",
        limit.min(500)
    ))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ScanRecord {
                id: r.get(0)?,
                location_id: r.get(1)?,
                root: r.get(2)?,
                started_at: r.get(3)?,
                finished_at: r.get(4)?,
                status: r.get(5)?,
                files_seen: r.get(6)?,
                dirs_seen: r.get(7)?,
                bytes_seen: r.get(8)?,
                files_added: r.get(9)?,
                files_updated: r.get(10)?,
                files_removed: r.get(11)?,
                errors: r.get(12)?,
                skipped: r.get(13)?,
                message: r.get(14)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub total_files: i64,
    pub total_size: i64,
    pub total_directories: i64,
    pub location_count: i64,
    pub hashed_files: i64,
    pub duplicate_groups: i64,
    pub duplicate_waste: i64,
    pub pending_recommendations: i64,
    pub incomplete_locations: i64,
    pub categories: Vec<CategoryStat>,
    pub largest_files: Vec<FileRecord>,
    pub recent_files: Vec<FileRecord>,
    pub recent_scans: Vec<ScanRecord>,
}

pub fn overview(conn: &Connection) -> AppResult<Overview> {
    let (total_files, total_size, hashed_files): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(size), 0), COALESCE(SUM(full_hash IS NOT NULL), 0) FROM files",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let total_directories: i64 =
        conn.query_row("SELECT COUNT(*) FROM directories", [], |r| r.get(0))?;
    let location_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM locations WHERE kind = 'scan'",
        [],
        |r| r.get(0),
    )?;
    let incomplete_locations: i64 = conn.query_row(
        "SELECT COUNT(*) FROM locations l JOIN directories d ON d.path = l.path WHERE d.complete = 0",
        [],
        |r| r.get(0),
    )?;
    let (duplicate_groups, duplicate_waste) = crate::duplicates::totals(conn)?;
    let pending_recommendations: i64 = conn.query_row(
        "SELECT COUNT(*) FROM recommendations WHERE status = 'pending'",
        [],
        |r| r.get(0),
    )?;
    let categories = category_stats(conn, None)?;
    let largest_files = query_files(
        conn,
        &FileFilter::default(),
        SortField::Size,
        SortOrder::Desc,
        6,
        0,
    )?
    .items;
    let recent_files = query_files(
        conn,
        &FileFilter::default(),
        SortField::Modified,
        SortOrder::Desc,
        6,
        0,
    )?
    .items;
    let recent_scans = list_scans(conn, 5)?;
    Ok(Overview {
        total_files,
        total_size,
        total_directories,
        location_count,
        hashed_files,
        duplicate_groups,
        duplicate_waste,
        pending_recommendations,
        incomplete_locations,
        categories,
        largest_files,
        recent_files,
        recent_scans,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_db;
    use crate::scanner::{run_scan, ScanOptions};
    use crate::security::{add_location, LocationKind};
    use std::sync::atomic::AtomicBool;

    fn setup() -> (
        tempfile::TempDir,
        crate::database::Database,
        tempfile::TempDir,
        String,
    ) {
        let (d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        std::fs::create_dir_all(r.join("a/b")).unwrap();
        std::fs::write(r.join("a/big.iso"), vec![0u8; 5000]).unwrap();
        std::fs::write(r.join("a/b/small.txt"), b"tiny").unwrap();
        std::fs::write(r.join("photo.png"), vec![0u8; 100]).unwrap();
        let loc_path = db
            .with(|c| {
                let loc = add_location(c, r, LocationKind::Scan)?;
                run_scan(
                    c,
                    &loc,
                    &ScanOptions::default(),
                    &AtomicBool::new(false),
                    &|_| {},
                )?;
                Ok(loc.path)
            })
            .unwrap();
        (d, db, root, loc_path)
    }

    #[test]
    fn list_directory_returns_children_and_breadcrumbs() {
        let (_d, db, _root, loc) = setup();
        db.with(|c| {
            let l = list_directory(c, &loc, SortField::Size, SortOrder::Desc, 100, 0)?;
            assert_eq!(l.subdirectories.len(), 1);
            assert_eq!(l.files.total, 1);
            assert_eq!(l.directory.total_size, 5104);
            let a = &l.subdirectories[0].path;
            let la = list_directory(c, a, SortField::Name, SortOrder::Asc, 100, 0)?;
            assert_eq!(la.breadcrumbs.len(), 2);
            assert_eq!(la.files.items[0].name, "big.iso");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn filters_and_sorting() {
        let (_d, db, _root, loc) = setup();
        db.with(|c| {
            let big = query_files(
                c,
                &FileFilter {
                    min_size: Some(1000),
                    ..Default::default()
                },
                SortField::Size,
                SortOrder::Desc,
                10,
                0,
            )?;
            assert_eq!(big.total, 1);
            assert_eq!(big.items[0].name, "big.iso");
            let images = query_files(
                c,
                &FileFilter {
                    categories: vec!["images".into()],
                    ..Default::default()
                },
                SortField::Name,
                SortOrder::Asc,
                10,
                0,
            )?;
            assert_eq!(images.total, 1);
            let sub = Path::new(&loc).join("a").to_string_lossy().into_owned();
            let under = query_files(
                c,
                &FileFilter {
                    directory: Some(sub),
                    ..Default::default()
                },
                SortField::Size,
                SortOrder::Asc,
                10,
                0,
            )?;
            assert_eq!(under.total, 2);
            assert_eq!(under.items[0].name, "small.txt");
            assert!(query_files(
                c,
                &FileFilter {
                    categories: vec!["bogus".into()],
                    ..Default::default()
                },
                SortField::Size,
                SortOrder::Asc,
                10,
                0
            )
            .is_err());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn directory_prefix_does_not_match_siblings_with_same_prefix() {
        let (_d, db, root, loc) = setup();
        std::fs::create_dir_all(root.path().join("ab")).unwrap();
        std::fs::write(root.path().join("ab/other.txt"), b"x").unwrap();
        db.with(|c| {
            let l = crate::security::list_locations(c)?.remove(0);
            run_scan(
                c,
                &l,
                &ScanOptions::default(),
                &AtomicBool::new(false),
                &|_| {},
            )?;
            let sub = Path::new(&loc).join("a").to_string_lossy().into_owned();
            let under = query_files(
                c,
                &FileFilter {
                    directory: Some(sub),
                    ..Default::default()
                },
                SortField::Size,
                SortOrder::Asc,
                10,
                0,
            )?;
            assert_eq!(under.total, 2);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn category_override_changes_effective_category() {
        let (_d, db, _root, _loc) = setup();
        db.with(|c| {
            let id: i64 =
                c.query_row("SELECT id FROM files WHERE name = 'photo.png'", [], |r| {
                    r.get(0)
                })?;
            let f = set_category_override(c, id, Some(Category::Design))?;
            assert_eq!(f.category, "design");
            assert!(f.category_overridden);
            let f = set_category_override(c, id, None)?;
            assert_eq!(f.category, "images");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn overview_uses_real_index_data() {
        let (_d, db, _root, _loc) = setup();
        db.with(|c| {
            let o = overview(c)?;
            assert_eq!(o.total_files, 3);
            assert_eq!(o.total_size, 5104);
            assert_eq!(o.location_count, 1);
            assert_eq!(o.largest_files[0].name, "big.iso");
            assert_eq!(o.recent_scans.len(), 1);
            Ok(())
        })
        .unwrap();
    }
}
