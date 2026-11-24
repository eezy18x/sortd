//! Operation engine tests. Every test runs inside its own temporary
//! directory; nothing touches real user folders or the system trash.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::execute::{dry_run, execute, recover, undo};
use super::*;
use crate::database::{test_db, Database};
use crate::scanner::{run_scan, ScanOptions};
use crate::security::{add_location, LocationKind};

struct Env {
    _dbdir: tempfile::TempDir,
    db: Database,
    _root: tempfile::TempDir,
    root: PathBuf,
}

fn env() -> Env {
    let (dbdir, db) = test_db();
    let root_dir = tempfile::tempdir().unwrap();
    let r = root_dir.path();
    fs::create_dir_all(r.join("Downloads")).unwrap();
    fs::create_dir_all(r.join("Documents")).unwrap();
    fs::write(r.join("Downloads/report.pdf"), b"%PDF-1.7 report").unwrap();
    fs::write(r.join("Downloads/photo.jpg"), vec![7u8; 4096]).unwrap();
    fs::write(
        r.join("Documents/report.pdf"),
        b"an existing, different report",
    )
    .unwrap();
    let root = db
        .with(|c| {
            let loc = add_location(c, r, LocationKind::Scan)?;
            run_scan(
                c,
                &loc,
                &ScanOptions::default(),
                &AtomicBool::new(false),
                &|_| {},
            )?;
            Ok(PathBuf::from(loc.path))
        })
        .unwrap();
    Env {
        _dbdir: dbdir,
        db,
        _root: root_dir,
        root,
    }
}

impl Env {
    /// Joins a '/'-separated relative path component by component, so the
    /// result uses the platform separator exactly like SORTD's index does.
    fn at(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.root.clone(), |acc, c| acc.join(c))
    }
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn mv(src: &Path, dir: &Path) -> OperationRequest {
    OperationRequest::Move {
        source: s(src),
        destination_dir: s(dir),
    }
}

fn req(ops: Vec<OperationRequest>, policy: ConflictPolicy) -> PlanRequest {
    PlanRequest {
        label: "test".into(),
        origin: "manual".into(),
        operations: ops,
        conflict_policy: policy,
        allow_cross_volume: false,
    }
}

fn plan_and_run(e: &Env, r: PlanRequest) -> Batch {
    e.db.with(|c| {
        let b = plan(c, &r)?;
        execute(c, b.summary.id, &AtomicBool::new(false), &|_| {})
    })
    .unwrap()
}

fn indexed(e: &Env, path: &Path) -> bool {
    e.db.with(|c| Ok(crate::indexing::get_file_by_path(c, &s(path))?.is_some()))
        .unwrap()
}

#[test]
fn move_updates_disk_and_index_and_undo_restores() {
    let e = env();
    let src = e.at("Downloads/photo.jpg");
    let dir = e.at("Pictures/2024");
    let b = plan_and_run(&e, req(vec![mv(&src, &dir)], ConflictPolicy::Skip));
    assert_eq!(b.summary.status, "completed");
    // Two mkdir items (Pictures, Pictures/2024) and the move.
    assert_eq!(b.summary.succeeded, 3);
    let dst = dir.join("photo.jpg");
    assert!(dst.exists() && !src.exists());
    assert_eq!(fs::read(&dst).unwrap(), vec![7u8; 4096]);
    assert!(indexed(&e, &dst) && !indexed(&e, &src));
    assert!(b.items.iter().all(|i| i.verified));

    let u = e.db.with(|c| undo(c, b.summary.id, &|_| {})).unwrap();
    assert_eq!(u.summary.status, "undone");
    assert!(src.exists() && !dst.exists());
    assert!(
        !e.at("Pictures").exists(),
        "created folders are removed on undo"
    );
    assert!(indexed(&e, &src) && !indexed(&e, &dst));
}

#[test]
fn conflicts_are_skipped_and_never_overwritten() {
    let e = env();
    let src = e.at("Downloads/report.pdf");
    let b = plan_and_run(
        &e,
        req(vec![mv(&src, &e.at("Documents"))], ConflictPolicy::Skip),
    );
    assert_eq!(b.items[0].status, "skipped");
    assert!(b.items[0]
        .issue
        .as_deref()
        .unwrap()
        .contains("already exists"));
    assert_eq!(
        fs::read(e.at("Documents/report.pdf")).unwrap(),
        b"an existing, different report"
    );
    assert!(src.exists());
}

