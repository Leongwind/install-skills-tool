pub mod adapters;
pub mod commands;
pub mod domain;
pub mod inventory;
pub mod operations;
pub mod skill;
pub mod storage;
pub mod windows;

use commands::AppState;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Manager;

fn data_dir_from(appdata: &Path) -> PathBuf {
    appdata.join("Skill Installer")
}

fn data_dir() -> Result<PathBuf, std::io::Error> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::other("APPDATA is unavailable"))
        .map(|root| data_dir_from(&root))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = data_dir()?;
            for folder in ["cache", "backups", "logs"] {
                std::fs::create_dir_all(root.join(folder))?;
            }
            app.manage(AppState {
                data_dir: root,
                inspections: Mutex::new(HashMap::new()),
                plans: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_clients,
            commands::scan_environment,
            commands::inspect_source,
            commands::plan_install,
            commands::apply_install_plan,
            commands::adopt_external_skill,
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

#[cfg(test)]
mod tests {
    use super::data_dir_from;

    #[test]
    fn windows_data_directory_is_appdata_scoped() {
        let root = std::path::Path::new(r"C:\Users\tester\AppData\Roaming");
        assert_eq!(
            data_dir_from(root),
            std::path::PathBuf::from(r"C:\Users\tester\AppData\Roaming").join("Skill Installer")
        );
    }
}
