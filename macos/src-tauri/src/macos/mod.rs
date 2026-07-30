use crate::adapters::adapters;
use crate::domain::{ClientEdition, DetectedClient, DetectionStatus};
use plist::Value;
use semver::Version;
use serde_json::Value as JsonValue;
use std::env;
use std::path::{Path, PathBuf};

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn find_cli(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|part| part.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn app_candidates(name: &str, home: &Path) -> [PathBuf; 2] {
    [
        PathBuf::from("/Applications").join(format!("{name}.app")),
        home.join("Applications").join(format!("{name}.app")),
    ]
}

fn first_app(names: &[&str], home: &Path) -> Option<PathBuf> {
    names
        .iter()
        .flat_map(|name| app_candidates(name, home))
        .find(|path| path.is_dir())
}

fn app_metadata(path: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let plist_path = path.join("Contents/Info.plist");
    let plist = Value::from_file(plist_path).ok();
    let bundle_id = plist
        .as_ref()
        .and_then(|value| value.as_dictionary())
        .and_then(|dict| dict.get("CFBundleIdentifier"))
        .and_then(Value::as_string)
        .map(str::to_owned);
    let version = plist
        .as_ref()
        .and_then(|value| value.as_dictionary())
        .and_then(|dict| {
            dict.get("CFBundleShortVersionString")
                .or_else(|| dict.get("CFBundleVersion"))
        })
        .and_then(Value::as_string)
        .map(str::to_owned);
    let data_folder = std::fs::read_to_string(path.join("Contents/Resources/app/product.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<JsonValue>(&raw).ok())
        .and_then(|json| {
            json.get("dataFolderName")
                .and_then(JsonValue::as_str)
                .map(str::to_owned)
        });
    (bundle_id, version, data_folder)
}

fn version_supported(raw: Option<&str>, minimum: &str) -> bool {
    match raw.and_then(|value| Version::parse(value).ok()) {
        Some(version) => version >= Version::parse(minimum).expect("valid minimum version"),
        None => true,
    }
}

pub fn scan_clients() -> Vec<DetectedClient> {
    let home = home_dir();
    adapters()
        .into_iter()
        .map(|adapter| {
            let (app_names, cli_name, config_relative): (&[&str], &str, &str) = match adapter.id {
                "codex" => (&["Codex"], "codex", ".codex"),
                "claude-code" => (&["Claude"], "claude", ".claude"),
                "kiro" => (&["Kiro"], "kiro", ".kiro"),
                "cursor" => (&["Cursor"], "cursor", ".cursor"),
                "windsurf" => (&["Windsurf"], "windsurf", ".codeium/windsurf"),
                "trae-international" => (&["Trae", "TRAE"], "trae", ".trae"),
                "trae-china" => (&["Trae CN", "TRAE CN", "Trae"], "trae-cn", ".trae-cn"),
                _ => (&[], "", ""),
            };
            let mut application = first_app(app_names, &home);
            let cli = if cli_name.is_empty() {
                None
            } else {
                find_cli(cli_name)
            };
            let config = home.join(config_relative);
            let mut version = None;
            let mut notes = Vec::new();

            if let Some(path) = application.as_ref() {
                let (bundle_id, app_version, data_folder) = app_metadata(path);
                version = app_version;
                if adapter.edition == ClientEdition::TraeInternational {
                    let is_international = bundle_id.as_deref() == Some("com.trae.app")
                        && data_folder.as_deref() != Some(".trae-cn");
                    if !is_international {
                        application = None;
                    }
                } else if adapter.edition == ClientEdition::TraeChina {
                    let is_china = data_folder.as_deref() == Some(".trae-cn")
                        || bundle_id.as_deref().is_some_and(|id| id.contains("trae"))
                            && config.is_dir()
                            && data_folder.as_deref() != Some(".trae");
                    if !is_china {
                        application = None;
                    }
                }
            }

            let minimum = match adapter.edition {
                ClientEdition::TraeInternational => Some("3.5.25"),
                ClientEdition::TraeChina => Some("3.3.25"),
                ClientEdition::Standard => None,
            };
            let supports_version = minimum
                .map(|minimum| version_supported(version.as_deref(), minimum))
                .unwrap_or(true);
            if let Some(minimum) = minimum {
                notes.push(format!("原生 Skills 最低版本 {minimum}"));
            }
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
            let usable = matches!(
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
                global_skills_path: home.join(adapter.global_relative).display().to_string(),
                project_skills_path: adapter.project_relative.to_string(),
                supports_skills: usable && supports_version,
                notes,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trae_minimum_versions_are_enforced() {
        assert!(!version_supported(Some("3.5.24"), "3.5.25"));
        assert!(version_supported(Some("3.5.25"), "3.5.25"));
        assert!(!version_supported(Some("3.3.24"), "3.3.25"));
    }

    #[test]
    fn local_trae_fixture_is_recognized_when_present() {
        if Path::new("/Applications/Trae.app").is_dir() {
            let clients = scan_clients();
            let international = clients
                .iter()
                .find(|client| client.id == "trae-international")
                .expect("international adapter exists");
            assert_eq!(international.status, DetectionStatus::Installed);
            assert!(international.supports_skills);
        }
    }
}
