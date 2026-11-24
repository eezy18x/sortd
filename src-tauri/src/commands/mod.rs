//! IPC command layer.
//!
//! This is a thin adapter: it validates input, runs blocking work off the
//! async runtime, and delegates to the core modules. Commands never accept
//! arbitrary paths to *authorize* — folder authorization happens only via
//! native dialogs opened here, in the backend. Paths that arrive from the
//! frontend are re-validated against authorized locations by the core.

pub mod list;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::activity::{self, kind};
use crate::categorization::{self, Category, Classification, ContentSignature};
use crate::database::{self, Database};
use crate::duplicates::{self, DuplicateGroup, DuplicateOptions};
use crate::error::{AppError, AppResult};
use crate::filesystem;
use crate::indexing::{
    self, CategoryStat, DirectoryListing, FileFilter, FileRecord, Overview, Page, ScanRecord,
    SortField, SortOrder,
};
use crate::operations::{self, execute, Batch, BatchSummary, PlanRequest};
use crate::organization::{self, GenerationSummary, Recommendation};
use crate::scanner::{self, ScanOptions, ScanProgress};
use crate::search::{self, SearchRequest, SearchResponse, Suggestion};
use crate::security::{self, Location, LocationKind};
use crate::settings::{self, Settings};
use crate::state::{
    AppState, Job, JobStatus, Jobs, EVENT_DUPLICATES, EVENT_INDEX_CHANGED, EVENT_OPERATION,
    EVENT_SCAN,
};
use crate::storage::{self, StorageAnalysis};

type CmdResult<T> = Result<T, AppError>;

/// Runs blocking database work on the blocking thread pool.
async fn with_db<T, F>(state: &State<'_, AppState>, f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Connection) -> AppResult<T> + Send + 'static,
{
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || db.with(f))
        .await
        .map_err(|e| AppError::Internal(format!("background task failed: {e}")))?
}

fn notify_index_changed(app: &AppHandle) {
    let _ = app.emit(EVENT_INDEX_CHANGED, ());
}

// ---------------------------------------------------------------------------
// App / overview
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub arch: String,
    pub database_path: String,
    pub database_size: u64,
    pub schema_version: u32,
}

#[tauri::command]
pub async fn app_info(app: AppHandle, state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let path = state.db.path().to_path_buf();
    let size = ["", "-wal"]
        .iter()
        .filter_map(|s| std::fs::metadata(format!("{}{s}", path.display())).ok())
        .map(|m| m.len())
        .sum();
    Ok(AppInfo {
        name: app.package_info().name.clone(),
        version: app.package_info().version.to_string(),
        platform: filesystem::platform_name().into(),
        arch: std::env::consts::ARCH.into(),
        database_path: path.display().to_string(),
        database_size: size,
        schema_version: database::SCHEMA_VERSION,
    })
}

#[tauri::command]
pub async fn get_overview(state: State<'_, AppState>) -> CmdResult<Overview> {
    with_db(&state, |c| indexing::overview(c)).await
}

#[tauri::command]
pub async fn get_job_status(state: State<'_, AppState>) -> CmdResult<JobStatus> {
    state.jobs.status()
}

// ---------------------------------------------------------------------------
// Locations and scanning
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_locations(state: State<'_, AppState>) -> CmdResult<Vec<Location>> {
    with_db(&state, |c| security::list_locations(c)).await
}

async fn pick_folder(app: &AppHandle, title: &str) -> CmdResult<Option<PathBuf>> {
    let app = app.clone();
    let title = title.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title(&title);
        if let Some(home) = filesystem::home_dir() {
            dialog = dialog.set_directory(home);
        }
        if let Some(w) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&w);
        }
        dialog
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))
}

/// Opens a native folder picker. The chosen folder becomes an authorized
/// location. Returns `None` when the user cancels.
#[tauri::command]
pub async fn add_location(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
) -> CmdResult<Option<Location>> {
    let kind = match kind.as_str() {
        "scan" => LocationKind::Scan,
        "destination" => LocationKind::Destination,
        _ => return Err(AppError::invalid("kind must be scan or destination")),
    };
    let title = if kind == LocationKind::Scan {
        "Choose a folder for SORTD to index"
    } else {
        "Choose a destination folder"
    };
    let Some(picked) = pick_folder(&app, title).await? else {
        return Ok(None);
    };
    let loc = with_db(&state, move |c| {
        let loc = security::add_location(c, &picked, kind)?;
        activity::record(
            c,
            kind::LOCATION_ADDED,
            &format!("Added location {}", loc.path),
            Some(serde_json::json!({ "path": loc.path, "kind": kind.as_str() })),
            None,
        )?;
        Ok(loc)
    })
    .await?;
    notify_index_changed(&app);
    Ok(Some(loc))
}

