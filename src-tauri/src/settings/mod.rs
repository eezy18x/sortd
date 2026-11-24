//! Persistent, validated application settings.
//!
//! Every field here drives real behaviour; there are no decorative settings.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::categorization::Category;
use crate::error::{AppError, AppResult};

const KEY: &str = "app";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// "system", "dark" or "light".
    pub theme: String,
    /// Apply the platform's built-in system-location exclusions.
    pub use_default_exclusions: bool,
    /// Additional absolute paths never scanned.
    pub excluded_paths: Vec<String>,
    /// Directory names skipped at any depth (e.g. "node_modules").
    pub excluded_names: Vec<String>,
    /// Index hidden files and folders.
    pub include_hidden: bool,
    /// Descend into other mounted filesystems below a scan root.
    pub cross_filesystems: bool,
    /// Worker threads used for hashing (duplicate detection).
    pub hash_concurrency: u32,
    /// Rescan all locations when SORTD starts.
    pub scan_on_startup: bool,
    /// Run duplicate detection automatically after each completed scan.
    pub detect_duplicates_after_scan: bool,
    /// Files smaller than this are ignored by duplicate detection.
    pub duplicate_min_size: u64,
    /// Read a file's first bytes to verify its type in the details view.
    pub verify_content_type: bool,
    /// User extension → category rules, applied before built-in rules.
    pub category_rules: Vec<CategoryRule>,
    /// Ask for confirmation before executing even a single-item operation.
    pub confirm_single_operations: bool,
    /// Default conflict policy for new operation previews: "skip" or "rename".
    pub default_conflict_policy: String,
    /// Remember search queries locally.
    pub record_search_history: bool,
    /// Base folder for organization suggestions. Must be an authorized location.
    pub organization_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryRule {
    pub extension: String,
    pub category: Category,
}

impl Default for Settings {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(2);
        Settings {
            theme: "system".into(),
            use_default_exclusions: true,
            excluded_paths: Vec::new(),
            excluded_names: Vec::new(),
            include_hidden: true,
            cross_filesystems: false,
            hash_concurrency: cpus.clamp(1, 4),
            scan_on_startup: false,
            detect_duplicates_after_scan: true,
            duplicate_min_size: 1,
            verify_content_type: true,
            category_rules: Vec::new(),
            confirm_single_operations: true,
            default_conflict_policy: "skip".into(),
            record_search_history: true,
            organization_root: None,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> AppResult<()> {
        if !matches!(self.theme.as_str(), "system" | "dark" | "light") {
            return Err(AppError::invalid("theme must be system, dark or light"));
        }
        if !(1..=32).contains(&self.hash_concurrency) {
            return Err(AppError::invalid(
                "hash concurrency must be between 1 and 32",
            ));
        }
        if !matches!(self.default_conflict_policy.as_str(), "skip" | "rename") {
            return Err(AppError::invalid("conflict policy must be skip or rename"));
        }
        for p in &self.excluded_paths {
            if crate::filesystem::normalize_absolute(Path::new(p)).is_none() {
                return Err(AppError::invalid(format!(
                    "excluded path must be absolute: {p}"
                )));
            }
        }
        for n in &self.excluded_names {
            crate::filesystem::validate_file_name(n)
                .map_err(|e| AppError::invalid(format!("excluded name '{n}': {e}")))?;
        }
        for r in &self.category_rules {
            let ext = r.extension.trim_start_matches('.');
            if ext.is_empty()
                || ext.len() > 32
                || !ext
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Err(AppError::invalid(format!(
                    "invalid extension in category rule: {}",
                    r.extension
                )));
            }
        }
        if self.excluded_paths.len() > 1000
            || self.excluded_names.len() > 1000
            || self.category_rules.len() > 1000
        {
            return Err(AppError::invalid("too many entries"));
        }
        Ok(())
    }

    /// Normalizes user input (lowercases extensions, trims whitespace).
    pub fn normalized(mut self) -> Self {
        for r in &mut self.category_rules {
            r.extension = r
                .extension
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
        }
        self.excluded_names = self
            .excluded_names
            .iter()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        self.excluded_paths = self
            .excluded_paths
            .iter()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        self
    }

    /// The full exclusion set (paths, names) used by the scanner.
    pub fn exclusions(&self) -> (Vec<PathBuf>, Vec<String>) {
        let mut paths: Vec<PathBuf> = self.excluded_paths.iter().map(PathBuf::from).collect();
        let mut names = self.excluded_names.clone();
        if self.use_default_exclusions {
            paths.extend(crate::filesystem::default_excluded_paths());
            names.extend(
                crate::filesystem::default_excluded_names()
                    .iter()
                    .map(|s| s.to_string()),
            );
        }
        (paths, names)
    }
}

pub fn load(conn: &Connection) -> AppResult<Settings> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [KEY], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(match raw {
        // Unknown or malformed values fall back to defaults field-by-field.
        Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        None => Settings::default(),
    })
}

pub fn save(conn: &Connection, settings: &Settings) -> AppResult<()> {
    settings.validate()?;
    let json = serde_json::to_string(settings).map_err(|e| AppError::Internal(e.to_string()))?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![KEY, json],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_db;

    #[test]
    fn defaults_round_trip() {
        let (_d, db) = test_db();
        db.with(|c| {
            assert_eq!(load(c)?, Settings::default());
            let s = Settings {
                theme: "dark".into(),
                excluded_names: vec!["node_modules".into()],
                ..Default::default()
            };
            save(c, &s)?;
            assert_eq!(load(c)?, s);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn rejects_invalid_values() {
        let s = Settings {
            theme: "neon".into(),
            ..Default::default()
        };
        assert!(s.validate().is_err());
        let s = Settings {
            hash_concurrency: 0,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.excluded_paths.push("relative/dir".into());
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.excluded_names.push("a/b".into());
        assert!(s.validate().is_err());
    }

    #[test]
    fn partial_json_uses_defaults_for_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(s.theme, "light");
        assert!(s.use_default_exclusions);
    }
}
