//! Local search over the index.
//!
//! Uses an FTS5 trigram index for substring matching on names (and
//! optionally full paths), combined with structured filters. A small query
//! language lets power users filter from the search box:
//!
//! | token              | meaning                                   |
//! |--------------------|-------------------------------------------|
//! | `ext:pdf,docx`     | extension is one of                       |
//! | `cat:images`       | category (also `category:` / `type:`)     |
//! | `size>10mb`        | size comparison (`>`, `>=`, `<`, `<=`)    |
//! | `newer:7d`         | modified within the last 7 days (d/w/m/y) |
//! | `older:1y`         | modified more than a year ago             |
//! | `in:path`          | match text against the full path          |
//! | `"exact phrase"`   | phrase (spaces preserved)                 |
//!
//! Search content is never sent anywhere; history is stored locally only
//! when enabled in Settings.

use rusqlite::types::Value;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::categorization::Category;
use crate::error::{AppError, AppResult};
use crate::indexing::{
    clamp_limit, file_from_row, order_by, FileFilter, FileRecord, Page, SortField, SortOrder,
    Where, FILE_COLUMNS,
};
use crate::util::{escape_like, now};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedQuery {
    pub terms: Vec<String>,
    pub extensions: Vec<String>,
    pub categories: Vec<String>,
    pub min_size: Option<i64>,
    pub max_size: Option<i64>,
    pub modified_after: Option<i64>,
    pub modified_before: Option<i64>,
    pub in_path: bool,
}

fn parse_size(s: &str) -> Option<i64> {
    let s = s.trim().to_ascii_lowercase();
    let idx = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(idx);
    let n: f64 = num.parse().ok()?;
    let mult: f64 = match unit.trim() {
        "" | "b" => 1.0,
        "k" | "kb" | "kib" => 1024.0,
        "m" | "mb" | "mib" => 1024.0 * 1024.0,
        "g" | "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" | "tib" => 1024.0_f64.powi(4),
        _ => return None,
    };
    let v = n * mult;
    (v.is_finite() && v >= 0.0 && v < i64::MAX as f64).then_some(v as i64)
}

