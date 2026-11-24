# Architecture

SORTD is a Tauri 2 application: a Rust backend that owns all filesystem and database access, and a React
frontend rendered in the system webview.

```
src/                         React + TypeScript frontend
  app/                       App shell, router, providers
  components/                Design system (ui/), file table, operation preview, details sheet
  features/<area>/           One folder per screen (dashboard, explorer, organization, …)
  hooks/                     Jobs/progress, theme, location actions
  services/api.ts            The ONLY module that talks to the backend (typed IPC wrappers)
  lib/                       Formatting, validation, category metadata
  types/                     Types mirroring backend payloads
src-tauri/
  migrations/                Versioned SQL schema
  capabilities/default.json  Webview permission allowlist (one entry per command)
  src/
    filesystem/              Cross-platform abstraction (mod.rs + windows.rs / unix.rs)
    database/                Connections, pragmas, migrations, integrity
    security/                Authorized locations, path validation/authorization
    scanner/                 Streaming, cancellable, read-only traversal
    indexing/                Read-side queries (files, directories, overview)
    categorization/          Rule-based classification + content signatures
    duplicates/              Size → partial hash → BLAKE3 pipeline
    storage/                 Storage analytics
    search/                  FTS5 search, query language, history
    organization/            Recommendation engine
    operations/              Plan / dry-run / execute / undo / recover
    activity/                Activity log
    settings/                Validated persistent settings
    commands/                Thin Tauri IPC layer (+ list.rs: the command allowlist)
    state.rs                 Shared state and background job slots
```

## Core principles

1. **The core does not depend on Tauri.** Everything except `commands` and `lib.rs::run` is plain Rust and is
   tested without a window.
2. **Platform differences live in one place.** `filesystem/` exposes a single interface; `cfg(target_os)` appears
   nowhere else.
3. **Read paths and write paths are separate.** Scanning, analysis and recommendations cannot modify files; only
   `operations/` can, and only for a persisted, user-approved plan.
4. **Never trust the webview.** Commands validate input, and every path is authorized against user-picked locations.

## Data model

SQLite in WAL mode (`PRAGMA user_version` tracks the schema). Main tables:

| Table | Purpose |
|---|---|
| `locations` | Folders authorized via the native picker (`scan` or `destination`) |
| `scans` | One row per scan run, with counts and sampled issues |
| `files` | One row per indexed file: metadata, category, hash cache, scan stamps |
| `directories` | Recursive size/count aggregates and a `complete` flag |
| `files_fts` | FTS5 trigram index over name/path, maintained by triggers |
| `operation_batches` / `operation_items` | The operation journal |
| `recommendations` | Organization suggestions with signals and confidence |
| `activity`, `settings`, `search_history` | Log, preferences, optional history |

## Scanning

- `walkdir` streaming traversal with `follow_links(false)` and `same_file_system(!cross_filesystems)`.
- Exclusions: platform defaults (system folders) plus user paths/names; a root the user picked is never excluded.
- Files are upserted in 1,000-row transactions. Each scan stamps rows (`last_seen_scan`, `first_seen_scan`,
  `changed_scan`); a completed scan deletes rows it didn't see. A cancelled scan keeps existing rows.
- Hash caches survive rescans unless size or mtime changed.
- Directory totals are aggregated in memory (O(directories), not O(files)) and propagated bottom-up. Any read
  error marks the directory and its ancestors incomplete.
- Progress is throttled (≈150 ms) and streamed as Tauri events; the UI coalesces index-change events.

## Duplicate detection

1. Candidates: files sharing an exact size (symlinks, empty files and cloud placeholders excluded).
2. Partial BLAKE3 of the first and last 16 KiB (the whole file when ≤ 32 KiB — then it *is* the full hash).
3. Full streaming BLAKE3 for (size, partial) collisions, on a bounded worker pool; the writer stays on one thread.
4. Every file is re-checked against indexed size/mtime before and after hashing.

## Operations engine

`plan → (dry run) → execute → (undo)`, with `recover` on startup. See the module docs in
`src-tauri/src/operations/mod.rs` and the safety table in the README.

## Frontend

- TanStack Query caches all backend data; the `sortd://index-changed` event triggers a debounced invalidation.
- Long lists use TanStack Virtual plus infinite pagination.
- Every file-changing action goes through `FileActionsProvider` → `OperationPreview`, so there is exactly one
  code path from a click to a filesystem change.
- Theme: CSS variables with dark/light token sets; "system" follows `prefers-color-scheme`.

## Extensibility

New capabilities slot in as core modules plus a command and a feature folder. Planned extension points: a
filesystem watcher feeding the same upsert path as the scanner; pluggable classifiers behind
`categorization::classify` (for local AI); perceptual hashing as a separate table so it never mixes with exact
duplicates; connectors that produce `locations` of new kinds.
