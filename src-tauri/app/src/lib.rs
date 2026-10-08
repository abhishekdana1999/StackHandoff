//! StackHandoff - Main application crate

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_os::init())
        .setup(|app| {
            workspace_clone_commands::init(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // App commands
            workspace_clone_commands::app::get_app_version,
            workspace_clone_commands::app::get_platform,
            workspace_clone_commands::app::get_device_key_exists,
            workspace_clone_commands::app::generate_device_key,
            workspace_clone_commands::app::get_device_fingerprint,
            workspace_clone_commands::app::get_device_identity,
            // Device commands
            workspace_clone_commands::device::list_paired_devices,
            workspace_clone_commands::device::get_paired_device,
            workspace_clone_commands::device::add_paired_device,
            workspace_clone_commands::device::update_paired_device,
            workspace_clone_commands::device::revoke_paired_device,
            workspace_clone_commands::device::delete_paired_device,
            // Project discovery
            workspace_clone_commands::projects::list_project_roots,
            workspace_clone_commands::projects::set_project_roots,
            // Application discovery
            workspace_clone_commands::app_discovery::discover_applications,
            // Workspace commands
            workspace_clone_commands::workspace::list_workspaces,
            workspace_clone_commands::workspace::get_workspace,
            workspace_clone_commands::workspace::get_workspace_snapshots,
            workspace_clone_commands::workspace::get_workspace_restore_runs,
            workspace_clone_commands::workspace::delete_workspace,
            // Capture commands
            workspace_clone_commands::capture::capture_workspace,
            workspace_clone_commands::capture::get_manifest,
            workspace_clone_commands::capture::validate_manifest,
            workspace_clone_commands::capture::scrub_manifest_secrets,
            // Transfer commands
            workspace_clone_commands::transfer::start_discovery,
            workspace_clone_commands::transfer::get_discovered_devices,
            workspace_clone_commands::transfer::probe_device,
            workspace_clone_commands::transfer::create_pairing_invitation,
            workspace_clone_commands::transfer::get_safety_number,
            workspace_clone_commands::transfer::verify_pairing,
            workspace_clone_commands::transfer::send_workspace,
            // Receive commands. The accept loop these back is started in
            // `commands::init`; these are how the window finds out what it took.
            workspace_clone_commands::receive::get_incoming_transfers,
            workspace_clone_commands::receive::dismiss_incoming_transfer,
            workspace_clone_commands::receive::get_transfer_history,
            // Preflight commands
            workspace_clone_commands::preflight::run_preflight,
            workspace_clone_commands::preflight::rerun_preflight_check,
            // Restore commands
            workspace_clone_commands::restore::generate_restore_plan,
            workspace_clone_commands::restore::summarize_restore_plan,
            workspace_clone_commands::restore::execute_restore,
            // Settings commands
            workspace_clone_commands::settings::get_setting,
            workspace_clone_commands::settings::set_setting,
            workspace_clone_commands::settings::delete_setting,
            workspace_clone_commands::settings::list_settings,
            workspace_clone_commands::settings::export_settings,
            workspace_clone_commands::settings::import_settings,
        ])
        // No path argument: `generate_context!` with none resolves
        // `tauri.conf.json` from `CARGO_MANIFEST_DIR`, which is where the config
        // lives. Naming the path explicitly used to be necessary when the config
        // sat a level above this crate, and that indirection is what let the
        // build script and the Tauri CLI disagree about the app's layout -- see
        // the comment in `build.rs`.
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
