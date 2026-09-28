//! Workspace Clone - Tauri application library

use tauri::Manager;
use workspace_clone_commands::init;

// Tauri commands
mod commands {
    tauri::generate_context!();
}

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
            init(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // App commands
            workspace_clone_commands::app::get_app_version,
            workspace_clone_commands::app::get_platform,
            workspace_clone_commands::app::get_device_key_exists,
            workspace_clone_commands::app::generate_device_key,
            workspace_clone_commands::app::get_device_fingerprint,
            // Device commands
            workspace_clone_commands::device::list_paired_devices,
            workspace_clone_commands::device::get_paired_device,
            workspace_clone_commands::device::add_paired_device,
            workspace_clone_commands::device::update_paired_device,
            workspace_clone_commands::device::revoke_paired_device,
            workspace_clone_commands::device::delete_paired_device,
            // Workspace commands
            workspace_clone_commands::workspace::list_workspaces,
            workspace_clone_commands::workspace::get_workspace,
            workspace_clone_commands::workspace::get_workspace_snapshots,
            workspace_clone_commands::workspace::get_workspace_restore_runs,
            workspace_clone_commands::workspace::delete_workspace,
            // Capture commands
            workspace_clone_commands::capture::capture_workspace,
            workspace_clone_commands::capture::validate_manifest,
            workspace_clone_commands::capture::scrub_manifest_secrets,
            // Transfer commands
            workspace_clone_commands::transfer::start_discovery,
            workspace_clone_commands::transfer::create_pairing_invitation,
            workspace_clone_commands::transfer::verify_pairing,
            workspace_clone_commands::transfer::send_workspace,
            // Preflight commands
            workspace_clone_commands::preflight::run_preflight,
            workspace_clone_commands::preflight::rerun_preflight_check,
            // Restore commands
            workspace_clone_commands::restore::generate_restore_plan,
            workspace_clone_commands::restore::execute_restore,
            // Settings commands
            workspace_clone_commands::settings::get_setting,
            workspace_clone_commands::settings::set_setting,
            workspace_clone_commands::settings::delete_setting,
            workspace_clone_commands::settings::list_settings,
            workspace_clone_commands::settings::export_settings,
            workspace_clone_commands::settings::import_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}