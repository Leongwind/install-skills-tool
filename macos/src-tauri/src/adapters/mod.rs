use crate::domain::{ClientEdition, DetectedClient};
use semver::Version;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Adapter {
    pub id: &'static str,
    pub name: &'static str,
    pub edition: ClientEdition,
    pub install_relative: &'static str,
    pub inventory_relatives: &'static [&'static str],
}

pub fn adapters() -> Vec<Adapter> {
    vec![
        Adapter {
            id: "codex",
            name: "Codex",
            edition: ClientEdition::Standard,
            install_relative: ".agents/skills",
            inventory_relatives: &[".agents/skills", ".codex/skills"],
        },
        Adapter {
            id: "claude-code",
            name: "Claude Code",
            edition: ClientEdition::Standard,
            install_relative: ".claude/skills",
            inventory_relatives: &[".claude/skills"],
        },
        Adapter {
            id: "kiro",
            name: "Kiro",
            edition: ClientEdition::Standard,
            install_relative: ".kiro/skills",
            inventory_relatives: &[".kiro/skills"],
        },
        Adapter {
            id: "cursor",
            name: "Cursor",
            edition: ClientEdition::Standard,
            install_relative: ".cursor/skills",
            inventory_relatives: &[".cursor/skills"],
        },
        Adapter {
            id: "windsurf",
            name: "Windsurf",
            edition: ClientEdition::Standard,
            install_relative: ".codeium/windsurf/skills",
            inventory_relatives: &[".codeium/windsurf/skills"],
        },
        Adapter {
            id: "trae-international",
            name: "TRAE International",
            edition: ClientEdition::TraeInternational,
            install_relative: ".trae/skills",
            inventory_relatives: &[".trae/skills"],
        },
        Adapter {
            id: "trae-china",
            name: "TRAE China",
            edition: ClientEdition::TraeChina,
            install_relative: ".trae-cn/skills",
            inventory_relatives: &[".trae-cn/skills"],
        },
    ]
}

pub fn resolve_global_target(adapter: &Adapter, home: &Path, skill_name: &str) -> PathBuf {
    home.join(adapter.install_relative).join(skill_name)
}

pub fn resolve_inventory_roots(adapter: &Adapter, home: &Path) -> Vec<PathBuf> {
    adapter
        .inventory_relatives
        .iter()
        .map(|relative| home.join(relative))
        .collect()
}

pub fn detected_map(clients: &[DetectedClient]) -> HashMap<String, DetectedClient> {
    clients
        .iter()
        .cloned()
        .map(|client| (client.id.clone(), client))
        .collect()
}

pub fn passive_consumers_for(
    path: &Path,
    active_consumers: &[String],
    clients: &[DetectedClient],
) -> Vec<String> {
    if !path.to_string_lossy().contains(".agents/skills") {
        return Vec::new();
    }
    let candidates = ["cursor", "windsurf", "trae-international", "trae-china"];
    clients
        .iter()
        .filter(|client| {
            candidates.contains(&client.id.as_str())
                && client.supports_skills
                && !active_consumers.contains(&client.id)
                && passive_discovery_supported(client)
        })
        .map(|client| client.id.clone())
        .collect()
}

fn passive_discovery_supported(client: &DetectedClient) -> bool {
    let minimum = match client.edition {
        ClientEdition::TraeInternational => Some("3.5.44"),
        ClientEdition::TraeChina => Some("3.3.44"),
        ClientEdition::Standard => None,
    };
    minimum.is_none_or(|minimum| {
        client
            .version
            .as_deref()
            .and_then(|raw| Version::parse(raw).ok())
            .is_some_and(|version| version >= Version::parse(minimum).expect("valid version"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trae_editions_use_distinct_global_paths() {
        let list = adapters();
        let international = list
            .iter()
            .find(|adapter| adapter.id == "trae-international")
            .unwrap();
        let china = list
            .iter()
            .find(|adapter| adapter.id == "trae-china")
            .unwrap();
        let home = Path::new("/Users/test");
        let int_global = resolve_global_target(international, home, "demo");
        let cn_global = resolve_global_target(china, home, "demo");
        assert_ne!(int_global, cn_global);
    }

    #[test]
    fn codex_separates_install_root_from_inventory_roots() {
        let codex = adapters()
            .into_iter()
            .find(|adapter| adapter.id == "codex")
            .unwrap();
        let home = Path::new("/Users/test");

        assert_eq!(
            resolve_global_target(&codex, home, "demo"),
            home.join(".agents/skills/demo")
        );
        assert_eq!(
            resolve_inventory_roots(&codex, home),
            vec![home.join(".agents/skills"), home.join(".codex/skills"),]
        );
    }

    #[test]
    fn trae_passive_discovery_obeys_version_gate() {
        let client = |version: &str| DetectedClient {
            id: "trae-international".to_string(),
            name: "TRAE International".to_string(),
            edition: ClientEdition::TraeInternational,
            version: Some(version.to_string()),
            status: crate::domain::DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: String::new(),
            inventory_skills_paths: Vec::new(),
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };
        assert!(!passive_discovery_supported(&client("3.5.43")));
        assert!(passive_discovery_supported(&client("3.5.44")));
    }
}
