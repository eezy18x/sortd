//! SORTD backend.
//!
//! The crate is split into a Tauri-independent core (everything except
//! `commands` and `run`) and a thin IPC layer. The core can be tested
//! without a window or webview.

pub mod activity;
pub mod categorization;
pub mod commands;
pub mod database;
pub mod duplicates;
pub mod error;
pub mod filesystem;
pub mod indexing;
pub mod operations;
pub mod organization;
pub mod scanner;
pub mod search;
pub mod security;
pub mod settings;
pub mod state;
pub mod storage;
pub mod util;

use std::sync::Arc;

use tauri::Manager;

use crate::database::Database;
use crate::state::{AppState, Jobs};

const DATABASE_FILE: &str = "sortd.db";

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            // Development builds only: SORTD_DATA_DIR points the app at a
            // separate index (used for demos and screenshots, so a developer's
            // real index is never touched). Release builds ignore it.
            #[cfg(debug_assertions)]
            let dir = std::env::var_os("SORTD_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(dir);
            let db = Arc::new(Database::open(&dir.join(DATABASE_FILE))?);
            let scan_on_startup = db.with(|c| {
                // Reconcile anything left over from an unexpected exit.
                scanner::mark_interrupted_scans(c)?;
                operations::execute::recover(c)?;
                Ok(settings::load(c)?.scan_on_startup)
            })?;
            let jobs = Arc::new(Jobs::default());
            app.manage(AppState {
                db: db.clone(),
                jobs: jobs.clone(),
            });
            if scan_on_startup {
                let _ = commands::spawn_scan(app.handle().clone(), db, jobs, None);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_overview,
            commands::get_job_status,
            commands::list_locations,
            commands::add_location,
            commands::remove_location,
            commands::start_scan,
            commands::cancel_scan,
            commands::list_scans,
            commands::list_directory,
            commands::get_file_details,
            commands::set_category_override,
            commands::query_files,
            commands::category_stats,
            commands::search_files,
            commands::search_suggestions,
            commands::search_history,
            commands::record_search,
            commands::clear_search_history,
            commands::storage_analysis,
            commands::start_duplicate_scan,
            commands::cancel_duplicate_scan,
            commands::list_duplicate_groups,
            commands::generate_recommendations,
            commands::list_recommendations,
            commands::set_recommendation_status,
            commands::plan_operations,
            commands::dry_run_operations,
            commands::execute_operations,
            commands::cancel_operations,
            commands::undo_batch,
            commands::discard_plan,
            commands::list_batches,
            commands::get_batch,
            commands::list_activity,
            commands::get_settings,
            commands::update_settings,
            commands::pick_organization_root,
            commands::clear_organization_root,
            commands::open_file,
            commands::reveal_file,
            commands::reveal_directory,
            commands::check_database,
            commands::optimize_database,
            commands::reset_index,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SORTD");
}
