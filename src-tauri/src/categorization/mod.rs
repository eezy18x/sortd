//! Rule-based file classification.
//!
//! File extensions and extension-derived MIME types are treated as
//! *untrusted hints*: they determine an initial category with a stated
//! confidence. When the user opens a file's details, SORTD can read the first
//! bytes of the file (never executing it) and compare the content signature
//! against the hint, raising or lowering confidence accordingly.

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::settings::CategoryRule;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Documents,
    Images,
    Videos,
    Audio,
    Archives,
    Installers,
    Development,
    Code,
    Spreadsheets,
    Presentations,
    Design,
    Backups,
    Other,
}

impl Category {
    pub const ALL: [Category; 13] = [
        Category::Documents,
        Category::Images,
        Category::Videos,
        Category::Audio,
        Category::Archives,
        Category::Installers,
        Category::Development,
        Category::Code,
        Category::Spreadsheets,
        Category::Presentations,
        Category::Design,
        Category::Backups,
        Category::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Documents => "documents",
            Category::Images => "images",
            Category::Videos => "videos",
            Category::Audio => "audio",
            Category::Archives => "archives",
            Category::Installers => "installers",
            Category::Development => "development",
            Category::Code => "code",
            Category::Spreadsheets => "spreadsheets",
            Category::Presentations => "presentations",
            Category::Design => "design",
            Category::Backups => "backups",
            Category::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Category> {
        Category::ALL.iter().copied().find(|c| c.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Documents => "Documents",
            Category::Images => "Images",
            Category::Videos => "Videos",
            Category::Audio => "Audio",
            Category::Archives => "Archives",
            Category::Installers => "Installers",
            Category::Development => "Development",
            Category::Code => "Code",
            Category::Spreadsheets => "Spreadsheets",
            Category::Presentations => "Presentations",
            Category::Design => "Design",
            Category::Backups => "Backups",
            Category::Other => "Other",
        }
    }
}

/// Built-in extension table. Order matters only for readability.
const EXTENSIONS: &[(Category, &[&str])] = &[
    (
        Category::Documents,
        &[
            "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "markdown", "tex", "epub", "mobi",
            "pages", "wpd", "xps", "djvu", "rst", "org",
        ],
    ),
    (
        Category::Spreadsheets,
        &[
            "xls", "xlsx", "xlsm", "xlsb", "ods", "csv", "tsv", "numbers",
        ],
    ),
    (
        Category::Presentations,
        &["ppt", "pptx", "pps", "ppsx", "odp", "key"],
    ),
    (
        Category::Images,
        &[
            "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "avif",
            "svg", "ico", "raw", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "jxl",
        ],
    ),
    (
        Category::Videos,
        &[
            "mp4", "mkv", "mov", "avi", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "3gp", "ts",
            "mts", "m2ts", "vob", "ogv",
        ],
    ),
    (
        Category::Audio,
        &[
            "mp3", "wav", "flac", "aac", "ogg", "oga", "opus", "m4a", "wma", "aiff", "aif", "alac",
            "mid", "midi", "amr",
        ],
    ),
    (
        Category::Archives,
        &[
            "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "lz",
            "lzma", "cab", "iso", "img",
        ],
    ),
    (
        Category::Installers,
        &[
            "exe",
            "msi",
            "msix",
            "msixbundle",
            "appx",
            "dmg",
            "pkg",
            "deb",
            "rpm",
            "appimage",
            "apk",
            "flatpakref",
            "snap",
        ],
    ),
    (
        Category::Code,
        &[
            "rs", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "java", "kt", "kts", "c", "h",
            "cpp", "cc", "cxx", "hpp", "cs", "go", "rb", "php", "swift", "m", "mm", "scala", "lua",
            "sh", "bash", "zsh", "ps1", "bat", "cmd", "sql", "r", "dart", "ex", "exs", "erl", "hs",
            "clj", "vue", "svelte", "html", "htm", "css", "scss", "sass", "less", "zig", "nim",
            "pl", "groovy",
        ],
    ),
    (
        Category::Development,
        &[
            "json", "yaml", "yml", "toml", "ini", "cfg", "conf", "xml", "lock", "sln", "csproj",
            "vcxproj", "gradle", "env", "dll", "so", "dylib", "o", "obj", "a", "lib", "class",
            "pyc", "wasm", "pdb", "db", "sqlite", "sqlite3", "ipynb", "proto", "patch", "diff",
            "jar", "map",
        ],
    ),
    (
        Category::Design,
        &[
            "psd", "psb", "ai", "eps", "sketch", "fig", "xd", "afdesign", "afphoto", "afpub",
            "indd", "cdr", "xcf", "blend", "fbx", "stl", "dwg", "dxf", "kra",
        ],
    ),
    (
        Category::Backups,
        &[
            "bak", "bkp", "backup", "old", "orig", "tib", "vbk", "wbcat", "sav", "bkf",
        ],
    ),
];

/// Well-known file names that identify development files without extensions.
const DEV_NAMES: &[&str] = &[
    "dockerfile",
    "makefile",
    "cmakelists.txt",
    "gemfile",
    "rakefile",
    "procfile",
    "cargo.toml",
    "package.json",
    ".gitignore",
    ".editorconfig",
    "justfile",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    pub category: Category,
    /// 0.0 – 1.0.
    pub confidence: f64,
    /// Human-readable explanation, most important first.
    pub reasons: Vec<String>,
    /// MIME type implied by the extension (a hint, not proof).
    pub mime_hint: Option<String>,
}

fn by_extension(ext: &str) -> Option<Category> {
    EXTENSIONS
        .iter()
        .find(|(_, exts)| exts.contains(&ext))
        .map(|(c, _)| *c)
}

fn by_mime(mime: &str) -> Option<Category> {
    let top = mime.split('/').next().unwrap_or("");
    match top {
        "image" => Some(Category::Images),
        "video" => Some(Category::Videos),
        "audio" => Some(Category::Audio),
        "text" => Some(Category::Documents),
        "font" => Some(Category::Design),
        _ => None,
    }
}

/// Classifies a file from its name and extension.
pub fn classify(name: &str, ext: Option<&str>, rules: &[CategoryRule]) -> Classification {
    let lower = name.to_lowercase();
    let mime_hint = ext
        .and_then(|e| mime_guess::from_ext(e).first_raw())
        .map(str::to_string);

    if let Some(e) = ext {
        if let Some(rule) = rules.iter().find(|r| r.extension == e) {
            return Classification {
                category: rule.category,
                confidence: 1.0,
                reasons: vec![format!(
                    "Matches your rule: .{e} files are {}",
                    rule.category.label()
                )],
                mime_hint,
            };
        }
    }

    // A name that says "backup" is a strong signal regardless of extension.
    let backup_named =
        ["backup", ".bak.", "~$"].iter().any(|k| lower.contains(k)) || lower.ends_with('~');

    if let Some(e) = ext {
        if let Some(cat) = by_extension(e) {
            let mut reasons = vec![format!(
                ".{e} is a known {} format",
                cat.label().to_lowercase()
            )];
            if backup_named && cat != Category::Backups {
                reasons.insert(0, "The file name indicates a backup copy".into());
                return Classification {
                    category: Category::Backups,
                    confidence: 0.7,
                    reasons,
                    mime_hint,
                };
            }
            reasons.push("Based on the extension only; the content has not been inspected".into());
            return Classification {
                category: cat,
                confidence: 0.8,
                reasons,
                mime_hint,
            };
        }
    }

    if DEV_NAMES.contains(&lower.as_str()) {
        return Classification {
            category: Category::Development,
            confidence: 0.8,
            reasons: vec![format!("'{name}' is a well-known development file name")],
            mime_hint,
        };
    }

    if backup_named {
        return Classification {
            category: Category::Backups,
            confidence: 0.6,
            reasons: vec!["The file name indicates a backup copy".into()],
            mime_hint,
        };
    }

    if let Some(cat) = mime_hint.as_deref().and_then(by_mime) {
        return Classification {
            category: cat,
            confidence: 0.6,
            reasons: vec![format!(
                "The extension maps to the MIME type {}, which is usually {}",
                mime_hint.as_deref().unwrap_or(""),
                cat.label().to_lowercase()
            )],
            mime_hint,
        };
    }

    Classification {
        category: Category::Other,
        confidence: 0.3,
        reasons: vec![match ext {
            Some(e) => format!(".{e} is not a recognized format"),
            None => "The file has no extension".into(),
        }],
        mime_hint,
    }
}

/// Result of inspecting a file's leading bytes.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContentSignature {
    pub mime: String,
    pub extension: String,
}