#[tauri::command]
pub async fn remove_location(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let jobs = state.jobs.clone();
    // Removing index data while a scan writes to it is not allowed.
    let flag = jobs.claim(Job::Scan)?;
    let result = with_db(&state, move |c| {
        let loc = security::remove_location(c, id)?;
        let mut s = settings::load(c)?;
        if s.organization_root
            .as_deref()
            .map(|r| filesystem::path_starts_with(Path::new(r), Path::new(&loc.path)))
            .unwrap_or(false)
        {
            s.organization_root = None;
            settings::save(c, &s)?;
        }
        activity::record(
            c,
            kind::LOCATION_REMOVED,
            &format!(
                "Removed location {} from the index (no files were changed)",
                loc.path
            ),
            Some(serde_json::json!({ "path": loc.path })),
            None,
        )?;
        Ok(())
    })
    .await;
    drop(flag);
    jobs.release(Job::Scan);
    notify_index_changed(&app);
    result
}

fn emit_scan(app: &AppHandle, jobs: &Jobs, p: &ScanProgress) {
    if let Ok(mut s) = jobs.scan.lock() {
        s.last = Some(p.clone());
    }
    let _ = app.emit(EVENT_SCAN, p);
}

/// Starts scanning the given locations (all scan locations when omitted) on
/// a background thread. Progress is streamed as `sortd://scan-progress`.
pub fn spawn_scan(
    app: AppHandle,
    db: Arc<Database>,
    jobs: Arc<Jobs>,
    ids: Option<Vec<i64>>,
) -> AppResult<()> {
    let cancel = jobs.claim(Job::Scan)?;
    let spawned = std::thread::Builder::new().name("sortd-scan".into()).spawn(move || {
        let run = || -> AppResult<(bool, Settings)> {
            let mut conn = db.connect()?;
            let settings = settings::load(&conn)?;
            let opts = ScanOptions::from_settings(&settings);
            let locations: Vec<Location> = security::list_locations(&conn)?
                .into_iter()
                .filter(|l| l.kind == LocationKind::Scan)
                .filter(|l| ids.as_ref().map(|ids| ids.contains(&l.id)).unwrap_or(true))
                .collect();
            let mut any_completed = false;
            for loc in locations {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                activity::record(&conn, kind::SCAN_STARTED, &format!("Scan started: {}", loc.path), Some(serde_json::json!({ "path": loc.path })), None)?;
                let result = scanner::run_scan(&mut conn, &loc, &opts, &cancel, &|p| emit_scan(&app, &jobs, p));
                match result {
                    Ok(s) => {
                        let (k, verb) = match s.status.as_str() {
                            "completed" => (kind::SCAN_COMPLETED, "Scan completed"),
                            "cancelled" => (kind::SCAN_CANCELLED, "Scan cancelled"),
                            _ => (kind::SCAN_FAILED, "Scan failed"),
                        };
                        any_completed |= s.status == "completed";
                        activity::record(
                            &conn,
                            k,
                            &format!("{verb}: {} — {} files indexed, {} added, {} updated, {} removed", loc.path, s.files_seen, s.files_added, s.files_updated, s.files_removed),
                            Some(serde_json::json!({
                                "path": loc.path, "scanId": s.scan_id, "filesSeen": s.files_seen, "bytesSeen": s.bytes_seen,
                                "added": s.files_added, "updated": s.files_updated, "removed": s.files_removed,
                                "errors": s.errors, "skipped": s.skipped, "elapsedMs": s.elapsed_ms
                            })),
                            None,
                        )?;
                    }
                    Err(e) => {
                        activity::record(&conn, kind::SCAN_FAILED, &format!("Scan failed: {} — {e}", loc.path), Some(serde_json::json!({ "path": loc.path, "error": e.to_string() })), None)?;
                        emit_scan(&app, &jobs, &ScanProgress {
                            location_id: loc.id,
                            root: loc.path.clone(),
                            phase: "done".into(),
                            status: Some("failed".into()),
                            current_path: Some(e.to_string()),
                            ..Default::default()
                        });
                    }
                }
                notify_index_changed(&app);
            }
            let cancelled = cancel.load(std::sync::atomic::Ordering::Relaxed);
            Ok((any_completed && !cancelled, settings))
        };
        let outcome = run();
        jobs.release(Job::Scan);
        notify_index_changed(&app);
        if let Ok((true, settings)) = outcome {
            if settings.detect_duplicates_after_scan {
                let _ = spawn_duplicates(app.clone(), db.clone(), jobs.clone());
            }
        }
    });
    if let Err(e) = spawned {
        return Err(AppError::Internal(format!(
            "could not start scan thread: {e}"
        )));
    }
    Ok(())
}