#[test]
fn conflict_rename_policy_picks_a_free_name() {
    let e = env();
    let src = e.at("Downloads/report.pdf");
    let b = plan_and_run(
        &e,
        req(vec![mv(&src, &e.at("Documents"))], ConflictPolicy::Rename),
    );
    assert_eq!(b.summary.status, "completed");
    assert_eq!(
        fs::read(e.at("Documents/report (1).pdf")).unwrap(),
        b"%PDF-1.7 report"
    );
    assert_eq!(
        fs::read(e.at("Documents/report.pdf")).unwrap(),
        b"an existing, different report"
    );
}

#[test]
fn conflicts_within_one_batch_are_detected() {
    let e = env();
    fs::create_dir_all(e.at("Other")).unwrap();
    fs::write(e.at("Other/photo.jpg"), b"another photo").unwrap();
    let dst = e.at("Pictures");
    let r = req(
        vec![
            mv(&e.at("Downloads/photo.jpg"), &dst),
            mv(&e.at("Other/photo.jpg"), &dst),
        ],
        ConflictPolicy::Skip,
    );
    let b = e.db.with(|c| plan(c, &r)).unwrap();
    let statuses: Vec<_> = b.items.iter().map(|i| (i.op, i.status.as_str())).collect();
    assert_eq!(
        statuses,
        vec![
            (Op::Mkdir, "pending"),
            (Op::Move, "pending"),
            (Op::Move, "skipped")
        ]
    );
}

#[test]
fn unauthorized_and_traversal_paths_are_rejected() {
    let e = env();
    let outside = tempfile::tempdir().unwrap();
    let src = e.at("Downloads/photo.jpg");
    let traversal = format!(
        "{}{}..{}..",
        s(&e.at("Documents")),
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    );
    let r = req(
        vec![
            mv(&src, outside.path()),
            OperationRequest::Move {
                source: s(&src),
                destination_dir: traversal,
            },
            OperationRequest::Trash {
                source: s(&outside.path().join("x")),
            },
            OperationRequest::Trash { source: s(&e.root) },
            OperationRequest::Move {
                source: "relative/path".into(),
                destination_dir: s(&e.root),
            },
        ],
        ConflictPolicy::Skip,
    );
    let b = e.db.with(|c| plan(c, &r)).unwrap();
    assert_eq!(b.pending, 0, "{:#?}", b.items);
    assert!(src.exists());
    assert!(e.root.exists());
}

#[test]
fn source_changed_after_preview_fails_safely() {
    let e = env();
    let src = e.at("Downloads/photo.jpg");
    let r = req(vec![mv(&src, &e.at("Documents"))], ConflictPolicy::Skip);
    let b = e.db.with(|c| plan(c, &r)).unwrap();
    fs::write(&src, b"edited after the preview").unwrap();
    let b =
        e.db.with(|c| execute(c, b.summary.id, &AtomicBool::new(false), &|_| {}))
            .unwrap();
    assert_eq!(b.summary.status, "failed");
    assert!(b.items[0]
        .error
        .as_deref()
        .unwrap()
        .contains("changed since the preview"));
    assert!(src.exists());
}

#[test]
fn destination_created_after_preview_is_not_overwritten() {
    let e = env();
    let src = e.at("Downloads/photo.jpg");
    let r = req(vec![mv(&src, &e.at("Documents"))], ConflictPolicy::Skip);
    let b = e.db.with(|c| plan(c, &r)).unwrap();
    fs::write(e.at("Documents/photo.jpg"), b"appeared later").unwrap();
    let b =
        e.db.with(|c| execute(c, b.summary.id, &AtomicBool::new(false), &|_| {}))
            .unwrap();
    assert_eq!(b.items[0].status, "failed");
    assert_eq!(
        fs::read(e.at("Documents/photo.jpg")).unwrap(),
        b"appeared later"
    );
    assert!(src.exists());
}

#[test]
fn a_plan_cannot_run_twice() {
    let e = env();
    let r = req(
        vec![mv(&e.at("Downloads/photo.jpg"), &e.at("Documents"))],
        ConflictPolicy::Skip,
    );
    let b = plan_and_run(&e, r);
    let again =
        e.db.with(|c| execute(c, b.summary.id, &AtomicBool::new(false), &|_| {}));
    assert!(matches!(again, Err(AppError::Conflict(_))));
}

#[test]
fn dry_run_changes_nothing() {
    let e = env();
    let src = e.at("Downloads/photo.jpg");
    let r = req(vec![mv(&src, &e.at("New/Deep"))], ConflictPolicy::Skip);
    let (b, results) =
        e.db.with(|c| {
            let b = plan(c, &r)?;
            let res = dry_run(c, b.summary.id)?;
            Ok((b, res))
        })
        .unwrap();
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.ok), "{results:?}");
    assert!(src.exists());
    assert!(!e.at("New").exists());
    let after = e.db.with(|c| get_batch(c, b.summary.id)).unwrap();
    assert_eq!(after.summary.status, "planned");
}

