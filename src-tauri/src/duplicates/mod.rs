//! Exact duplicate detection.
//!
//! Pipeline:
//! 1. **Size grouping** — only files sharing an exact byte size can be
//!    duplicates. Symlinks, empty files and cloud placeholders are excluded.
//! 2. **Partial hash** — BLAKE3 over the first and last 16 KiB (the whole file
//!    when it is ≤ 32 KiB, in which case this *is* the full hash).
//! 3. **Full hash** — streaming BLAKE3 over the full content, only for files
//!    whose size and partial hash both collide.
//! 4. **Groups** — files with identical full hashes (and sizes).
//!
//! Names are never used to decide duplication. Hashes are cached in the index
//! and invalidated by the scanner whenever size or mtime changes. Each file is
//! re-checked against its indexed size/mtime immediately before hashing; a
//! file that changed is skipped rather than hashed under stale metadata.
//!
//! This module never deletes or modifies files.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::indexing::{file_from_row, FileRecord, Page, FILE_COLUMNS};

const EDGE: u64 = 16 * 1024;
const READ_BUF: usize = 1024 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone)]
pub struct DuplicateOptions {
    pub min_size: u64,
    pub workers: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateProgress {
    /// "grouping", "partial", "full" or "done".
    pub phase: String,
    pub status: Option<String>,
    pub candidates: u64,
    pub to_hash: u64,
    pub hashed: u64,
    pub bytes_to_hash: u64,
    pub bytes_hashed: u64,
    pub changed: u64,
    pub errors: u64,
    pub groups: i64,
    pub reclaimable: i64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
struct Candidate {
    id: i64,
    path: String,
    size: i64,
    mtime: Option<i64>,
}

enum HashOutcome {
    Ok(String),
    Changed,
    Failed,
}

/// Hashes the first and last `EDGE` bytes. For small files this covers the
/// whole content and the result equals the full hash.
fn partial_hash(path: &Path, size: u64) -> std::io::Result<String> {
    let mut f = crate::filesystem::open_for_read(path)?;
    let mut h = blake3::Hasher::new();
    if size <= EDGE * 2 {
        let mut buf = Vec::with_capacity(size as usize);
        f.read_to_end(&mut buf)?;
        h.update(&buf);
    } else {
        let mut buf = vec![0u8; EDGE as usize];
        f.read_exact(&mut buf)?;
        h.update(&buf);
        f.seek(SeekFrom::End(-(EDGE as i64)))?;
        f.read_exact(&mut buf)?;
        h.update(&buf);
        h.update(&size.to_le_bytes());
    }
    Ok(h.finalize().to_hex().to_string())
}

/// Streams the full content through BLAKE3. Checks `cancel` between chunks.
pub fn full_hash(path: &Path, cancel: &AtomicBool, bytes: &AtomicU64) -> std::io::Result<String> {
    let mut f = crate::filesystem::open_for_read(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; READ_BUF];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cancelled",
            ));
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        bytes.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(h.finalize().to_hex().to_string())
}

fn unchanged(c: &Candidate) -> bool {
    match std::fs::symlink_metadata(&c.path) {
        Ok(m) => {
            m.is_file()
                && m.len() as i64 == c.size
                && m.modified().ok().and_then(crate::util::to_unix) == c.mtime
        }
        Err(_) => false,
    }
}

