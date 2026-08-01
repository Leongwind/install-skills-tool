use std::path::{Path, PathBuf};

pub mod adapters;
pub mod domain;
pub mod windows;

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
        .setup(|_app| {
            let root = data_dir()?;
            for folder in ["cache", "backups", "logs"] {
                std::fs::create_dir_all(root.join(folder))?;
            }
            Ok(())
        })
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
