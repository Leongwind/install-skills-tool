mod adapters;
mod commands;
mod domain;
mod inventory;
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
                inspections: Mutex::new(HashMap::new()),
                plans: Mutex::new(HashMap::new()),
                update_plans: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_environment,
            commands::inspect_source,
            commands::plan_install,
            commands::apply_install_plan,
            commands::list_installations,
            commands::list_backups,
            commands::get_app_overview,
            commands::scan_client_inventory,
            commands::recover_operation,
            commands::rollback_operation,
            commands::export_skill_bundle,
            commands::export_lockfile,
            commands::plan_lockfile_import,
            commands::adopt_external_skill,
            commands::check_updates,
            commands::plan_updates,
            commands::apply_update_plan,
            commands::set_installation_pinned,
            commands::uninstall_installation,
            commands::restore_backup,
            commands::set_backup_policy,
            commands::delete_backup,
            commands::export_diagnostics,
            commands::reveal_in_finder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Skill Installer");
}
