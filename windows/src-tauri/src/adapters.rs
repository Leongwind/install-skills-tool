use crate::domain::{ClientEdition, DetectedClient};
use semver::Version;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Adapter {
    pub id: &'static str,
    pub name: &'static str,
    pub edition: ClientEdition,
    pub global_relative: &'static str,
    pub config_relative: &'static str,
    pub executable_names: &'static [&'static str],
    pub cli_names: &'static [&'static str],
    pub install_folder_names: &'static [&'static str],
    pub minimum_skills_version: Option<&'static str>,
}

pub fn adapters() -> Vec<Adapter> {
    vec![
        Adapter {
            id: "codex",
            name: "Codex",
            edition: ClientEdition::Standard,
            global_relative: ".agents/skills",
            config_relative: ".codex",
            executable_names: &["Codex.exe"],
            cli_names: &["codex.exe", "codex.cmd"],
            install_folder_names: &["Codex"],
            minimum_skills_version: None,
        },
        Adapter {
            id: "claude-code",
            name: "Claude Code",
            edition: ClientEdition::Standard,
            global_relative: ".claude/skills",
            config_relative: ".claude",
            executable_names: &["Claude.exe"],
            cli_names: &["claude.exe", "claude.cmd"],
            install_folder_names: &["Claude"],
            minimum_skills_version: None,
        },
        Adapter {
            id: "kiro",
            name: "Kiro",
            edition: ClientEdition::Standard,
            global_relative: ".kiro/skills",
            config_relative: ".kiro",
            executable_names: &["Kiro.exe"],
            cli_names: &["kiro.exe", "kiro.cmd"],
            install_folder_names: &["Kiro"],
            minimum_skills_version: None,
        },
        Adapter {
            id: "cursor",
            name: "Cursor",
            edition: ClientEdition::Standard,
            global_relative: ".cursor/skills",
            config_relative: ".cursor",
            executable_names: &["Cursor.exe"],
            cli_names: &["cursor.exe", "cursor.cmd"],
            install_folder_names: &["Cursor"],
            minimum_skills_version: None,
        },
        Adapter {
            id: "windsurf",
            name: "Windsurf",
            edition: ClientEdition::Standard,
            global_relative: ".codeium/windsurf/skills",
            config_relative: ".codeium/windsurf",
            executable_names: &["Windsurf.exe"],
            cli_names: &["windsurf.exe", "windsurf.cmd"],
            install_folder_names: &["Windsurf"],
            minimum_skills_version: None,
        },
        Adapter {
            id: "trae-international",
            name: "TRAE International",
            edition: ClientEdition::TraeInternational,
            global_relative: ".trae/skills",
            config_relative: ".trae",
            executable_names: &["Trae.exe", "TRAE.exe"],
            cli_names: &["trae.exe", "trae.cmd"],
            install_folder_names: &["Trae", "TRAE"],
            minimum_skills_version: Some("3.5.25"),
        },
        Adapter {
            id: "trae-china",
            name: "TRAE China",
            edition: ClientEdition::TraeChina,
            global_relative: ".trae-cn/skills",
            config_relative: ".trae-cn",
            executable_names: &["Trae.exe", "TRAE.exe"],
            cli_names: &["trae-cn.exe", "trae-cn.cmd"],
            install_folder_names: &["Trae CN", "TRAE CN", "Trae"],
            minimum_skills_version: Some("3.3.25"),
        },
    ]
}

pub fn resolve_global_target(adapter: &Adapter, user_profile: &Path, skill: &str) -> PathBuf {
    user_profile.join(adapter.global_relative).join(skill)
}

pub fn detected_map(clients: &[DetectedClient]) -> HashMap<String, DetectedClient> {
    clients
        .iter()
        .cloned()
        .map(|client| (client.id.clone(), client))
        .collect()
}

pub fn inventory_roots(adapter: &Adapter, user_profile: &Path) -> Vec<PathBuf> {
    let primary = user_profile.join(adapter.global_relative);
    if adapter.id == "codex" {
        vec![primary, user_profile.join(".codex/skills")]
    } else {
        vec![primary]
    }
}

pub fn version_supports_skills(adapter: &Adapter, raw: Option<&str>) -> bool {
    let Some(minimum) = adapter.minimum_skills_version else {
        return true;
    };
    raw.and_then(|value| Version::parse(value).ok())
        .is_none_or(|version| version >= Version::parse(minimum).expect("valid version"))
}

pub fn passive_consumers_for(
    path: &Path,
    active_consumers: &[String],
    clients: &[DetectedClient],
) -> Vec<String> {
    let normalized = path.to_string_lossy().replace('\\', "/").to_lowercase();
    if !normalized.contains("/.agents/skills/") {
        return Vec::new();
    }
    let candidates = ["cursor", "windsurf", "trae-international", "trae-china"];
    clients
        .iter()
        .filter(|client| {
            candidates.contains(&client.id.as_str())
                && client.supports_skills
                && !active_consumers.contains(&client.id)
        })
        .map(|client| client.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_adapters_resolve_expected_global_roots() {
        let profile = Path::new(r"C:\Users\tester");
        let list = adapters();
        let codex = list.iter().find(|item| item.id == "codex").unwrap();
        let china = list.iter().find(|item| item.id == "trae-china").unwrap();
        assert_eq!(
            resolve_global_target(codex, profile, "demo"),
            profile.join(".agents/skills/demo")
        );
        assert_eq!(
            resolve_global_target(china, profile, "demo"),
            profile.join(".trae-cn/skills/demo")
        );
    }

    #[test]
    fn codex_inventory_includes_current_and_legacy_roots() {
        let profile = Path::new(r"C:\Users\tester");
        let codex = adapters()
            .into_iter()
            .find(|item| item.id == "codex")
            .unwrap();
        assert_eq!(
            inventory_roots(&codex, profile),
            vec![
                profile.join(".agents/skills"),
                profile.join(".codex/skills")
            ]
        );
    }

    #[test]
    fn trae_editions_enforce_distinct_minimum_versions() {
        let list = adapters();
        let international = list
            .iter()
            .find(|item| item.id == "trae-international")
            .unwrap();
        let china = list.iter().find(|item| item.id == "trae-china").unwrap();
        assert!(!version_supports_skills(international, Some("3.5.24")));
        assert!(version_supports_skills(international, Some("3.5.25")));
        assert!(!version_supports_skills(china, Some("3.3.24")));
        assert!(version_supports_skills(china, Some("3.3.25")));
    }
}
