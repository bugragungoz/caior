mod commands;
mod context;
mod errors;

use commands::{installer, registry};
use installer::InstallState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .manage(InstallState::default())
        .invoke_handler(tauri::generate_handler![
            registry::check_registry_batch,
            installer::start_installation,
            installer::cancel_installation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