#[test]
fn copy_is_verified_and_undo_trashes_only_an_unmodified_copy() {
    let e = env();
    let src = e.at("Downloads/photo.jpg");
    let r = req(
        vec![OperationRequest::Copy {
            source: s(&src),
            destination_dir: s(&e.at("Documents")),
        }],
        ConflictPolicy::Skip,
    );
    let b = plan_and_run(&e, r);
    let dst = e.at("Documents/photo.jpg");
    assert_eq!(b.summary.status, "completed");
    assert!(src.exists() && dst.exists());
    assert!(indexed(&e, &dst));
    assert_eq!(
        fs::metadata(&src).unwrap().modified().unwrap(),
        fs::metadata(&dst).unwrap().modified().unwrap()
    );
    let u = e.db.with(|c| undo(c, b.summary.id, &|_| {})).unwrap();
    assert_eq!(u.summary.status, "undone");
    assert!(!dst.exists() && src.exists());

    // A copy modified after the operation is left in place.
    let b = plan_and_run(
        &e,
        req(
            vec![OperationRequest::Copy {
                source: s(&src),
                destination_dir: s(&e.at("Documents")),
            }],
            ConflictPolicy::Skip,
        ),
    );
    fs::write(&dst, b"user edited the copy").unwrap();
    let u = e.db.with(|c| undo(c, b.summary.id, &|_| {})).unwrap();
    assert_eq!(u.items[0].status, "undo_failed");
    assert_eq!(fs::read(&dst).unwrap(), b"user edited the copy");
}

#[test]
fn undo_refuses_when_moved_file_was_modified() {
    let e = env();
    let src = e.at("Downloads/photo.jpg");
    let b = plan_and_run(
        &e,
        req(vec![mv(&src, &e.at("Documents"))], ConflictPolicy::Skip),
    );
    let dst = e.at("Documents/photo.jpg");
    fs::write(&dst, b"changed").unwrap();
    let u = e.db.with(|c| undo(c, b.summary.id, &|_| {})).unwrap();
    assert_eq!(u.items[0].status, "undo_failed");
    assert!(dst.exists() && !src.exists());
}

#[test]
fn rename_validates_names_and_supports_case_changes() {
    let e = env();
    let src = e.at("Downloads/photo.jpg");
    let bad = req(
        vec![OperationRequest::Rename {
            source: s(&src),
            new_name: "a/b.jpg".into(),
        }],
        ConflictPolicy::Skip,
    );
    let b = e.db.with(|c| plan(c, &bad)).unwrap();
    assert_eq!(b.pending, 0);

    let b = plan_and_run(
        &e,
        req(
            vec![OperationRequest::Rename {
                source: s(&src),
                new_name: "Photo.JPG".into(),
            }],
            ConflictPolicy::Skip,
        ),
    );
    assert_eq!(b.summary.status, "completed", "{:#?}", b.items);
    let names: Vec<_> = fs::read_dir(e.at("Downloads"))
        .unwrap()
        .map(|d| d.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"Photo.JPG".to_string()));
    assert!(indexed(&e, &e.at("Downloads/Photo.JPG")));

    let b = plan_and_run(
        &e,
        req(
            vec![OperationRequest::Rename {
                source: s(&e.at("Downloads/Photo.JPG")),
                new_name: "холидей фото.jpg".into(),
            }],
            ConflictPolicy::Skip,
        ),
    );
    assert_eq!(b.summary.status, "completed");
    assert!(e.at("Downloads/холидей фото.jpg").exists());
}

#[test]
fn trash_removes_file_and_index_row() {
    let e = env();
    let src = e.at("Downloads/photo.jpg");
    let b = plan_and_run(
        &e,
        req(
            vec![OperationRequest::Trash { source: s(&src) }],
            ConflictPolicy::Skip,
        ),
    );
    assert_eq!(b.summary.status, "completed");
    assert!(!src.exists());
    assert!(!indexed(&e, &src));
    assert!(
        !b.summary.can_undo,
        "trash is restored from the system trash, not undone in-app"
    );
}

