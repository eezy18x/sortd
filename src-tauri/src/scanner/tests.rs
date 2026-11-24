use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use super::*;
use crate::database::{test_db, Database};
use crate::security::{add_location, LocationKind};

fn write(path: &Path, bytes: &[u8]) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(&r.join("docs/report.pdf"), &[0u8; 1000]);
    write(&r.join("docs/notes.txt"), b"hello");
    write(&r.join("photos/2024/beach.jpg"), &[1u8; 3000]);
    write(&r.join("music/song.mp3"), &[2u8; 500]);
    write(&r.join("empty.bin"), b"");
    fs::create_dir_all(r.join("emptydir")).unwrap();
    dir
}

fn scan(db: &Database, root: &Path, opts: &ScanOptions) -> ScanSummary {
    db.with(|c| {
        let loc = add_location(c, root, LocationKind::Scan)?;
        run_scan(c, &loc, opts, &AtomicBool::new(false), &|_| {})
    })
    .unwrap()
}

fn count(db: &Database, sql: &str) -> i64 {
    db.with(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn scans_files_directories_and_metadata() {
    let (_d, db) = test_db();
    let root = fixture();
    let s = scan(&db, root.path(), &ScanOptions::default());
    assert_eq!(s.status, "completed");
    assert_eq!(s.files_seen, 5);
    assert_eq!(s.bytes_seen, 1000 + 5 + 3000 + 500);
    assert_eq!(s.files_added, 5);
    assert_eq!(s.errors, 0);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM files"), 5);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM files WHERE category = 'documents'"
        ),
        2
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM files WHERE category = 'images'"),
        1
    );
    // Root + docs + photos + photos/2024 + music + emptydir
    assert_eq!(count(&db, "SELECT COUNT(*) FROM directories"), 6);
    assert_eq!(
        count(&db, "SELECT total_size FROM directories WHERE depth = 0"),
        4505
    );
    assert_eq!(
        count(&db, "SELECT total_files FROM directories WHERE depth = 0"),
        5
    );
    assert_eq!(
        count(&db, "SELECT total_dirs FROM directories WHERE depth = 0"),
        5
    );
    assert_eq!(
        count(
            &db,
            "SELECT total_size FROM directories WHERE name = 'photos'"
        ),
        3000
    );
    assert_eq!(
        count(&db, "SELECT complete FROM directories WHERE depth = 0"),
        1
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM files WHERE modified_at IS NULL"),
        0
    );
    // Full-text index is populated by triggers.
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM files_fts WHERE files_fts MATCH 'beach'"
        ),
        1
    );
}

#[test]
fn incremental_rescan_preserves_hashes_and_tracks_changes() {
    let (_d, db) = test_db();
    let root = fixture();
    scan(&db, root.path(), &ScanOptions::default());
    db.with(|c| {
        c.execute(
            "UPDATE files SET full_hash = 'cached', partial_hash = 'p'",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    // Modify one file (different size), delete one, add one.
    write(&root.path().join("docs/notes.txt"), b"hello, world");
    fs::remove_file(root.path().join("music/song.mp3")).unwrap();
    write(&root.path().join("new.md"), b"# new");

    let s = scan(&db, root.path(), &ScanOptions::default());
    assert_eq!(s.files_added, 1);
    assert_eq!(s.files_updated, 1);
    assert_eq!(s.files_removed, 1);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM files"), 5);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM files WHERE name = 'notes.txt' AND full_hash IS NULL"
        ),
        1
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM files WHERE name = 'report.pdf' AND full_hash = 'cached'"
        ),
        1
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM files_fts WHERE files_fts MATCH 'song'"
        ),
        0
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM scans WHERE status = 'completed'"),
        2
    );
}

#[test]
fn exclusions_by_name_and_path() {
    let (_d, db) = test_db();
    let root = fixture();
    write(&root.path().join("project/node_modules/lib/index.js"), b"x");
    write(&root.path().join("project/src/main.rs"), b"fn main(){}");
    let canonical = crate::filesystem::canonicalize(root.path()).unwrap();
    let opts = ScanOptions {
        excluded_names: vec!["NODE_MODULES".into()],
        excluded_paths: vec![canonical.join("photos")],
        ..Default::default()
    };
    let s = scan(&db, root.path(), &opts);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM files WHERE path LIKE '%node_modules%'"
        ),
        // Name matching is case-insensitive only where the platform is.
        if crate::filesystem::CASE_INSENSITIVE {
            0
        } else {
            1
        }
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM files WHERE name = 'beach.jpg'"),
        0
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM files WHERE name = 'main.rs'"),
        1
    );
    assert_eq!(s.status, "completed");
}

#[test]
fn root_inside_excluded_path_is_still_scanned() {
    let (_d, db) = test_db();
    let root = fixture();
    let canonical = crate::filesystem::canonicalize(root.path()).unwrap();
    let opts = ScanOptions {
        excluded_paths: vec![canonical.parent().unwrap().to_path_buf()],
        ..Default::default()
    };
    let s = scan(&db, root.path(), &opts);
    assert_eq!(s.files_seen, 5);
}

#[test]
fn hidden_files_respect_setting() {
    let (_d, db) = test_db();
    let root = fixture();
    write(&root.path().join(".hidden/secret.txt"), b"s");
    write(&root.path().join(".dotfile"), b"d");
    let s = scan(
        &db,
        root.path(),
        &ScanOptions {
            include_hidden: false,
            ..Default::default()
        },
    );
    assert_eq!(s.files_seen, 5);
    let (_d2, db2) = test_db();
    let s = scan(
        &db2,
        root.path(),
        &ScanOptions {
            include_hidden: true,
            ..Default::default()
        },
    );
    assert_eq!(s.files_seen, 7);
    // Only `.dotfile` is hidden itself; `secret.txt` lives in a hidden folder.
    assert_eq!(
        count(&db2, "SELECT COUNT(*) FROM files WHERE is_hidden = 1"),
        1
    );
}

