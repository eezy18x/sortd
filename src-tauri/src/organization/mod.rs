//! Organization recommendations ("SORTD Intelligence").
//!
//! A transparent, local, rule-based engine. For files sitting in catch-all
//! locations (Downloads, Desktop, or loose at a location's root) it proposes
//! a destination built from:
//!
//! * the file's category (Documents, Pictures, Videos, …),
//! * topic keywords in the file name (e.g. "security", "invoice", "tax"),
//! * document-type keywords ("report", "receipt", "contract", …),
//! * the modification year for photos,
//! * folders that already exist in the user's structure (preferred).
//!
//! Every recommendation carries the signals that produced it and a
//! confidence score. Nothing is ever moved automatically: applying a
//! recommendation produces an ordinary operation plan that the user previews
//! and approves.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::categorization::Category;
use crate::error::AppResult;
use crate::filesystem;
use crate::indexing::{file_from_row, FileRecord, Page, FILE_COLUMNS};
use crate::security::{self, LocationKind};

/// Folder names treated as catch-all inboxes.
const INBOX_NAMES: &[&str] = &[
    "downloads",
    "download",
    "desktop",
    "inbox",
    "unsorted",
    "new folder",
    "temp",
];

const TOPICS: &[(&str, &[&str])] = &[
    (
        "Security",
        &[
            "security",
            "cyber",
            "cybersecurity",
            "vulnerability",
            "vulnerabilities",
            "pentest",
            "threat",
            "malware",
            "firewall",
            "incident",
            "cve",
            "phishing",
            "infosec",
        ],
    ),
    (
        "Finance",
        &[
            "invoice",
            "invoices",
            "receipt",
            "receipts",
            "tax",
            "taxes",
            "bank",
            "statement",
            "budget",
            "payroll",
            "expense",
            "expenses",
            "salary",
            "payslip",
            "w2",
            "1099",
            "billing",
        ],
    ),
    (
        "Career",
        &[
            "resume",
            "cv",
            "coverletter",
            "cover",
            "offer",
            "interview",
            "portfolio",
        ],
    ),
    (
        "Education",
        &[
            "lecture",
            "assignment",
            "homework",
            "syllabus",
            "thesis",
            "exam",
            "course",
            "coursework",
            "semester",
            "tutorial",
            "lab",
        ],
    ),
    (
        "Travel",
        &[
            "ticket",
            "boarding",
            "itinerary",
            "booking",
            "hotel",
            "flight",
            "visa",
            "passport",
            "reservation",
        ],
    ),
    (
        "Health",
        &[
            "medical",
            "prescription",
            "insurance",
            "vaccine",
            "vaccination",
            "doctor",
            "health",
            "clinic",
        ],
    ),
    (
        "Work",
        &[
            "meeting",
            "agenda",
            "minutes",
            "proposal",
            "contract",
            "client",
            "project",
            "roadmap",
            "spec",
            "requirements",
        ],
    ),
    (
        "Legal",
        &[
            "agreement",
            "nda",
            "lease",
            "license",
            "terms",
            "legal",
            "deed",
            "will",
        ],
    ),
];

const DOC_TYPES: &[(&str, &[&str])] = &[
    (
        "Reports",
        &[
            "report",
            "reports",
            "analysis",
            "assessment",
            "audit",
            "findings",
        ],
    ),
    ("Invoices", &["invoice", "invoices", "bill"]),
    ("Receipts", &["receipt", "receipts"]),
    ("Statements", &["statement", "statements"]),
    ("Contracts", &["contract", "contracts", "agreement", "nda"]),
    ("Notes", &["notes", "note", "minutes"]),
    (
        "Manuals",
        &["manual", "guide", "handbook", "instructions", "userguide"],
    ),
    ("Presentations", &["slides", "deck", "presentation"]),
];

fn base_folder(cat: Category) -> Option<&'static [&'static str]> {
    Some(match cat {
        Category::Documents => &["Documents"],
        Category::Spreadsheets => &["Documents", "Spreadsheets"],
        Category::Presentations => &["Documents", "Presentations"],
        Category::Images => &["Pictures"],
        Category::Videos => &["Videos"],
        Category::Audio => &["Music"],
        Category::Archives => &["Archives"],
        Category::Installers => &["Installers"],
        Category::Code => &["Code"],
        Category::Development => &["Development"],
        Category::Design => &["Design"],
        Category::Backups => &["Backups"],
        Category::Other => return None,
    })
}

