# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Tauri 2 desktop application with a Rust backend and React/TypeScript frontend.
- Cross-platform filesystem abstraction (hidden files, cloud placeholders, no-replace rename, volume identity,
  name validation, default exclusions, disk capacity) for Windows, macOS and Linux.
- SQLite index with WAL mode, versioned migrations, FTS5 trigram search and integrity checks.
- Folder authorization through native dialogs opened by the backend.
- Streaming, cancellable, read-only scanner with live progress, incremental updates, hash-cache preservation,
  exclusions, symlink safety and incomplete-folder tracking.
- Rule-based categorization (13 categories) with confidence, explanations, content-signature verification and
  per-file overrides plus user extension rules.
- Exact duplicate detection (size → partial → full BLAKE3) with a bounded worker pool.
- Storage analyzer, large/recent files, categories view and search with a query language and optional history.
- Smart Organization recommendation engine with transparent signals and confidence.
- Safe operations engine: preview, dry run, journaled execution, verified copies, opt-in cross-drive moves,
  undo and crash recovery. Trash only; no permanent deletion.
- Animated opening screen ("Created and managed by muffle"), honouring reduced-motion and the chosen theme.
- Release packaging: Windows MSI and per-user NSIS installer, macOS DMG (Apple Silicon and Intel, ad-hoc signed), Linux AppImage, deb and rpm.
- Activity history, settings, themes (system/dark/light), CI for Windows/macOS/Linux and release bundling.
