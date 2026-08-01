use crate::adapters::{adapters, version_supports_skills, Adapter};
use crate::domain::{ClientEdition, DetectedClient, DetectionStatus};
use serde_json::Value;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct RegisteredApplication {
    pub display_name: String,
    pub display_version: Option<String>,
    pub install_location: Option<PathBuf>,
    pub display_icon: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ScanContext {
    pub user_profile: PathBuf,
    pub appdata: PathBuf,
    pub local_appdata: PathBuf,
    pub program_files: Vec<PathBuf>,
    pub path_dirs: Vec<PathBuf>,
    pub registered_apps: Vec<RegisteredApplication>,
}

fn existing_cli(adapter: &Adapter, path_dirs: &[PathBuf]) -> Option<PathBuf> {
    path_dirs
        .iter()
        .flat_map(|folder| adapter.cli_names.iter().map(move |name| folder.join(name)))
        .find(|candidate| candidate.is_file())
}

fn executable_in(root: &Path, adapter: &Adapter) -> Option<PathBuf> {
    adapter
        .executable_names
        .iter()
        .map(|name| root.join(name))
        .find(|candidate| candidate.is_file())
}

fn application_candidates(context: &ScanContext, adapter: &Adapter) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for folder in adapter.install_folder_names {
        roots.push(context.local_appdata.join("Programs").join(folder));
        roots.push(context.local_appdata.join(folder));
        for program_files in &context.program_files {
            roots.push(program_files.join(folder));
        }
    }
    roots
}

fn product_metadata(application: &Path) -> (Option<String>, Option<String>) {
    let Some(root) = application.parent() else {
        return (None, None);
    };
    let candidates = [
        root.join("resources/app/product.json"),
        root.join("Resources/app/product.json"),
        root.join("product.json"),
    ];
    let parsed = candidates.iter().find_map(|path| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
    });
    let version = parsed
        .as_ref()
        .and_then(|json| json.get("version"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let data_folder = parsed
        .as_ref()
        .and_then(|json| json.get("dataFolderName"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    (version, data_folder)
}

fn registry_match<'a>(
    context: &'a ScanContext,
    adapter: &Adapter,
) -> Option<&'a RegisteredApplication> {
    context.registered_apps.iter().find(|app| {
        let name = app.display_name.to_lowercase();
        match adapter.edition {
            ClientEdition::TraeInternational => {
                name.contains("trae") && !name.contains("cn") && !name.contains("中国")
            }
            ClientEdition::TraeChina => {
                name.contains("trae") && (name.contains("cn") || name.contains("中国"))
            }
            ClientEdition::Standard => name.contains(&adapter.name.to_lowercase()),
        }
    })
}

fn registered_executable(app: &RegisteredApplication, adapter: &Adapter) -> Option<PathBuf> {
    app.display_icon
        .as_ref()
        .filter(|path| path.is_file())
        .cloned()
        .or_else(|| {
            app.install_location
                .as_deref()
                .and_then(|root| executable_in(root, adapter))
        })
}

fn scan_client(context: &ScanContext, adapter: Adapter) -> DetectedClient {
    let registered = registry_match(context, &adapter);
    let mut application = registered.and_then(|app| registered_executable(app, &adapter));
    if application.is_none() {
        application = application_candidates(context, &adapter)
            .iter()
            .find_map(|root| executable_in(root, &adapter));
    }
    let cli = existing_cli(&adapter, &context.path_dirs);
    let config = context.user_profile.join(adapter.config_relative);
    let (product_version, data_folder) = application
        .as_deref()
        .map(product_metadata)
        .unwrap_or_default();
    let version = registered
        .and_then(|app| app.display_version.clone())
        .or(product_version);

    if adapter.edition == ClientEdition::TraeInternational
        && data_folder.as_deref() == Some(".trae-cn")
    {
        application = None;
    }
    if adapter.edition == ClientEdition::TraeChina && data_folder.as_deref() == Some(".trae") {
        application = None;
    }

    let supports_version = version_supports_skills(&adapter, version.as_deref());
    let status = if application.is_some() && !supports_version {
        DetectionStatus::UnsupportedVersion
    } else if application.is_some() {
        DetectionStatus::Installed
    } else if cli.is_some() {
        DetectionStatus::CliOnly
    } else if config.is_dir() {
        DetectionStatus::ConfigOnly
    } else {
        DetectionStatus::NotInstalled
    };
    let mut notes = Vec::new();
    if let Some(minimum) = adapter.minimum_skills_version {
        notes.push(format!("原生 Skills 最低版本 {minimum}"));
    }
    let supports_skills = supports_version
        && matches!(
            status,
            DetectionStatus::Installed | DetectionStatus::CliOnly
        );
    DetectedClient {
        id: adapter.id.to_string(),
        name: adapter.name.to_string(),
        edition: adapter.edition,
        version,
        status,
        application_path: application.map(|path| path.display().to_string()),
        cli_path: cli.map(|path| path.display().to_string()),
        global_skills_path: context
            .user_profile
            .join(adapter.global_relative)
            .display()
            .to_string(),
        supports_skills,
        notes,
    }
}

pub fn scan_clients_with_context(context: &ScanContext) -> Vec<DetectedClient> {
    adapters()
        .into_iter()
        .map(|adapter| scan_client(context, adapter))
        .collect()
}

#[cfg(windows)]
fn read_registered_apps() -> Vec<RegisteredApplication> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut applications = Vec::new();
    for hive in [
        RegKey::predef(HKEY_CURRENT_USER),
        RegKey::predef(HKEY_LOCAL_MACHINE),
    ] {
        for key_path in [
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let Ok(root) = hive.open_subkey(key_path) else {
                continue;
            };
            for name in root.enum_keys().filter_map(Result::ok) {
                let Ok(key) = root.open_subkey(name) else {
                    continue;
                };
                let Ok(display_name) = key.get_value::<String, _>("DisplayName") else {
                    continue;
                };
                let path_value = |name: &str| {
                    key.get_value::<String, _>(name)
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                        .map(|value| PathBuf::from(value.trim_matches('"')))
                };
                applications.push(RegisteredApplication {
                    display_name,
                    display_version: key.get_value("DisplayVersion").ok(),
                    install_location: path_value("InstallLocation"),
                    display_icon: path_value("DisplayIcon"),
                });
            }
        }
    }
    applications
}

