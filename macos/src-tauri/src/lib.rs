mod adapters;
mod commands;
mod domain;
mod macos;
mod skill;
mod storage;

use commands::AppState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| std::io::Error::other("无法确定用户目录"))?
                .join("Library/Application Support/Skill Installer");
            for folder in ["cache", "backups", "logs"] {
                std::fs::create_dir_all(data_dir.join(folder))?;
            }
            app.manage(AppState {
                data_dir,
                plans: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_clients,
            commands::inspect_skill,
            commands::plan_install,
            commands::apply_install_plan,
            commands::list_installations,
            commands::list_backups,
            commands::check_updates,
            commands::uninstall_installation,
            commands::restore_backup,
            commands::export_diagnostics,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Skill Installer");
}