#[tauri::command]
pub async fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    location_ids: Option<Vec<i64>>,
) -> CmdResult<()> {
    spawn_scan(app, state.db.clone(), state.jobs.clone(), location_ids)
}

#[tauri::command]
pub async fn cancel_scan(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.jobs.cancel(Job::Scan))
}

#[tauri::command]
pub async fn list_scans(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> CmdResult<Vec<ScanRecord>> {
    with_db(&state, move |c| {
        indexing::list_scans(c, limit.unwrap_or(50))
    })
    .await
}

// ---------------------------------------------------------------------------
// Explorer, files, categories
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_directory(
    state: State<'_, AppState>,
    path: String,
    sort: Option<SortField>,
    order: Option<SortOrder>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> CmdResult<DirectoryListing> {
    let path = security::parse_untrusted_path(&path)?
        .to_string_lossy()
        .into_owned();
    with_db(&state, move |c| {
        indexing::list_directory(
            c,
            &path,
            sort.unwrap_or(SortField::Size),
            order.unwrap_or_default(),
            limit.unwrap_or(500),
            offset.unwrap_or(0),
        )
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDetails {
    pub file: FileRecord,
    pub classification: Classification,
    pub signature: Option<ContentSignature>,
    pub exists: bool,
    pub unsafe_to_open: bool,
    pub duplicates: Vec<FileRecord>,
}

#[tauri::command]
pub async fn get_file_details(state: State<'_, AppState>, id: i64) -> CmdResult<FileDetails> {
    with_db(&state, move |c| {
        let file = indexing::get_file(c, id)?;
        let settings = settings::load(c)?;
        let path = PathBuf::from(&file.path);
        let meta = std::fs::symlink_metadata(&path).ok();
        let exists = meta.is_some();
        let info = meta.as_ref().map(|m| filesystem::entry_info(&path, m)).unwrap_or_default();
        let user_rule = file.extension.as_deref().map(|e| settings.category_rules.iter().any(|r| r.extension == e)).unwrap_or(false);
        let mut classification = categorization::classify(&file.name, file.extension.as_deref(), &settings.category_rules);
        let mut signature = None;
        // Reading the first bytes is skipped for cloud placeholders (it would
        // trigger a download) and for symlinks (never followed).
        if settings.verify_content_type && exists && meta.as_ref().map(|m| m.is_file()).unwrap_or(false) && !info.is_placeholder && !file.is_placeholder {
            signature = security::authorize_path(c, &path).ok().and_then(|_| categorization::sniff(&path).ok().flatten());
            classification = categorization::refine(classification, signature.as_ref(), user_rule);
        }
        if file.category_overridden {
            if let Some(cat) = Category::parse(&file.category) {
                classification.category = cat;
                classification.confidence = 1.0;
                classification.reasons.insert(0, format!("You assigned this file to {}", cat.label()));
            }
        }
        let duplicates = match &file.full_hash {
            Some(h) => {
                let mut stmt = c.prepare(&format!(
                    "SELECT {} FROM files f WHERE f.full_hash = ?1 AND f.size = ?2 AND f.id != ?3 AND f.is_symlink = 0 LIMIT 100",
                    indexing::FILE_COLUMNS
                ))?;
                let rows = stmt.query_map(rusqlite::params![h, file.size, file.id], indexing::file_from_row)?.collect::<Result<Vec<_>, _>>()?;
                rows
            }
            None => Vec::new(),
        };
        let unsafe_to_open = filesystem::is_unsafe_to_open(file.extension.as_deref(), &info) || file.is_symlink;
        Ok(FileDetails { file, classification, signature, exists, unsafe_to_open, duplicates })
    })
    .await
}

#[tauri::command]
pub async fn set_category_override(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    category: Option<String>,
) -> CmdResult<FileRecord> {
    let cat = match category.as_deref() {
        None => None,
        Some(s) => Some(Category::parse(s).ok_or_else(|| AppError::invalid("unknown category"))?),
    };
    let f = with_db(&state, move |c| {
        let f = indexing::set_category_override(c, id, cat)?;
        activity::record(
            c,
            kind::CATEGORY_OVERRIDDEN,
            &match cat {
                Some(cat) => format!("Set {} to {}", f.name, cat.label()),
                None => format!("Restored automatic category for {}", f.name),
            },
            Some(serde_json::json!({ "path": f.path, "category": cat.map(|c| c.as_str()) })),
            None,
        )?;
        Ok(f)
    })
    .await?;
    notify_index_changed(&app);
    Ok(f)
}

#[tauri::command]
pub async fn query_files(
    state: State<'_, AppState>,
    filter: FileFilter,
    sort: Option<SortField>,
    order: Option<SortOrder>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> CmdResult<Page<FileRecord>> {
    with_db(&state, move |c| {
        indexing::query_files(
            c,
            &filter,
            sort.unwrap_or_default(),
            order.unwrap_or_default(),
            limit.unwrap_or(200),
            offset.unwrap_or(0),
        )
    })
    .await
}

#[tauri::command]
pub async fn category_stats(
    state: State<'_, AppState>,
    location_id: Option<i64>,
) -> CmdResult<Vec<CategoryStat>> {
    with_db(&state, move |c| indexing::category_stats(c, location_id)).await
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn search_files(
    state: State<'_, AppState>,
    request: SearchRequest,
) -> CmdResult<SearchResponse> {
    with_db(&state, move |c| search::search(c, &request)).await
}

#[tauri::command]
pub async fn search_suggestions(
    state: State<'_, AppState>,
    prefix: String,
) -> CmdResult<Vec<Suggestion>> {
    with_db(&state, move |c| {
        let history = settings::load(c)?.record_search_history;
        search::suggestions(c, &prefix, history)
    })
    .await
}

#[tauri::command]
pub async fn search_history(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    with_db(&state, |c| {
        if !settings::load(c)?.record_search_history {
            return Ok(Vec::new());
        }
        search::list_history(c)
    })
    .await
}

#[tauri::command]
pub async fn record_search(state: State<'_, AppState>, query: String) -> CmdResult<()> {
    with_db(&state, move |c| {
        if settings::load(c)?.record_search_history {
            search::record_history(c, &query)?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn clear_search_history(state: State<'_, AppState>) -> CmdResult<()> {
    with_db(&state, |c| search::clear_history(c)).await
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn storage_analysis(
    state: State<'_, AppState>,
    location_id: Option<i64>,
) -> CmdResult<StorageAnalysis> {
    with_db(&state, move |c| storage::analyze(c, location_id)).await
}

// ---------------------------------------------------------------------------
// Duplicates
// ---------------------------------------------------------------------------

pub fn spawn_duplicates(app: AppHandle, db: Arc<Database>, jobs: Arc<Jobs>) -> AppResult<()> {
    let cancel = jobs.claim(Job::Duplicates)?;
    let thread_jobs = jobs.clone();
    let spawned = std::thread::Builder::new().name("sortd-duplicates".into()).spawn(move || {
        let jobs = thread_jobs;
        let run = || -> AppResult<()> {
            let mut conn = db.connect()?;
            let s = settings::load(&conn)?;
            let opts = DuplicateOptions { min_size: s.duplicate_min_size, workers: s.hash_concurrency as usize };
            let p = duplicates::find_duplicates(&mut conn, &opts, &cancel, &|p| {
                if let Ok(mut slot) = jobs.duplicates.lock() {
                    slot.last = Some(p.clone());
                }
                let _ = app.emit(EVENT_DUPLICATES, p);
            })?;
            activity::record(
                &conn,
                kind::DUPLICATES_COMPLETED,
                &format!(
                    "Duplicate detection {}: {} groups, {} reclaimable",
                    p.status.as_deref().unwrap_or("finished"),
                    p.groups,
                    crate::util::format_bytes(p.reclaimable.max(0) as u64)
                ),
                Some(serde_json::json!({ "groups": p.groups, "reclaimable": p.reclaimable, "hashed": p.hashed, "errors": p.errors, "changed": p.changed })),
                None,
            )?;
            Ok(())
        };
        if let Err(e) = run() {
            let _ = app.emit(EVENT_DUPLICATES, duplicates::DuplicateProgress { phase: "done".into(), status: Some(format!("failed: {e}")), ..Default::default() });
        }
        jobs.release(Job::Duplicates);
        notify_index_changed(&app);
    });
    if let Err(e) = spawned {
        jobs.release(Job::Duplicates);
        return Err(AppError::Internal(format!(
            "could not start duplicate detection: {e}"
        )));
    }
    Ok(())
}

#[tauri::command]
pub async fn start_duplicate_scan(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    spawn_duplicates(app, state.db.clone(), state.jobs.clone())
}

#[tauri::command]
pub async fn cancel_duplicate_scan(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.jobs.cancel(Job::Duplicates))
}

#[tauri::command]
pub async fn list_duplicate_groups(
    state: State<'_, AppState>,
    sort: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> CmdResult<Page<DuplicateGroup>> {
    with_db(&state, move |c| {
        duplicates::list_groups(
            c,
            sort.as_deref().unwrap_or("waste"),
            limit.unwrap_or(50),
            offset.unwrap_or(0),
        )
    })
    .await
}

// ---------------------------------------------------------------------------
// Organization
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn generate_recommendations(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<GenerationSummary> {
    let r = with_db(&state, |c| {
        let root = settings::load(c)?.organization_root;
        organization::generate(c, root.as_deref())
    })
    .await?;
    notify_index_changed(&app);
    Ok(r)
}

#[tauri::command]
pub async fn list_recommendations(
    state: State<'_, AppState>,
    status: Option<String>,
    min_confidence: Option<f64>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> CmdResult<Page<Recommendation>> {
    with_db(&state, move |c| {
        organization::list(
            c,
            status.as_deref().unwrap_or("pending"),
            min_confidence.unwrap_or(0.0).clamp(0.0, 1.0),
            limit.unwrap_or(200),
            offset.unwrap_or(0),
        )
    })
    .await
}

#[tauri::command]
pub async fn set_recommendation_status(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    status: String,
) -> CmdResult<usize> {
    let n = with_db(&state, move |c| organization::set_status(c, &ids, &status)).await?;
    notify_index_changed(&app);
    Ok(n)
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn plan_operations(state: State<'_, AppState>, request: PlanRequest) -> CmdResult<Batch> {
    with_db(&state, move |c| operations::plan(c, &request)).await
}

#[tauri::command]
pub async fn dry_run_operations(
    state: State<'_, AppState>,
    batch_id: i64,
) -> CmdResult<Vec<execute::DryRunItem>> {
    with_db(&state, move |c| execute::dry_run(c, batch_id)).await
}

#[tauri::command]
pub async fn execute_operations(
    app: AppHandle,
    state: State<'_, AppState>,
    batch_id: i64,
) -> CmdResult<Batch> {
    let jobs = state.jobs.clone();
    let cancel = jobs.claim(Job::Operation)?;
    let app2 = app.clone();
    let jobs2 = jobs.clone();
    let result = with_db(&state, move |c| {
        let b = execute::execute(c, batch_id, &cancel, &|p| {
            if let Ok(mut slot) = jobs2.operation.lock() {
                slot.last = Some(p.clone());
            }
            let _ = app2.emit(EVENT_OPERATION, p);
        })?;
        if b.summary.origin == "organization" {
            organization::mark_applied_for_batch(c, batch_id)?;
        }
        Ok(b)
    })
    .await;
    jobs.release(Job::Operation);
    notify_index_changed(&app);
    result
}

#[tauri::command]
pub async fn cancel_operations(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.jobs.cancel(Job::Operation))
}

#[tauri::command]
pub async fn undo_batch(
    app: AppHandle,
    state: State<'_, AppState>,
    batch_id: i64,
) -> CmdResult<Batch> {
    let jobs = state.jobs.clone();
    let _flag = jobs.claim(Job::Operation)?;
    let app2 = app.clone();
    let result = with_db(&state, move |c| {
        let b = execute::undo(c, batch_id, &|p| {
            let _ = app2.emit(EVENT_OPERATION, p);
        })?;
        c.execute("UPDATE recommendations SET status = 'pending' WHERE status = 'applied' AND file_id IN (SELECT f.id FROM files f JOIN operation_items i ON i.source = f.path WHERE i.batch_id = ?1 AND i.status = 'undone')", [batch_id])?;
        Ok(b)
    })
    .await;
    jobs.release(Job::Operation);
    notify_index_changed(&app);
    result
}

#[tauri::command]
pub async fn discard_plan(state: State<'_, AppState>, batch_id: i64) -> CmdResult<()> {
    with_db(&state, move |c| operations::discard(c, batch_id)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchList {
    pub items: Vec<BatchSummary>,
    pub total: i64,
}

#[tauri::command]
pub async fn list_batches(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> CmdResult<BatchList> {
    with_db(&state, move |c| {
        let (items, total) = operations::list_batches(c, limit.unwrap_or(50), offset.unwrap_or(0))?;
        Ok(BatchList { items, total })
    })
    .await
}

#[tauri::command]
pub async fn get_batch(state: State<'_, AppState>, batch_id: i64) -> CmdResult<Batch> {
    with_db(&state, move |c| operations::get_batch(c, batch_id)).await
}

// ---------------------------------------------------------------------------
// Activity
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPage {
    pub items: Vec<activity::ActivityEntry>,
    pub total: i64,
}

#[tauri::command]
pub async fn list_activity(
    state: State<'_, AppState>,
    kinds: Option<Vec<String>>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> CmdResult<ActivityPage> {
    let kinds = kinds.unwrap_or_default();
    if kinds.len() > 50 {
        return Err(AppError::invalid("too many filters"));
    }
    with_db(&state, move |c| {
        let (items, total) = activity::list(c, &kinds, limit.unwrap_or(100), offset.unwrap_or(0))?;
        Ok(ActivityPage { items, total })
    })
    .await
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    with_db(&state, |c| settings::load(c)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub settings: Settings,
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    update: SettingsUpdate,
) -> CmdResult<Settings> {
    with_db(&state, move |c| {
        let current = settings::load(c)?;
        let mut next = update.settings.normalized();
        // The organization root can only be changed through the folder picker.
        next.organization_root = current.organization_root.clone();
        settings::save(c, &next)?;
        if !next.record_search_history {
            search::clear_history(c)?;
        }
        activity::record(c, kind::SETTINGS_CHANGED, "Settings updated", None, None)?;
        Ok(next)
    })
    .await
}

#[tauri::command]
pub async fn pick_organization_root(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<Settings> {
    let Some(picked) =
        pick_folder(&app, "Choose where SORTD should suggest organizing files").await?
    else {
        return with_db(&state, |c| settings::load(c)).await;
    };
    with_db(&state, move |c| {
        security::add_location(c, &picked, LocationKind::Destination)?;
        let canonical = filesystem::canonicalize(&picked)?;
        let mut s = settings::load(c)?;
        s.organization_root = canonical.to_str().map(str::to_string);
        settings::save(c, &s)?;
        activity::record(
            c,
            kind::SETTINGS_CHANGED,
            &format!("Organization folder set to {}", canonical.display()),
            None,
            None,
        )?;
        Ok(s)
    })
    .await
}

#[tauri::command]
pub async fn clear_organization_root(state: State<'_, AppState>) -> CmdResult<Settings> {
    with_db(&state, |c| {
        let mut s = settings::load(c)?;
        s.organization_root = None;
        settings::save(c, &s)?;
        Ok(s)
    })
    .await
}

// ---------------------------------------------------------------------------
// Opening and revealing
// ---------------------------------------------------------------------------

async fn indexed_path(state: &State<'_, AppState>, id: i64) -> CmdResult<(PathBuf, bool)> {
    with_db(state, move |c| {
        let f = indexing::get_file(c, id)?;
        let path = PathBuf::from(&f.path);
        security::authorize_path(c, &path)?;
        let meta = std::fs::symlink_metadata(&path).map_err(|_| {
            AppError::NotFound(format!(
                "{} no longer exists; rescan to update the index",
                f.path
            ))
        })?;
        let info = filesystem::entry_info(&path, &meta);
        let unsafe_open =
            f.is_symlink || filesystem::is_unsafe_to_open(f.extension.as_deref(), &info);
        Ok((path, unsafe_open))
    })
    .await
}

/// Opens an indexed file with its default application. Executables,
/// scripts, installers and links are refused: SORTD never runs files.
#[tauri::command]
pub async fn open_file(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let (path, unsafe_open) = indexed_path(&state, id).await?;
    if unsafe_open {
        return Err(AppError::PermissionDenied(
            "SORTD does not open programs, scripts or links. Use “Show in folder” instead.".into(),
        ));
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::Internal(format!("could not open the file: {e}")))
}

#[tauri::command]
pub async fn reveal_file(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let (path, _) = indexed_path(&state, id).await?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppError::Internal(format!("could not show the file: {e}")))
}

#[tauri::command]
pub async fn reveal_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<()> {
    let p = security::parse_untrusted_path(&path)?;
    let p2 = p.clone();
    with_db(&state, move |c| {
        security::authorize_path(c, &p2).map(|_| ())
    })
    .await?;
    if !p.is_dir() {
        return Err(AppError::NotFound("the folder no longer exists".into()));
    }
    app.opener()
        .reveal_item_in_dir(&p)
        .map_err(|e| AppError::Internal(format!("could not show the folder: {e}")))
}

// ---------------------------------------------------------------------------
// Database maintenance
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub ok: bool,
    pub problems: Vec<String>,
    pub guidance: Option<String>,
}

#[tauri::command]
pub async fn check_database(state: State<'_, AppState>) -> CmdResult<IntegrityReport> {
    with_db(&state, |c| {
        let problems = database::integrity_check(c)?;
        let ok = problems.is_empty();
        Ok(IntegrityReport {
            ok,
            guidance: (!ok).then(|| {
                "The index is damaged. Your files are not affected — the index only describes them. \
                 Use “Reset index” and rescan your locations to rebuild it."
                    .to_string()
            }),
            problems,
        })
    })
    .await
}

#[tauri::command]
pub async fn optimize_database(state: State<'_, AppState>) -> CmdResult<()> {
    let jobs = state.jobs.clone();
    let _f = jobs.claim(Job::Scan)?;
    let r = with_db(&state, |c| {
        database::rebuild_search_index(c)?;
        c.execute_batch("INSERT INTO files_fts(files_fts) VALUES ('optimize'); ANALYZE; VACUUM;")?;
        Ok(())
    })
    .await;
    jobs.release(Job::Scan);
    r
}

#[tauri::command]
pub async fn reset_index(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let jobs = state.jobs.clone();
    let _f = jobs.claim(Job::Scan)?;
    let r = with_db(&state, |c| {
        let tx = c.transaction()?;
        tx.execute_batch(
            "DELETE FROM recommendations; DELETE FROM files; DELETE FROM directories; DELETE FROM scans;
             DELETE FROM search_history; UPDATE locations SET last_scan_id = NULL;",
        )?;
        activity::record(&tx, kind::INDEX_RESET, "Index reset (no files were changed)", None, None)?;
        tx.commit()?;
        database::rebuild_search_index(c)?;
        Ok(())
    })
    .await;
    jobs.release(Job::Scan);
    notify_index_changed(&app);
    r
}

#[cfg(test)]
mod tests {
    #[test]
    fn capability_grants_exactly_the_declared_commands() {
        let raw = include_str!("../../capabilities/default.json");
        let json: serde_json::Value = serde_json::from_str(raw).unwrap();
        let granted: Vec<String> = json["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str())
            .filter(|p| p.starts_with("allow-"))
            .map(|p| p.trim_start_matches("allow-").replace('-', "_"))
            .collect();
        let mut declared: Vec<String> = super::list::COMMANDS
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut g = granted.clone();
        declared.sort();
        g.sort();
        assert_eq!(declared, g);
    }
}