#[test]
fn unicode_and_long_file_names() {
    let (_d, db) = test_db();
    let dir = tempfile::tempdir().unwrap();
    let names = [
        "日本語のレポート.pdf",
        "Ünïcödé — résumé.docx",
        "emoji 📁 folder.txt",
        "ελληνικά.md",
    ];
    for n in names {
        write(&dir.path().join("intl").join(n), b"data");
    }
    let long = format!("{}.txt", "a".repeat(200));
    write(&dir.path().join(&long), b"long");
    let s = scan(&db, dir.path(), &ScanOptions::default());
    assert_eq!(s.files_seen, 5);
    assert_eq!(s.skipped, 0);
    for n in names {
        let found: i64 = db
            .with(|c| {
                Ok(
                    c.query_row("SELECT COUNT(*) FROM files WHERE name = ?1", [n], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(found, 1, "missing {n}");
    }
}

#[test]
fn cancellation_stops_scan_without_removing_rows() {
    let (_d, db) = test_db();
    let root = fixture();
    scan(&db, root.path(), &ScanOptions::default());
    let cancel = AtomicBool::new(true);
    let s = db
        .with(|c| {
            let loc = crate::security::list_locations(c)?.remove(0);
            run_scan(c, &loc, &ScanOptions::default(), &cancel, &|_| {})
        })
        .unwrap();
    assert_eq!(s.status, "cancelled");
    // Rows from the earlier complete scan are not deleted by a cancelled scan.
    assert_eq!(count(&db, "SELECT COUNT(*) FROM files"), 5);
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM scans WHERE status = 'cancelled'"),
        1
    );
}

#[test]
fn progress_is_reported_and_ends_with_done() {
    let (_d, db) = test_db();
    let root = fixture();
    let events: Mutex<Vec<ScanProgress>> = Mutex::new(Vec::new());
    db.with(|c| {
        let loc = add_location(c, root.path(), LocationKind::Scan)?;
        run_scan(
            c,
            &loc,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            &|p| events.lock().unwrap().push(p.clone()),
        )
    })
    .unwrap();
    let ev = events.into_inner().unwrap();
    assert_eq!(ev.first().unwrap().phase, "walking");
    let last = ev.last().unwrap();
    assert_eq!(last.phase, "done");
    assert_eq!(last.status.as_deref(), Some("completed"));
    assert_eq!(last.files_seen, 5);
}

#[test]
fn interrupted_scans_are_marked_on_startup() {
    let (_d, db) = test_db();
    let root = fixture();
    scan(&db, root.path(), &ScanOptions::default());
    db.with(|c| {
        c.execute("UPDATE scans SET status = 'running'", [])?;
        assert_eq!(mark_interrupted_scans(c)?, 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM scans WHERE status = 'interrupted'"
        ),
        1
    );
}

#[test]
fn scanning_never_modifies_files() {
    // Compares file contents and modification times. Directory timestamps
    // are not compared: NTFS updates them lazily in the parent's entry.
    fn snapshot(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>, std::time::SystemTime)> {
        walkdir::WalkDir::new(root)
            .sort_by_file_name()
            .into_iter()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().is_file())
            .map(|e| {
                (
                    e.path().to_path_buf(),
                    fs::read(e.path()).unwrap(),
                    fs::metadata(e.path()).unwrap().modified().unwrap(),
                )
            })
            .collect()
    }
    let (_d, db) = test_db();
    let root = fixture();
    let before = snapshot(root.path());
    scan(&db, root.path(), &ScanOptions::default());
    assert_eq!(before, snapshot(root.path()));
}

#[cfg(unix)]
#[test]
fn symlinks_are_recorded_but_not_followed() {
    let (_d, db) = test_db();
    let root = fixture();
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("private/secret.txt"), b"secret");
    std::os::unix::fs::symlink(outside.path(), root.path().join("link-to-outside")).unwrap();
    let s = scan(&db, root.path(), &ScanOptions::default());
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM files WHERE name = 'secret.txt'"),
        0
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM files WHERE is_symlink = 1"),
        1
    );
    assert_eq!(s.files_seen, 6);
}

#[cfg(windows)]
#[test]
fn symlinks_are_recorded_but_not_followed() {
    let (_d, db) = test_db();
    let root = fixture();
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("private/secret.txt"), b"secret");
    // Creating symlinks on Windows requires Developer Mode or elevation.
    if std::os::windows::fs::symlink_dir(outside.path(), root.path().join("link-to-outside"))
        .is_err()
    {
        eprintln!("skipping: symlink creation not permitted on this Windows account");
        return;
    }
    scan(&db, root.path(), &ScanOptions::default());
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM files WHERE name = 'secret.txt'"),
        0
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM files WHERE is_symlink = 1"),
        1
    );
}

#[cfg(unix)]
#[test]
fn permission_denied_marks_directory_incomplete() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, db) = test_db();
    let root = fixture();
    let locked = root.path().join("locked");
    write(&locked.join("inside.txt"), b"x");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        // Running as root: permissions are not enforced.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let s = scan(&db, root.path(), &ScanOptions::default());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(s.status, "completed");
    assert!(s.errors >= 1);
    assert!(s.issues.iter().any(|i| i.error.contains("permission")));
    assert_eq!(
        count(&db, "SELECT complete FROM directories WHERE depth = 0"),
        0
    );
}