#[test]
fn moving_a_folder_rewrites_descendant_paths() {
    let e = env();
    let src = e.at("Downloads");
    let b = plan_and_run(
        &e,
        req(vec![mv(&src, &e.at("Archive"))], ConflictPolicy::Skip),
    );
    assert_eq!(b.summary.status, "completed", "{:#?}", b.items);
    let moved = e.at("Archive/Downloads/photo.jpg");
    assert!(moved.exists());
    assert!(indexed(&e, &moved));
    assert!(!indexed(&e, &src.join("photo.jpg")));
    let size: i64 =
        e.db.with(|c| {
            Ok(c.query_row(
                "SELECT total_size FROM directories WHERE path = ?1",
                [s(&e.at("Archive/Downloads"))],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(size, 4096 + 15);

    // A folder cannot be moved into itself.
    let b =
        e.db.with(|c| {
            plan(
                c,
                &req(
                    vec![mv(&e.at("Archive"), &e.at("Archive/Downloads"))],
                    ConflictPolicy::Skip,
                ),
            )
        })
        .unwrap();
    assert_eq!(b.pending, 0);

    let u = e.db.with(|c| undo(c, b.summary.id - 1, &|_| {})).unwrap();
    assert_eq!(u.summary.status, "undone");
    assert!(src.join("photo.jpg").exists());
    assert!(indexed(&e, &src.join("photo.jpg")));
}

#[test]
fn interrupted_items_are_reconciled_on_recovery() {
    let e = env();
    let src = e.at("Downloads/photo.jpg");
    let other = e.at("Downloads/report.pdf");
    let r = req(
        vec![mv(&src, &e.at("Documents")), mv(&other, &e.at("Pictures"))],
        ConflictPolicy::Skip,
    );
    let b = e.db.with(|c| plan(c, &r)).unwrap();
    // Simulate a crash: the first move happened on disk while "running",
    // the rest never started.
    fs::rename(&src, e.at("Documents/photo.jpg")).unwrap();
    e.db.with(|c| {
        c.execute("UPDATE operation_batches SET status = 'running' WHERE id = ?1", [b.summary.id])?;
        c.execute("UPDATE operation_items SET status = 'running' WHERE batch_id = ?1 AND op = 'move' AND source = ?2", rusqlite::params![b.summary.id, s(&src)])?;
        Ok(())
    })
    .unwrap();
    let n = e.db.with(recover).unwrap();
    assert_eq!(n, 1);
    let after = e.db.with(|c| get_batch(c, b.summary.id)).unwrap();
    assert_eq!(after.summary.status, "interrupted");
    let moved = after
        .items
        .iter()
        .find(|i| i.source.as_deref() == Some(&s(&src)))
        .unwrap();
    assert_eq!(moved.status, "done");
    assert!(after
        .items
        .iter()
        .filter(|i| i.source.as_deref() == Some(&s(&other)))
        .all(|i| i.status == "skipped"));
    assert!(other.exists());
    assert!(indexed(&e, &e.at("Documents/photo.jpg")));
}

#[test]
fn cross_volume_move_primitive_verifies_before_removing() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("a.bin");
    let dst = d.path().join("b.bin");
    fs::write(&src, vec![3u8; 100_000]).unwrap();
    let m = fs::metadata(&src).unwrap();
    let h = execute::cross_volume_move(
        &src,
        &dst,
        &d.path().join(".tmp"),
        Some(m.len() as i64),
        m.modified().ok().and_then(crate::util::to_unix),
    )
    .unwrap();
    assert!(!src.exists());
    assert_eq!(h, blake3::hash(&vec![3u8; 100_000]).to_hex().to_string());

    // If the source changed during the copy, the copy is discarded.
    fs::write(&src, b"x").unwrap();
    let dst2 = d.path().join("c.bin");
    let err = execute::cross_volume_move(&src, &dst2, &d.path().join(".tmp2"), Some(999), None);
    assert!(err.is_err());
    assert!(src.exists() && !dst2.exists());
}

#[test]
fn history_lists_batches_and_discard_works() {
    let e = env();
    let r = req(
        vec![mv(&e.at("Downloads/photo.jpg"), &e.at("Documents"))],
        ConflictPolicy::Skip,
    );
    let b = e.db.with(|c| plan(c, &r)).unwrap();
    e.db.with(|c| discard(c, b.summary.id)).unwrap();
    let (list, total) = e.db.with(|c| list_batches(c, 50, 0)).unwrap();
    assert_eq!(total, 0);
    assert!(list.is_empty());
    assert!(e.db.with(|c| discard(c, b.summary.id)).is_err());
}

#[test]
fn plans_do_not_survive_a_restart() {
    let e = env();
    let r = req(
        vec![mv(&e.at("Downloads/photo.jpg"), &e.at("Documents"))],
        ConflictPolicy::Skip,
    );
    let b = e.db.with(|c| plan(c, &r)).unwrap();
    e.db.with(recover).unwrap();
    let after = e.db.with(|c| get_batch(c, b.summary.id)).unwrap();
    assert_eq!(after.summary.status, "discarded");
    let again =
        e.db.with(|c| execute(c, b.summary.id, &AtomicBool::new(false), &|_| {}));
    assert!(matches!(again, Err(AppError::Conflict(_))));
    assert!(e.at("Downloads/photo.jpg").exists());
}
