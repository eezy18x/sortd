fn main() {
    // Every IPC command is declared here so Tauri generates an explicit
    // `allow-<command>` permission for each. Capabilities in
    // `capabilities/` then grant only what the main window needs.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(sortd_commands::COMMANDS)),
    )
    .expect("failed to run tauri-build");
}

mod sortd_commands {
    include!("src/commands/list.rs");
}