/// Maximum number of bytes read for signature detection.
const SNIFF_BYTES: u64 = 8192;

/// Reads at most the first 8 KiB of a regular file and detects its type from
/// magic numbers. Never follows symlinks, never executes anything.
pub fn sniff(path: &Path) -> std::io::Result<Option<ContentSignature>> {
    let file = crate::filesystem::open_for_read(path)?;
    let mut buf = Vec::with_capacity(SNIFF_BYTES as usize);
    file.take(SNIFF_BYTES).read_to_end(&mut buf)?;
    Ok(infer::get(&buf).map(|t| ContentSignature {
        mime: t.mime_type().to_string(),
        extension: t.extension().to_string(),
    }))
}

/// Refines a classification with a content signature.
pub fn refine(
    mut c: Classification,
    sig: Option<&ContentSignature>,
    user_rule: bool,
) -> Classification {
    if user_rule {
        return c;
    }
    let Some(sig) = sig else {
        c.reasons
            .push("No known content signature was found in the first bytes".into());
        return c;
    };
    let sig_cat = by_extension(&sig.extension).or_else(|| by_mime(&sig.mime));
    // Office documents, JARs, EPUBs etc. are ZIP containers: a ZIP signature
    // is consistent with them.
    let container_ok = sig.extension == "zip"
        && matches!(
            c.category,
            Category::Documents
                | Category::Spreadsheets
                | Category::Presentations
                | Category::Development
                | Category::Design
                | Category::Installers
        );
    if sig_cat == Some(c.category) || container_ok {
        c.confidence = c.confidence.max(0.95);
        c.reasons.insert(
            0,
            format!("Content signature ({}) confirms the type", sig.mime),
        );
    } else if let Some(actual) = sig_cat {
        c.reasons.insert(
            0,
            format!(
                "Content signature ({}) suggests {}, which does not match the extension",
                sig.mime,
                actual.label().to_lowercase()
            ),
        );
        c.category = actual;
        c.confidence = 0.75;
    } else {
        c.reasons
            .push(format!("Content signature detected: {}", sig.mime));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extensions() {
        assert_eq!(
            classify("a.pdf", Some("pdf"), &[]).category,
            Category::Documents
        );
        assert_eq!(
            classify("a.xlsx", Some("xlsx"), &[]).category,
            Category::Spreadsheets
        );
        assert_eq!(
            classify("a.pptx", Some("pptx"), &[]).category,
            Category::Presentations
        );
        assert_eq!(
            classify("a.png", Some("png"), &[]).category,
            Category::Images
        );
        assert_eq!(
            classify("a.mkv", Some("mkv"), &[]).category,
            Category::Videos
        );
        assert_eq!(
            classify("a.flac", Some("flac"), &[]).category,
            Category::Audio
        );
        assert_eq!(
            classify("a.7z", Some("7z"), &[]).category,
            Category::Archives
        );
        assert_eq!(
            classify("setup.msi", Some("msi"), &[]).category,
            Category::Installers
        );
        assert_eq!(
            classify("main.rs", Some("rs"), &[]).category,
            Category::Code
        );
        assert_eq!(
            classify("Cargo.lock", Some("lock"), &[]).category,
            Category::Development
        );
        assert_eq!(
            classify("logo.psd", Some("psd"), &[]).category,
            Category::Design
        );
        assert_eq!(
            classify("db.bak", Some("bak"), &[]).category,
            Category::Backups
        );
    }

    #[test]
    fn unknown_is_other_with_low_confidence() {
        let c = classify("mystery.qqq", Some("qqq"), &[]);
        assert_eq!(c.category, Category::Other);
        assert!(c.confidence < 0.5);
        assert!(!c.reasons.is_empty());
    }

    #[test]
    fn well_known_names() {
        assert_eq!(
            classify("Dockerfile", None, &[]).category,
            Category::Development
        );
    }

    #[test]
    fn backup_names_win_over_extension() {
        let c = classify("thesis-backup.docx", Some("docx"), &[]);
        assert_eq!(c.category, Category::Backups);
    }

    #[test]
    fn user_rules_take_precedence() {
        let rules = vec![CategoryRule {
            extension: "pdf".into(),
            category: Category::Design,
        }];
        let c = classify("a.pdf", Some("pdf"), &rules);
        assert_eq!(c.category, Category::Design);
        assert_eq!(c.confidence, 1.0);
    }

    #[test]
    fn category_round_trip() {
        for c in Category::ALL {
            assert_eq!(Category::parse(c.as_str()), Some(c));
        }
    }

    #[test]
    fn sniff_detects_mismatched_extension() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("photo.pdf");
        // PNG magic bytes in a file pretending to be a PDF.
        std::fs::write(
            &fake,
            [
                0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, b'I', b'H', b'D',
                b'R',
            ],
        )
        .unwrap();
        let sig = sniff(&fake).unwrap().unwrap();
        assert_eq!(sig.mime, "image/png");
        let refined = refine(classify("photo.pdf", Some("pdf"), &[]), Some(&sig), false);
        assert_eq!(refined.category, Category::Images);
        assert!(refined.reasons[0].contains("does not match"));
    }

    #[test]
    fn sniff_confirms_matching_extension() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("real.pdf");
        std::fs::write(&pdf, b"%PDF-1.7\n%....").unwrap();
        let sig = sniff(&pdf).unwrap().unwrap();
        let refined = refine(classify("real.pdf", Some("pdf"), &[]), Some(&sig), false);
        assert_eq!(refined.category, Category::Documents);
        assert!(refined.confidence >= 0.95);
    }
}