#[cfg(not(windows))]
fn read_registered_apps() -> Vec<RegisteredApplication> {
    Vec::new()
}

pub fn scan_context_from_environment() -> Result<ScanContext, String> {
    let required = |name: &str| {
        env::var_os(name)
            .map(PathBuf::from)
            .ok_or_else(|| format!("环境变量 {name} 不可用"))
    };
    let program_files = ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(|name| env::var_os(name).map(PathBuf::from))
        .collect();
    let path_dirs = env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default();
    Ok(ScanContext {
        user_profile: required("USERPROFILE")?,
        appdata: required("APPDATA")?,
        local_appdata: required("LOCALAPPDATA")?,
        program_files,
        path_dirs,
        registered_apps: read_registered_apps(),
    })
}

pub fn scan_clients() -> Result<Vec<DetectedClient>, String> {
    scan_context_from_environment().map(|context| scan_clients_with_context(&context))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(root: &Path) -> ScanContext {
        ScanContext {
            user_profile: root.join("Users/tester"),
            appdata: root.join("Users/tester/AppData/Roaming"),
            local_appdata: root.join("Users/tester/AppData/Local"),
            program_files: vec![root.join("Program Files")],
            path_dirs: vec![root.join("bin")],
            registered_apps: Vec::new(),
        }
    }

    #[test]
    fn synthetic_windows_install_and_cli_are_classified_separately() {
        let root = tempfile::tempdir().unwrap();
        let fixture = context(root.path());
        let kiro = fixture.local_appdata.join("Programs/Kiro/Kiro.exe");
        std::fs::create_dir_all(kiro.parent().unwrap()).unwrap();
        std::fs::write(&kiro, b"fixture").unwrap();
        let codex = fixture.path_dirs[0].join("codex.cmd");
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        std::fs::write(&codex, b"fixture").unwrap();

        let clients = scan_clients_with_context(&fixture);
        assert_eq!(
            clients
                .iter()
                .find(|item| item.id == "kiro")
                .unwrap()
                .status,
            DetectionStatus::Installed
        );
        assert_eq!(
            clients
                .iter()
                .find(|item| item.id == "codex")
                .unwrap()
                .status,
            DetectionStatus::CliOnly
        );
    }

    #[test]
    fn synthetic_trae_product_metadata_distinguishes_editions() {
        let root = tempfile::tempdir().unwrap();
        let fixture = context(root.path());
        let app = fixture.local_appdata.join("Programs/Trae/Trae.exe");
        std::fs::create_dir_all(app.parent().unwrap().join("resources/app")).unwrap();
        std::fs::write(&app, b"fixture").unwrap();
        std::fs::write(
            app.parent().unwrap().join("resources/app/product.json"),
            r#"{"version":"3.5.78","dataFolderName":".trae"}"#,
        )
        .unwrap();

        let clients = scan_clients_with_context(&fixture);
        let international = clients
            .iter()
            .find(|item| item.id == "trae-international")
            .unwrap();
        let china = clients.iter().find(|item| item.id == "trae-china").unwrap();
        assert_eq!(international.status, DetectionStatus::Installed);
        assert_eq!(international.version.as_deref(), Some("3.5.78"));
        assert_eq!(china.status, DetectionStatus::NotInstalled);
    }

    #[test]
    fn synthetic_registry_entry_supplies_version_and_location() {
        let root = tempfile::tempdir().unwrap();
        let mut fixture = context(root.path());
        let install = root.path().join("registered/Cursor");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("Cursor.exe"), b"fixture").unwrap();
        fixture.registered_apps.push(RegisteredApplication {
            display_name: "Cursor".to_string(),
            display_version: Some("1.7.0".to_string()),
            install_location: Some(install),
            display_icon: None,
        });

        let cursor = scan_clients_with_context(&fixture)
            .into_iter()
            .find(|item| item.id == "cursor")
            .unwrap();
        assert_eq!(cursor.status, DetectionStatus::Installed);
        assert_eq!(cursor.version.as_deref(), Some("1.7.0"));
    }
}