/// Splits a file stem into lowercase words: separators, digits boundaries
/// and camelCase transitions.
pub fn words(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for c in stem.chars() {
        let boundary = !c.is_alphanumeric()
            || prev
                .map(|p| {
                    (p.is_lowercase() && c.is_uppercase())
                        || (p.is_alphabetic() != c.is_alphabetic())
                })
                .unwrap_or(false);
        if boundary && !cur.is_empty() {
            out.push(std::mem::take(&mut cur).to_lowercase());
        }
        if c.is_alphanumeric() {
            cur.push(c);
        }
        prev = Some(c);
    }
    if !cur.is_empty() {
        out.push(cur.to_lowercase());
    }
    out
}

fn detect<'a>(
    table: &'a [(&'a str, &'a [&'a str])],
    words: &[String],
) -> Option<(&'a str, &'a str)> {
    for (label, keys) in table {
        if let Some(k) = keys.iter().find(|k| words.iter().any(|w| w == *k)) {
            return Some((label, k));
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub destination: PathBuf,
    pub confidence: f64,
    pub reason: String,
    pub signals: Vec<String>,
}

/// Input describing a file, decoupled from the database for testability.
pub struct FileFacts<'a> {
    pub name: &'a str,
    pub category: Category,
    pub category_confidence: f64,
    pub modified_year: Option<i32>,
}

/// Existing folders under the organization root, keyed by case-folded name.
pub type FolderIndex = HashMap<String, Vec<PathBuf>>;

/// Pure recommendation function.
pub fn suggest(file: &FileFacts<'_>, root: &Path, existing: &FolderIndex) -> Option<Suggestion> {
    let base = base_folder(file.category)?;
    let stem = Path::new(file.name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ws = words(&stem);
    let mut signals = vec![format!(
        "Category: {} ({:.0}% confidence)",
        file.category.label(),
        file.category_confidence * 100.0
    )];
    let mut confidence = 0.35 + file.category_confidence * 0.25;

    let mut dest = root.to_path_buf();
    for part in base {
        dest.push(part);
    }

    let doc_like = matches!(
        file.category,
        Category::Documents | Category::Spreadsheets | Category::Presentations
    );
    let topic = detect(TOPICS, &ws);
    let doc_type = if doc_like {
        detect(DOC_TYPES, &ws)
    } else {
        None
    };
    let mut used_existing = None;

    if let Some((topic, kw)) = topic {
        signals.push(format!("File name mentions “{kw}” → topic {topic}"));
        confidence += 0.15;
        // Prefer a folder the user already has for this topic.
        if let Some(found) = existing
            .get(&topic.to_lowercase())
            .and_then(|v| v.iter().min_by_key(|p| p.as_os_str().len()))
        {
            dest = found.clone();
            used_existing = Some(found.clone());
        } else {
            dest.push(topic);
        }
    }
    if let Some((ty, kw)) = doc_type {
        if topic.map(|(t, _)| t) != Some(ty) {
            signals.push(format!("“{kw}” indicates the document type {ty}"));
            confidence += 0.1;
            dest.push(ty);
        }
    }
    if file.category == Category::Images {
        if ws
            .iter()
            .any(|w| w == "screenshot" || w == "screenshots" || w == "capture")
            || stem.to_lowercase().starts_with("screen shot")
        {
            signals.push("Looks like a screenshot".into());
            confidence += 0.15;
            dest.push("Screenshots");
        } else if let Some(y) = file.modified_year {
            signals.push(format!("Photo last modified in {y}"));
            confidence += 0.05;
            dest.push(y.to_string());
        }
    }
    if let Some(found) = used_existing {
        confidence += 0.15;
        signals.push(format!(
            "You already have a folder for this: {}",
            found.display()
        ));
    }
    let confidence = confidence.min(0.95);
    let rel = dest
        .strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| dest.display().to_string());
    let reason = match (topic, doc_type) {
        (Some((t, _)), Some((ty, _))) => format!(
            "A {} about {} — belongs with your {} {}",
            file.category.label().to_lowercase().trim_end_matches('s'),
            t.to_lowercase(),
            t,
            ty.to_lowercase()
        ),
        (Some((t, _)), None) => format!(
            "{} related to {} — suggested folder: {}",
            file.category.label(),
            t.to_lowercase(),
            rel
        ),
        (None, Some((ty, _))) => format!(
            "Looks like one of your {} — suggested folder: {}",
            ty.to_lowercase(),
            rel
        ),
        (None, None) => format!("{} usually live in {}", file.category.label(), rel),
    };
    Some(Suggestion {
        destination: dest,
        confidence,
        reason,
        signals,
    })
}

fn is_inbox(dir: &Path, root: &Path) -> bool {
    if filesystem::paths_equal(dir, root) {
        return true;
    }
    dir.file_name()
        .and_then(|n| n.to_str())
        .map(|n| INBOX_NAMES.contains(&n.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn year_of(ts: Option<i64>) -> Option<i32> {
    // Civil-from-days (Howard Hinnant), avoiding a date-library dependency.
    let days = ts?.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    i32::try_from(year).ok()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationSummary {
    pub examined: i64,
    pub generated: i64,
    pub removed: i64,
}

const MAX_CANDIDATES: i64 = 20_000;

/// Regenerates pending recommendations. Read-only with respect to files.
pub fn generate(
    conn: &mut Connection,
    organization_root: Option<&str>,
) -> AppResult<GenerationSummary> {
    let locations: Vec<_> = security::list_locations(conn)?
        .into_iter()
        .filter(|l| l.kind == LocationKind::Scan)
        .collect();
    let org_root = organization_root.map(PathBuf::from);

    // Existing folder names under each relevant root (bounded depth).
    let mut folder_cache: HashMap<String, FolderIndex> = HashMap::new();
    let mut folders_for = |conn: &Connection, root: &Path| -> AppResult<FolderIndex> {
        let key = root.to_string_lossy().into_owned();
        if let Some(f) = folder_cache.get(&key) {
            return Ok(f.clone());
        }
        let mut w = crate::indexing::Where::default();
        crate::indexing::push_under_directory(&mut w, "path", &key);
        let mut stmt = conn.prepare(&format!(
            "SELECT path, name FROM directories {} AND depth <= 6 LIMIT 50000",
            w.sql()
        ))?;
        let mut idx: FolderIndex = HashMap::new();
        for row in stmt.query_map(rusqlite::params_from_iter(w.params.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (path, name) = row?;
            if !INBOX_NAMES.contains(&name.to_lowercase().as_str()) {
                idx.entry(name.to_lowercase())
                    .or_default()
                    .push(PathBuf::from(path));
            }
        }
        folder_cache.insert(key, idx.clone());
        Ok(idx)
    };

    let files: Vec<FileRecord> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT {FILE_COLUMNS} FROM files f
             WHERE f.is_symlink = 0 AND f.is_hidden = 0 AND f.is_system = 0
             ORDER BY f.modified_at DESC LIMIT {MAX_CANDIDATES}"
        ))?;
        let rows = stmt
            .query_map([], file_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let mut keep: Vec<(i64, Suggestion)> = Vec::new();
    let mut examined = 0i64;
    for f in files {
        let path = Path::new(&f.path);
        let Some(loc) = locations
            .iter()
            .find(|l| filesystem::path_starts_with(path, Path::new(&l.path)))
        else {
            continue;
        };
        let loc_root = Path::new(&loc.path);
        let parent = Path::new(&f.parent);
        if !is_inbox(parent, loc_root) {
            continue;
        }
        examined += 1;
        let root = org_root.as_deref().unwrap_or(loc_root);
        let folders = folders_for(conn, root)?;
        let Some(cat) = Category::parse(&f.category) else {
            continue;
        };
        let facts = FileFacts {
            name: &f.name,
            category: cat,
            category_confidence: if f.category_overridden {
                1.0
            } else {
                f.category_confidence
            },
            modified_year: year_of(f.modified_at),
        };
        let Some(s) = suggest(&facts, root, &folders) else {
            continue;
        };
        if s.confidence < 0.45 || filesystem::path_starts_with(parent, &s.destination) {
            continue;
        }
        keep.push((f.id, s));
    }

    let tx = conn.transaction()?;
    tx.execute(
        "CREATE TEMP TABLE IF NOT EXISTS reco_keep(file_id INTEGER PRIMARY KEY)",
        [],
    )?;
    tx.execute("DELETE FROM reco_keep", [])?;
    let mut generated = 0;
    {
        let mut upsert = tx.prepare(
            "INSERT INTO recommendations(file_id, destination, reason, signals, confidence, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', strftime('%s','now'))
             ON CONFLICT(file_id) DO UPDATE SET destination = excluded.destination, reason = excluded.reason,
                signals = excluded.signals, confidence = excluded.confidence,
                status = CASE WHEN recommendations.status = 'dismissed' AND recommendations.destination = excluded.destination
                              THEN 'dismissed' ELSE 'pending' END,
                created_at = excluded.created_at",
        )?;
        let mut mark = tx.prepare("INSERT OR IGNORE INTO reco_keep(file_id) VALUES (?1)")?;
        for (id, s) in &keep {
            let Some(dest) = s.destination.to_str() else {
                continue;
            };
            upsert.execute(params![
                id,
                dest,
                s.reason,
                serde_json::to_string(&s.signals).unwrap_or_default(),
                s.confidence
            ])?;
            mark.execute([id])?;
            generated += 1;
        }
    }
    let removed = tx.execute(
        "DELETE FROM recommendations WHERE status = 'pending' AND file_id NOT IN (SELECT file_id FROM reco_keep)",
        [],
    )?;
    tx.execute("DROP TABLE reco_keep", [])?;
    crate::activity::record(
        &tx,
        crate::activity::kind::RECOMMENDATIONS_GENERATED,
        &format!("Generated {generated} organization suggestions"),
        Some(serde_json::json!({ "examined": examined, "generated": generated })),
        None,
    )?;
    tx.commit()?;
    Ok(GenerationSummary {
        examined,
        generated,
        removed: removed as i64,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub id: i64,
    pub file: FileRecord,
    pub destination: String,
    pub reason: String,
    pub signals: Vec<String>,
    pub confidence: f64,
    pub status: String,
    pub created_at: i64,
}

pub fn list(
    conn: &Connection,
    status: &str,
    min_confidence: f64,
    limit: u32,
    offset: u32,
) -> AppResult<Page<Recommendation>> {
    let status = if matches!(status, "pending" | "applied" | "dismissed") {
        status
    } else {
        "pending"
    };
    let limit = limit.clamp(1, 500);
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM recommendations WHERE status = ?1 AND confidence >= ?2",
        params![status, min_confidence],
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {FILE_COLUMNS}, r.id, r.destination, r.reason, r.signals, r.confidence, r.status, r.created_at
         FROM recommendations r JOIN files f ON f.id = r.file_id
         WHERE r.status = ?1 AND r.confidence >= ?2
         ORDER BY r.confidence DESC, f.name LIMIT {limit} OFFSET {offset}"
    ))?;
    let items = stmt
        .query_map(params![status, min_confidence], |r| {
            let file = file_from_row(r)?;
            let signals: String = r.get(22)?;
            Ok(Recommendation {
                file,
                id: r.get(19)?,
                destination: r.get(20)?,
                reason: r.get(21)?,
                signals: serde_json::from_str(&signals).unwrap_or_default(),
                confidence: r.get(23)?,
                status: r.get(24)?,
                created_at: r.get(25)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Page {
        items,
        total,
        offset,
        limit,
    })
}

pub fn set_status(conn: &Connection, ids: &[i64], status: &str) -> AppResult<usize> {
    if !matches!(status, "pending" | "dismissed") {
        return Err(crate::error::AppError::invalid("invalid status"));
    }
    let mut n = 0;
    for id in ids.iter().take(10_000) {
        n += conn.execute(
            "UPDATE recommendations SET status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
    }
    Ok(n)
}

/// Marks recommendations whose files were moved by an executed batch.
pub fn mark_applied_for_batch(conn: &Connection, batch_id: i64) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE recommendations SET status = 'applied'
         WHERE status = 'pending' AND file_id IN (
            SELECT f.id FROM files f JOIN operation_items i ON i.destination = f.path
            WHERE i.batch_id = ?1 AND i.status = 'done' AND i.op = 'move')",
        [batch_id],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(name: &str, cat: Category) -> FileFacts<'_> {
        FileFacts {
            name,
            category: cat,
            category_confidence: 0.8,
            modified_year: Some(2024),
        }
    }

    #[test]
    fn splits_words() {
        assert_eq!(
            words("network-security-report"),
            vec!["network", "security", "report"]
        );
        assert_eq!(
            words("Q3TaxInvoice_2024"),
            vec!["q", "3", "tax", "invoice", "2024"]
        );
        assert_eq!(
            words("Screen Shot 2024-01-01 at 10.00.00"),
            vec!["screen", "shot", "2024", "01", "01", "at", "10", "00", "00"]
        );
    }

    #[test]
    fn security_report_example_from_the_spec() {
        let root = std::env::temp_dir();
        let s = suggest(
            &facts("network-security-report.pdf", Category::Documents),
            &root,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(
            s.destination,
            root.join("Documents").join("Security").join("Reports")
        );
        assert!(s.signals.iter().any(|x| x.contains("security")));
        assert!(s.signals.iter().any(|x| x.contains("Reports")));
        assert!(s.confidence > 0.7);
    }

    #[test]
    fn prefers_existing_folders() {
        let root = std::env::temp_dir();
        let mut existing = HashMap::new();
        let mine = root.join("Work").join("Finance");
        existing.insert("finance".to_string(), vec![mine.clone()]);
        let s = suggest(
            &facts("march-invoice.pdf", Category::Documents),
            &root,
            &existing,
        )
        .unwrap();
        assert_eq!(s.destination, mine.join("Invoices"));
        assert!(s
            .signals
            .iter()
            .any(|x| x.contains("already have a folder")));
    }

    #[test]
    fn photos_screenshots_and_other() {
        let root = std::env::temp_dir();
        let s = suggest(
            &facts("IMG_1234.jpg", Category::Images),
            &root,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(s.destination, root.join("Pictures").join("2024"));
        let s = suggest(
            &facts("Screenshot 2024-05-01.png", Category::Images),
            &root,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(s.destination, root.join("Pictures").join("Screenshots"));
        assert!(suggest(
            &facts("mystery.qqq", Category::Other),
            &root,
            &HashMap::new()
        )
        .is_none());
    }

    #[test]
    fn year_conversion() {
        assert_eq!(year_of(Some(0)), Some(1970));
        assert_eq!(year_of(Some(1_704_067_200)), Some(2024)); // 2024-01-01
        assert_eq!(year_of(Some(1_704_067_199)), Some(2023));
        assert_eq!(year_of(None), None);
    }

    #[test]
    fn generation_only_targets_inbox_files_and_never_touches_disk() {
        use crate::database::test_db;
        use crate::scanner::{run_scan, ScanOptions};
        use std::sync::atomic::AtomicBool;
        let (_d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        std::fs::create_dir_all(r.join("Downloads")).unwrap();
        std::fs::create_dir_all(r.join("Projects/Security")).unwrap();
        std::fs::write(r.join("Downloads/network-security-report.pdf"), b"%PDF").unwrap();
        std::fs::write(r.join("Downloads/holiday.jpg"), b"jpg").unwrap();
        std::fs::write(r.join("Projects/notes.txt"), b"organized already").unwrap();
        db.with(|c| {
            let loc = security::add_location(c, r, LocationKind::Scan)?;
            run_scan(
                c,
                &loc,
                &ScanOptions::default(),
                &AtomicBool::new(false),
                &|_| {},
            )?;
            let g = generate(c, None)?;
            assert_eq!(g.generated, 2);
            let page = list(c, "pending", 0.0, 50, 0)?;
            let report = page
                .items
                .iter()
                .find(|x| x.file.name.starts_with("network"))
                .unwrap();
            // Uses the existing "Security" folder, then the document type.
            assert!(
                report.destination.ends_with(&format!(
                    "Projects{0}Security{0}Reports",
                    std::path::MAIN_SEPARATOR
                )),
                "{}",
                report.destination
            );
            assert!(!page.items.iter().any(|x| x.file.name == "notes.txt"));

            set_status(c, &[report.id], "dismissed")?;
            generate(c, None)?;
            assert_eq!(
                list(c, "pending", 0.0, 50, 0)?.total,
                1,
                "dismissed suggestions stay dismissed"
            );
            Ok(())
        })
        .unwrap();
        assert!(r.join("Downloads/network-security-report.pdf").exists());
    }
}