/// Runs `f` over `jobs` on a bounded pool of worker threads, delivering
/// results to `sink` on the calling thread (which owns the DB connection).
fn run_pool<F, S>(
    jobs: Vec<Candidate>,
    workers: usize,
    cancel: &AtomicBool,
    f: F,
    mut sink: S,
) -> AppResult<()>
where
    F: Fn(&Candidate) -> HashOutcome + Sync,
    S: FnMut(Candidate, HashOutcome) -> AppResult<()>,
{
    let queue = Mutex::new(VecDeque::from(jobs));
    let (tx, rx) = mpsc::sync_channel::<(Candidate, HashOutcome)>(workers * 4);
    std::thread::scope(|scope| -> AppResult<()> {
        for _ in 0..workers.max(1) {
            let tx = tx.clone();
            let queue = &queue;
            let f = &f;
            scope.spawn(move || loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let job = match queue.lock() {
                    Ok(mut q) => q.pop_front(),
                    Err(_) => None,
                };
                let Some(job) = job else { break };
                let out = f(&job);
                if tx.send((job, out)).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        let mut result = Ok(());
        for (c, out) in rx {
            if result.is_ok() {
                result = sink(c, out);
                if result.is_err() {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        }
        result
    })
}

/// Finds exact duplicates among indexed files.
pub fn find_duplicates(
    conn: &mut Connection,
    opts: &DuplicateOptions,
    cancel: &AtomicBool,
    on_progress: &dyn Fn(&DuplicateProgress),
) -> AppResult<DuplicateProgress> {
    let started = Instant::now();
    let min_size = opts.min_size.max(1) as i64;
    let mut p = DuplicateProgress {
        phase: "grouping".into(),
        ..Default::default()
    };
    on_progress(&p);

    // Stage 1: size groups. Placeholders and symlinks are never read.
    let candidates: Vec<(Candidate, Option<String>, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT id, path, size, modified_at,
                    CASE WHEN hashed_size = size AND hashed_mtime IS modified_at THEN partial_hash END,
                    CASE WHEN hashed_size = size AND hashed_mtime IS modified_at THEN full_hash END
             FROM files
             WHERE is_symlink = 0 AND is_placeholder = 0 AND size >= ?1 AND size IN (
                 SELECT size FROM files WHERE is_symlink = 0 AND is_placeholder = 0 AND size >= ?1
                 GROUP BY size HAVING COUNT(*) > 1)
             ORDER BY size DESC",
        )?;
        let rows = stmt
            .query_map([min_size], |r| {
                Ok((
                    Candidate {
                        id: r.get(0)?,
                        path: r.get(1)?,
                        size: r.get(2)?,
                        mtime: r.get(3)?,
                    },
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    p.candidates = candidates.len() as u64;

    // Stage 2: partial hashes for candidates without a valid cached value.
    let mut partial: HashMap<i64, String> = HashMap::new();
    let mut full: HashMap<i64, String> = HashMap::new();
    let mut need_partial = Vec::new();
    let mut by_id: HashMap<i64, Candidate> = HashMap::new();
    for (c, ph, fh) in candidates {
        if let Some(fh) = fh {
            full.insert(c.id, fh);
        }
        match ph {
            Some(ph) => {
                partial.insert(c.id, ph);
            }
            None => need_partial.push(c.clone()),
        }
        by_id.insert(c.id, c);
    }

    p.phase = "partial".into();
    p.to_hash = need_partial.len() as u64;
    p.bytes_to_hash = need_partial
        .iter()
        .map(|c| (c.size as u64).min(EDGE * 2))
        .sum();
    on_progress(&p);
    let bytes = AtomicU64::new(0);
    let mut last = Instant::now();
    {
        let tx = conn.transaction()?;
        {
            let mut store = tx.prepare(
                "UPDATE files SET partial_hash = ?2, hashed_size = ?3, hashed_mtime = ?4,
                    full_hash = CASE WHEN ?3 <= ?5 THEN ?2 ELSE NULL END
                 WHERE id = ?1 AND size = ?3 AND modified_at IS ?4",
            )?;
            let mut invalidate =
                tx.prepare("UPDATE files SET partial_hash = NULL, full_hash = NULL WHERE id = ?1")?;
            run_pool(
                need_partial,
                opts.workers,
                cancel,
                |c| {
                    if !unchanged(c) {
                        return HashOutcome::Changed;
                    }
                    match partial_hash(Path::new(&c.path), c.size as u64) {
                        Ok(h) => {
                            bytes.fetch_add((c.size as u64).min(EDGE * 2), Ordering::Relaxed);
                            HashOutcome::Ok(h)
                        }
                        Err(_) => HashOutcome::Failed,
                    }
                },
                |c, out| {
                    p.hashed += 1;
                    match out {
                        HashOutcome::Ok(h) => {
                            store.execute(params![c.id, h, c.size, c.mtime, (EDGE * 2) as i64])?;
                            if (c.size as u64) <= EDGE * 2 {
                                full.insert(c.id, h.clone());
                            }
                            partial.insert(c.id, h);
                        }
                        HashOutcome::Changed => {
                            p.changed += 1;
                            invalidate.execute([c.id])?;
                        }
                        HashOutcome::Failed => p.errors += 1,
                    }
                    if last.elapsed() >= PROGRESS_INTERVAL {
                        p.bytes_hashed = bytes.load(Ordering::Relaxed);
                        p.elapsed_ms = started.elapsed().as_millis() as u64;
                        on_progress(&p);
                        last = Instant::now();
                    }
                    Ok(())
                },
            )?;
        }
        tx.commit()?;
    }
    if cancel.load(Ordering::Relaxed) {
        return finish(conn, p, "cancelled", started, on_progress);
    }

    // Stage 3: full hashes where (size, partial) collide.
    let mut buckets: HashMap<(i64, &str), Vec<i64>> = HashMap::new();
    for (id, ph) in &partial {
        if let Some(c) = by_id.get(id) {
            buckets.entry((c.size, ph.as_str())).or_default().push(*id);
        }
    }
    let need_full: Vec<Candidate> = buckets
        .values()
        .filter(|ids| ids.len() > 1)
        .flatten()
        .filter(|id| !full.contains_key(id))
        .filter_map(|id| by_id.get(id).cloned())
        .collect();

    p.phase = "full".into();
    p.hashed = 0;
    p.to_hash = need_full.len() as u64;
    p.bytes_to_hash = need_full.iter().map(|c| c.size as u64).sum();
    bytes.store(0, Ordering::Relaxed);
    on_progress(&p);
    {
        let tx = conn.transaction()?;
        {
            let mut store = tx.prepare(
                "UPDATE files SET full_hash = ?2 WHERE id = ?1 AND size = ?3 AND modified_at IS ?4
                    AND hashed_size = ?3 AND hashed_mtime IS ?4",
            )?;
            let mut invalidate =
                tx.prepare("UPDATE files SET partial_hash = NULL, full_hash = NULL WHERE id = ?1")?;
            run_pool(
                need_full,
                opts.workers,
                cancel,
                |c| {
                    if !unchanged(c) {
                        return HashOutcome::Changed;
                    }
                    match full_hash(Path::new(&c.path), cancel, &bytes) {
                        // Re-check after hashing: a file modified while being
                        // read must not be recorded with this hash.
                        Ok(h) if unchanged(c) => HashOutcome::Ok(h),
                        Ok(_) => HashOutcome::Changed,
                        Err(_) => HashOutcome::Failed,
                    }
                },
                |c, out| {
                    p.hashed += 1;
                    match out {
                        HashOutcome::Ok(h) => {
                            store.execute(params![c.id, h, c.size, c.mtime])?;
                        }
                        HashOutcome::Changed => {
                            p.changed += 1;
                            invalidate.execute([c.id])?;
                        }
                        HashOutcome::Failed => {
                            if !cancel.load(Ordering::Relaxed) {
                                p.errors += 1;
                            }
                        }
                    }
                    if last.elapsed() >= PROGRESS_INTERVAL {
                        p.bytes_hashed = bytes.load(Ordering::Relaxed);
                        p.elapsed_ms = started.elapsed().as_millis() as u64;
                        on_progress(&p);
                        last = Instant::now();
                    }
                    Ok(())
                },
            )?;
        }
        tx.commit()?;
    }
    p.bytes_hashed = bytes.load(Ordering::Relaxed);
    let status = if cancel.load(Ordering::Relaxed) {
        "cancelled"
    } else {
        "completed"
    };
    finish(conn, p, status, started, on_progress)
}

fn finish(
    conn: &Connection,
    mut p: DuplicateProgress,
    status: &str,
    started: Instant,
    on_progress: &dyn Fn(&DuplicateProgress),
) -> AppResult<DuplicateProgress> {
    let (groups, waste) = totals(conn)?;
    p.groups = groups;
    p.reclaimable = waste;
    p.phase = "done".into();
    p.status = Some(status.into());
    p.elapsed_ms = started.elapsed().as_millis() as u64;
    on_progress(&p);
    Ok(p)
}

/// (number of duplicate groups, reclaimable bytes).
pub fn totals(conn: &Connection) -> AppResult<(i64, i64)> {
    Ok(conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM((n - 1) * size), 0) FROM (
            SELECT size, COUNT(*) AS n FROM files
            WHERE full_hash IS NOT NULL AND is_symlink = 0
            GROUP BY full_hash, size HAVING COUNT(*) > 1)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: i64,
    pub count: i64,
    pub reclaimable: i64,
    pub files: Vec<FileRecord>,
}

pub fn list_groups(
    conn: &Connection,
    sort: &str,
    limit: u32,
    offset: u32,
) -> AppResult<Page<DuplicateGroup>> {
    let order = match sort {
        "count" => "n DESC, size DESC",
        "size" => "size DESC",
        _ => "(n - 1) * size DESC",
    };
    let limit = limit.clamp(1, 200);
    let total: i64 = totals(conn)?.0;
    let mut stmt = conn.prepare(&format!(
        "SELECT full_hash, size, COUNT(*) AS n FROM files
         WHERE full_hash IS NOT NULL AND is_symlink = 0
         GROUP BY full_hash, size HAVING COUNT(*) > 1
         ORDER BY {order} LIMIT {limit} OFFSET {offset}"
    ))?;
    let heads = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut files_stmt = conn.prepare(&format!(
        "SELECT {FILE_COLUMNS} FROM files f WHERE f.full_hash = ?1 AND f.size = ?2 AND f.is_symlink = 0
         ORDER BY f.modified_at ASC, f.path"
    ))?;
    let mut items = Vec::with_capacity(heads.len());
    for (hash, size, count) in heads {
        let files = files_stmt
            .query_map(params![hash, size], file_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        items.push(DuplicateGroup {
            reclaimable: (count - 1) * size,
            hash,
            size,
            count,
            files,
        });
    }
    Ok(Page {
        items,
        total,
        offset,
        limit,
    })
}

pub fn get_group(conn: &Connection, hash: &str) -> AppResult<DuplicateGroup> {
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::invalid("invalid hash"));
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT {FILE_COLUMNS} FROM files f WHERE f.full_hash = ?1 AND f.is_symlink = 0 ORDER BY f.modified_at ASC"
    ))?;
    let files = stmt
        .query_map([hash], file_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    if files.len() < 2 {
        return Err(AppError::NotFound(
            "this duplicate group no longer exists".into(),
        ));
    }
    let size = files[0].size;
    let count = files.len() as i64;
    Ok(DuplicateGroup {
        hash: hash.to_string(),
        size,
        count,
        reclaimable: (count - 1) * size,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{test_db, Database};
    use crate::scanner::{run_scan, ScanOptions};
    use crate::security::{add_location, LocationKind};

    fn scan(db: &Database, root: &Path) {
        db.with(|c| {
            let loc = match crate::security::list_locations(c)?.into_iter().next() {
                Some(l) => l,
                None => add_location(c, root, LocationKind::Scan)?,
            };
            run_scan(
                c,
                &loc,
                &ScanOptions::default(),
                &AtomicBool::new(false),
                &|_| {},
            )?;
            Ok(())
        })
        .unwrap();
    }

    fn detect(db: &Database) -> DuplicateProgress {
        db.with(|c| {
            find_duplicates(
                c,
                &DuplicateOptions {
                    min_size: 1,
                    workers: 3,
                },
                &AtomicBool::new(false),
                &|_| {},
            )
        })
        .unwrap()
    }

    #[test]
    fn finds_exact_duplicates_only() {
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let mut big_variant = big.clone();
        big_variant[100_000] ^= 0xFF; // same size, same edges, different middle
        std::fs::create_dir_all(r.join("x")).unwrap();
        std::fs::write(r.join("a.bin"), &big).unwrap();
        std::fs::write(r.join("x/a-copy.bin"), &big).unwrap();
        std::fs::write(r.join("x/a-variant.bin"), &big_variant).unwrap();
        std::fs::write(r.join("s1.txt"), b"same small content").unwrap();
        std::fs::write(r.join("s2.txt"), b"same small content").unwrap();
        std::fs::write(r.join("same-size.txt"), b"diff small content").unwrap();
        // Same name, different content: must NOT be a duplicate.
        std::fs::write(r.join("x/s1.txt"), b"completely different!").unwrap();
        std::fs::write(r.join("empty1"), b"").unwrap();
        std::fs::write(r.join("empty2"), b"").unwrap();
        scan(&db, r);

        let p = detect(&db);
        assert_eq!(p.status.as_deref(), Some("completed"));
        assert_eq!(p.groups, 2);
        assert_eq!(p.reclaimable, 200_000 + 18);
        db.with(|c| {
            let page = list_groups(c, "waste", 10, 0)?;
            assert_eq!(page.items.len(), 2);
            assert_eq!(page.items[0].size, 200_000);
            let names: Vec<_> = page.items[0]
                .files
                .iter()
                .map(|f| f.name.as_str())
                .collect();
            assert!(names.contains(&"a.bin") && names.contains(&"a-copy.bin"));
            assert!(!names.contains(&"a-variant.bin"));
            let g = get_group(c, &page.items[1].hash)?;
            assert_eq!(g.count, 2);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn hashes_are_cached_and_invalidated_on_change() {
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        std::fs::write(r.join("a.txt"), b"identical").unwrap();
        std::fs::write(r.join("b.txt"), b"identical").unwrap();
        scan(&db, r);
        assert_eq!(detect(&db).groups, 1);
        // Second run: nothing new to hash.
        assert_eq!(detect(&db).to_hash, 0);

        // Modify one file (same size, new content and mtime) and rescan.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(r.join("b.txt"), b"different").unwrap();
        scan(&db, r);
        let p = detect(&db);
        assert_eq!(p.groups, 0);
    }

    #[test]
    fn files_changed_after_indexing_are_not_hashed() {
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        std::fs::write(r.join("a.txt"), b"identical").unwrap();
        std::fs::write(r.join("b.txt"), b"identical").unwrap();
        scan(&db, r);
        std::fs::remove_file(r.join("b.txt")).unwrap();
        let p = detect(&db);
        assert_eq!(p.changed, 1);
        assert_eq!(p.groups, 0);
    }

    #[test]
    fn cancellation_is_respected() {
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        for i in 0..20 {
            std::fs::write(root.path().join(format!("f{i}.txt")), b"same").unwrap();
        }
        scan(&db, root.path());
        let p = db
            .with(|c| {
                find_duplicates(
                    c,
                    &DuplicateOptions {
                        min_size: 1,
                        workers: 2,
                    },
                    &AtomicBool::new(true),
                    &|_| {},
                )
            })
            .unwrap();
        assert_eq!(p.status.as_deref(), Some("cancelled"));
    }

    #[test]
    fn partial_hash_of_small_file_equals_full_hash() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x");
        std::fs::write(&f, b"abc").unwrap();
        let ph = partial_hash(&f, 3).unwrap();
        let fh = full_hash(&f, &AtomicBool::new(false), &AtomicU64::new(0)).unwrap();
        assert_eq!(ph, fh);
        assert_eq!(fh, blake3::hash(b"abc").to_hex().to_string());
    }
}
