//! Storage analytics computed from the index.
//!
//! All figures describe *indexed* data. Physical disk capacity is reported
//! separately per volume and is never conflated with indexed totals.
//! Directories whose subtree could not be fully read are flagged
//! `complete = false`; their sizes are lower bounds.

use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use crate::error::AppResult;
use crate::filesystem::{self, DiskSpace};
use crate::indexing::{
    self, CategoryStat, DirectoryRecord, FileFilter, FileRecord, SortField, SortOrder,
};
use crate::util::now;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgeBucket {
    pub label: String,
    pub files: i64,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStat {
    pub extension: String,
    pub files: i64,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeStat {
    pub location_id: i64,
    pub path: String,
    pub indexed_size: i64,
    pub indexed_files: i64,
    pub complete: bool,
    /// Physical capacity of the volume holding this location, when readable.
    pub disk: Option<DiskSpace>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageAnalysis {
    pub indexed_size: i64,
    pub indexed_files: i64,
    pub indexed_directories: i64,
    pub categories: Vec<CategoryStat>,
    pub largest_directories: Vec<DirectoryRecord>,
    pub largest_files: Vec<FileRecord>,
    pub age_buckets: Vec<AgeBucket>,
    pub extensions: Vec<ExtensionStat>,
    pub duplicate_groups: i64,
    pub duplicate_waste: i64,
    pub volumes: Vec<VolumeStat>,
    pub incomplete_directories: i64,
}

const DAY: i64 = 86_400;

pub fn analyze(conn: &Connection, location_id: Option<i64>) -> AppResult<StorageAnalysis> {
    let (indexed_files, indexed_size): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM files WHERE (?1 IS NULL OR location_id = ?1)",
        [location_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (indexed_directories, incomplete_directories): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(complete = 0), 0) FROM directories WHERE (?1 IS NULL OR location_id = ?1)",
        [location_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    // Largest directories below the location roots. A directory's total
    // already includes its children, so this list naturally nests.
    let mut stmt = conn.prepare(
        "SELECT d.path, d.name, d.parent, d.location_id, d.depth, d.own_size, d.own_files, d.total_size,
                d.total_files, d.total_dirs, d.complete, d.is_hidden, d.modified_at
         FROM directories d WHERE d.depth > 0 AND (?1 IS NULL OR d.location_id = ?1)
         ORDER BY d.total_size DESC LIMIT 15",
    )?;
    let largest_directories = stmt
        .query_map([location_id], |r| {
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
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let filter = FileFilter {
        location_id,
        ..Default::default()
    };
    let largest_files =
        indexing::query_files(conn, &filter, SortField::Size, SortOrder::Desc, 15, 0)?.items;

    let t = now();
    let bounds: [(&str, Option<i64>, Option<i64>); 6] = [
        ("Last 30 days", Some(t - 30 * DAY), None),
        ("1–6 months", Some(t - 182 * DAY), Some(t - 30 * DAY)),
        ("6–12 months", Some(t - 365 * DAY), Some(t - 182 * DAY)),
        ("1–2 years", Some(t - 730 * DAY), Some(t - 365 * DAY)),
        ("2–5 years", Some(t - 1826 * DAY), Some(t - 730 * DAY)),
        ("Over 5 years", None, Some(t - 1826 * DAY)),
    ];
    let mut age_buckets = Vec::new();
    for (label, from, to) in bounds {
        let (files, size): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM files
             WHERE (?1 IS NULL OR location_id = ?1)
               AND modified_at IS NOT NULL
               AND (?2 IS NULL OR modified_at >= ?2)
               AND (?3 IS NULL OR modified_at < ?3)",
            rusqlite::params![location_id, from, to],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        age_buckets.push(AgeBucket {
            label: label.into(),
            files,
            size,
        });
    }
    let (files, size): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM files WHERE (?1 IS NULL OR location_id = ?1) AND modified_at IS NULL",
        [location_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if files > 0 {
        age_buckets.push(AgeBucket {
            label: "Unknown".into(),
            files,
            size,
        });
    }

    let mut stmt = conn.prepare(
        "SELECT COALESCE(extension, ''), COUNT(*), COALESCE(SUM(size), 0) FROM files
         WHERE (?1 IS NULL OR location_id = ?1) GROUP BY extension ORDER BY 3 DESC LIMIT 12",
    )?;
    let extensions = stmt
        .query_map([location_id], |r| {
            Ok(ExtensionStat {
                extension: r.get(0)?,
                files: r.get(1)?,
                size: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let (duplicate_groups, duplicate_waste) = crate::duplicates::totals(conn)?;

    let volumes = crate::security::list_locations(conn)?
        .into_iter()
        .filter(|l| l.kind == crate::security::LocationKind::Scan)
        .filter(|l| location_id.is_none() || location_id == Some(l.id))
        .map(|l| {
            let complete = indexing::get_directory(conn, &l.path)
                .ok()
                .flatten()
                .map(|d| d.complete)
                .unwrap_or(false);
            VolumeStat {
                location_id: l.id,
                indexed_size: l.total_size,
                indexed_files: l.file_count,
                complete,
                disk: filesystem::disk_space(Path::new(&l.path)).ok(),
                path: l.path,
            }
        })
        .collect();

    Ok(StorageAnalysis {
        indexed_size,
        indexed_files,
        indexed_directories,
        categories: indexing::category_stats(conn, location_id)?,
        largest_directories,
        largest_files,
        age_buckets,
        extensions,
        duplicate_groups,
        duplicate_waste,
        volumes,
        incomplete_directories,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_db;
    use crate::scanner::{run_scan, ScanOptions};
    use crate::security::{add_location, LocationKind};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn analysis_matches_fixture() {
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        std::fs::create_dir_all(r.join("videos")).unwrap();
        std::fs::write(r.join("videos/clip.mp4"), vec![0u8; 4000]).unwrap();
        std::fs::write(r.join("doc.pdf"), vec![0u8; 1000]).unwrap();
        db.with(|c| {
            let loc = add_location(c, r, LocationKind::Scan)?;
            run_scan(
                c,
                &loc,
                &ScanOptions::default(),
                &AtomicBool::new(false),
                &|_| {},
            )?;
            let a = analyze(c, None)?;
            assert_eq!(a.indexed_size, 5000);
            assert_eq!(a.indexed_files, 2);
            assert_eq!(a.largest_directories[0].name, "videos");
            assert_eq!(a.largest_directories[0].total_size, 4000);
            assert_eq!(a.categories[0].category, "videos");
            // Freshly written files land in the newest age bucket.
            assert_eq!(a.age_buckets[0].files, 2);
            assert_eq!(a.age_buckets.iter().map(|b| b.size).sum::<i64>(), 5000);
            assert_eq!(a.volumes.len(), 1);
            assert!(a.volumes[0].complete);
            let disk = a.volumes[0].disk.expect("disk space readable");
            assert!(disk.total >= 5000);
            Ok(())
        })
        .unwrap();
    }
}
