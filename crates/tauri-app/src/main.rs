// Phaselith Tauri Control Panel
//
// Provides system tray + GUI for controlling the APO DLL.
// Communicates with APO via memory-mapped file IPC.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ipc_bridge;
mod commands;
#[cfg(windows)]
mod endpoint_bind;
#[cfg(windows)]
mod com_bind;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_ipc_state,
            commands::get_host_platform,
            commands::set_config,
            commands::install_apo,
            commands::uninstall_apo,
            commands::get_config,
            commands::is_apo_installed,
            commands::license_activate,
            commands::license_validate,
            commands::license_deactivate,
            commands::license_get_cached,
        ])
        .setup(|_app| {
            // Initialize mmap IPC bridge
            ipc_bridge::init();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