fn parse_age(s: &str) -> Option<i64> {
    let s = s.trim().to_ascii_lowercase();
    let (num, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: i64 = num.parse().ok()?;
    let days = match unit {
        "d" => 1,
        "w" => 7,
        "m" => 30,
        "y" => 365,
        _ => return None,
    };
    n.checked_mul(days * 86_400)
}

/// Splits on whitespace, keeping double-quoted phrases together.
fn tokenize(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in q.chars() {
        match c {
            '"' => {
                if quoted || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter().filter(|t| !t.trim().is_empty()).collect()
}

pub fn parse_query(raw: &str) -> ParsedQuery {
    let mut p = ParsedQuery::default();
    let t0 = now();
    for tok in tokenize(raw) {
        let lower = tok.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("ext:") {
            p.extensions.extend(
                v.split(',')
                    .map(|e| e.trim().trim_start_matches('.').to_string())
                    .filter(|e| !e.is_empty()),
            );
            continue;
        }
        if let Some(v) = lower
            .strip_prefix("cat:")
            .or_else(|| lower.strip_prefix("category:"))
            .or_else(|| lower.strip_prefix("type:"))
        {
            let mut matched = false;
            for c in v.split(',') {
                if let Some(cat) = Category::parse(c.trim()) {
                    p.categories.push(cat.as_str().to_string());
                    matched = true;
                }
            }
            if matched {
                continue;
            }
        }
        if lower == "in:path" {
            p.in_path = true;
            continue;
        }
        if let Some(rest) = lower.strip_prefix("size") {
            let parsed = if let Some(v) = rest.strip_prefix(">=") {
                parse_size(v).map(|n| p.min_size = Some(n))
            } else if let Some(v) = rest.strip_prefix('>') {
                parse_size(v).map(|n| p.min_size = Some(n.saturating_add(1)))
            } else if let Some(v) = rest.strip_prefix("<=") {
                parse_size(v).map(|n| p.max_size = Some(n))
            } else if let Some(v) = rest.strip_prefix('<') {
                parse_size(v).map(|n| p.max_size = Some(n.saturating_sub(1)))
            } else {
                None
            };
            if parsed.is_some() {
                continue;
            }
        }
        if let Some(v) = lower.strip_prefix("newer:") {
            if let Some(secs) = parse_age(v) {
                p.modified_after = Some(t0 - secs);
                continue;
            }
        }
        if let Some(v) = lower.strip_prefix("older:") {
            if let Some(secs) = parse_age(v) {
                p.modified_before = Some(t0 - secs);
                continue;
            }
        }
        p.terms.push(tok);
    }
    p
}

/// Quotes a term as an FTS5 string literal.
fn fts_phrase(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchRequest {
    pub query: String,
    pub filter: FileFilter,
    pub sort: Option<SortField>,
    pub order: Option<SortOrder>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub page: Page<FileRecord>,
    pub elapsed_ms: u64,
}

pub fn search(conn: &Connection, req: &SearchRequest) -> AppResult<SearchResponse> {
    let started = std::time::Instant::now();
    if req.query.len() > 1000 {
        return Err(AppError::invalid("search query is too long"));
    }
    req.filter.validate()?;
    let parsed = parse_query(&req.query);

    // Merge query-language filters with explicit UI filters.
    let mut filter = req.filter.clone();
    filter.extensions.extend(parsed.extensions.iter().cloned());
    filter.categories.extend(parsed.categories.iter().cloned());
    filter.min_size = filter.min_size.max(parsed.min_size);
    filter.max_size = match (filter.max_size, parsed.max_size) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    filter.modified_after = filter.modified_after.max(parsed.modified_after);
    filter.modified_before = match (filter.modified_before, parsed.modified_before) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };

    let mut w = Where::default();
    filter.apply(&mut w);

    let column = if parsed.in_path { "path" } else { "name" };
    let fts_terms: Vec<&String> = parsed
        .terms
        .iter()
        .filter(|t| t.chars().count() >= 3)
        .collect();
    let short_terms: Vec<&String> = parsed
        .terms
        .iter()
        .filter(|t| t.chars().count() < 3)
        .collect();
    for t in &short_terms {
        w.push(
            format!("f.{column} LIKE ? ESCAPE '\\'"),
            [Value::Text(format!("%{}%", escape_like(t)))],
        );
    }
    let use_fts = !fts_terms.is_empty();
    let from = if use_fts {
        let expr = fts_terms
            .iter()
            .map(|t| fts_phrase(t))
            .collect::<Vec<_>>()
            .join(" AND ");
        let mut clauses = vec!["files_fts MATCH ?".to_string()];
        clauses.append(&mut w.clauses);
        let mut params = vec![Value::Text(format!("{column} : ({expr})"))];
        params.append(&mut w.params);
        w.clauses = clauses;
        w.params = params;
        "files f JOIN files_fts ON files_fts.rowid = f.id"
    } else {
        "files f"
    };

    let sort = req.sort.unwrap_or(if use_fts {
        SortField::Relevance
    } else {
        SortField::Modified
    });
    let order = req.order.unwrap_or_default();
    let order_sql = if sort == SortField::Relevance && use_fts {
        "bm25(files_fts), length(f.name), f.id"
    } else {
        order_by(sort, order)
    };

    let limit = clamp_limit(if req.limit == 0 { 100 } else { req.limit });
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM {from} {}", w.sql()),
        rusqlite::params_from_iter(w.params.iter()),
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {FILE_COLUMNS} FROM {from} {} ORDER BY {order_sql} LIMIT {limit} OFFSET {}",
        w.sql(),
        req.offset
    ))?;
    let items = stmt
        .query_map(rusqlite::params_from_iter(w.params.iter()), file_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SearchResponse {
        page: Page {
            items,
            total,
            offset: req.offset,
            limit,
        },
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub text: String,
    /// "history" or "file".
    pub source: String,
}

pub fn suggestions(
    conn: &Connection,
    prefix: &str,
    include_history: bool,
) -> AppResult<Vec<Suggestion>> {
    let prefix = prefix.trim();
    let mut out = Vec::new();
    if include_history {
        let mut stmt = conn.prepare(
            "SELECT query FROM search_history WHERE query LIKE ?1 ESCAPE '\\' ORDER BY ts DESC LIMIT 5",
        )?;
        let pat = format!("{}%", escape_like(prefix));
        for q in stmt.query_map([pat], |r| r.get::<_, String>(0))? {
            out.push(Suggestion {
                text: q?,
                source: "history".into(),
            });
        }
    }
    if prefix.chars().count() >= 3 && prefix.len() <= 200 {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT f.name FROM files_fts JOIN files f ON f.id = files_fts.rowid
             WHERE files_fts MATCH ?1 ORDER BY length(f.name) LIMIT 8",
        )?;
        for n in stmt.query_map([format!("name : {}", fts_phrase(prefix))], |r| {
            r.get::<_, String>(0)
        })? {
            let n = n?;
            if !out.iter().any(|s| s.text == n) {
                out.push(Suggestion {
                    text: n,
                    source: "file".into(),
                });
            }
        }
    }
    Ok(out)
}

pub fn record_history(conn: &Connection, query: &str) -> AppResult<()> {
    let q = query.trim();
    if q.is_empty() || q.len() > 500 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO search_history(query, ts) VALUES (?1, ?2) ON CONFLICT(query) DO UPDATE SET ts = excluded.ts",
        params![q, now()],
    )?;
    // Keep the history bounded.
    conn.execute(
        "DELETE FROM search_history WHERE id NOT IN (SELECT id FROM search_history ORDER BY ts DESC LIMIT 50)",
        [],
    )?;
    Ok(())
}

pub fn list_history(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT query FROM search_history ORDER BY ts DESC LIMIT 20")?;
    let rows = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn clear_history(conn: &Connection) -> AppResult<()> {
    conn.execute("DELETE FROM search_history", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_db;
    use crate::scanner::{run_scan, ScanOptions};
    use crate::security::{add_location, LocationKind};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn parses_query_language() {
        let p = parse_query(
            r#"budget ext:pdf,.XLSX cat:documents size>1mb size<=2GB newer:7d "q3 report" in:path"#,
        );
        assert_eq!(p.terms, vec!["budget", "q3 report"]);
        assert_eq!(p.extensions, vec!["pdf", "xlsx"]);
        assert_eq!(p.categories, vec!["documents"]);
        assert_eq!(p.min_size, Some(1024 * 1024 + 1));
        assert_eq!(p.max_size, Some(2 * 1024 * 1024 * 1024));
        assert!(p.modified_after.is_some());
        assert!(p.in_path);
    }

    #[test]
    fn unknown_operators_are_plain_terms() {
        let p = parse_query("cat:nonsense size>huge");
        assert_eq!(p.terms, vec!["cat:nonsense", "size>huge"]);
    }

    fn setup() -> (
        tempfile::TempDir,
        crate::database::Database,
        tempfile::TempDir,
    ) {
        let (d, db) = test_db();
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        std::fs::create_dir_all(r.join("Work/Security")).unwrap();
        std::fs::write(
            r.join("Work/Security/network-security-report.pdf"),
            vec![0u8; 2000],
        )
        .unwrap();
        std::fs::write(r.join("Work/budget-2024.xlsx"), vec![0u8; 500]).unwrap();
        std::fs::write(r.join("holiday photo.jpg"), vec![0u8; 3000]).unwrap();
        std::fs::write(r.join("a.md"), b"x").unwrap();
        std::fs::write(r.join("Отчёт.docx"), b"x").unwrap();
        db.with(|c| {
            let loc = add_location(c, r, LocationKind::Scan)?;
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
        (d, db, root)
    }

    fn names(db: &crate::database::Database, q: &str) -> Vec<String> {
        db.with(|c| {
            let r = search(
                c,
                &SearchRequest {
                    query: q.into(),
                    ..Default::default()
                },
            )?;
            Ok(r.page.items.into_iter().map(|f| f.name).collect())
        })
        .unwrap()
    }

    #[test]
    fn substring_search_on_names() {
        let (_d, db, _r) = setup();
        assert_eq!(names(&db, "secur"), vec!["network-security-report.pdf"]);
        assert_eq!(names(&db, "REPORT"), vec!["network-security-report.pdf"]);
        assert_eq!(names(&db, "photo holiday"), vec!["holiday photo.jpg"]);
        assert_eq!(names(&db, "\"holiday photo\""), vec!["holiday photo.jpg"]);
        assert!(names(&db, "nonexistent").is_empty());
        // Short terms fall back to LIKE.
        assert_eq!(names(&db, "a.").len(), 1);
        assert_eq!(names(&db, "Отч"), vec!["Отчёт.docx"]);
    }

    #[test]
    fn filters_and_path_search() {
        let (_d, db, _r) = setup();
        assert_eq!(names(&db, "ext:xlsx"), vec!["budget-2024.xlsx"]);
        assert_eq!(names(&db, "cat:images"), vec!["holiday photo.jpg"]);
        let mut big = names(&db, "size>1kb");
        big.sort();
        assert_eq!(
            big,
            vec!["holiday photo.jpg", "network-security-report.pdf"]
        );
        let mut work = names(&db, "work in:path");
        work.sort();
        assert_eq!(
            work,
            vec!["budget-2024.xlsx", "network-security-report.pdf"]
        );
        // Name-only search does not match folder names: budget-2024.xlsx is
        // only *inside* "Work"; "net-work-security…" matches by its own name.
        assert_eq!(names(&db, "work"), vec!["network-security-report.pdf"]);
    }

    #[test]
    fn fts_syntax_in_user_input_is_neutralized() {
        let (_d, db, _r) = setup();
        for q in [
            "\"",
            "NEAR(",
            "a OR b",
            "*",
            "name:x",
            "(((",
            "'; DROP TABLE files; --",
        ] {
            db.with(|c| {
                search(
                    c,
                    &SearchRequest {
                        query: q.into(),
                        ..Default::default()
                    },
                )?;
                Ok(())
            })
            .unwrap_or_else(|e| panic!("query {q:?} failed: {e}"));
        }
        assert_eq!(names(&db, "").len(), 5);
    }

    #[test]
    fn suggestions_and_history() {
        let (_d, db, _r) = setup();
        db.with(|c| {
            record_history(c, "budget")?;
            record_history(c, "budget")?;
            assert_eq!(list_history(c)?, vec!["budget"]);
            let s = suggestions(c, "bud", true)?;
            assert_eq!(s[0].source, "history");
            assert!(s.iter().any(|x| x.text == "budget-2024.xlsx"));
            let s = suggestions(c, "bud", false)?;
            assert!(s.iter().all(|x| x.source == "file"));
            clear_history(c)?;
            assert!(list_history(c)?.is_empty());
            Ok(())
        })
        .unwrap();
    }
}
