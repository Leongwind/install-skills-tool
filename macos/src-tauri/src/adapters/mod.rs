use crate::domain::{ClientEdition, DetectedClient, InstallScope};
use semver::Version;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Adapter {
    pub id: &'static str,
    pub name: &'static str,
    pub edition: ClientEdition,
    pub global_relative: &'static str,
    pub project_relative: &'static str,
}

pub fn adapters() -> Vec<Adapter> {
    vec![
        Adapter {
            id: "codex",
            name: "Codex",
            edition: ClientEdition::Standard,
            global_relative: ".agents/skills",
            project_relative: ".agents/skills",
        },
        Adapter {
            id: "claude-code",
            name: "Claude Code",
            edition: ClientEdition::Standard,
            global_relative: ".claude/skills",
            project_relative: ".claude/skills",
        },
        Adapter {
            id: "kiro",
            name: "Kiro",
            edition: ClientEdition::Standard,
            global_relative: ".kiro/skills",
            project_relative: ".kiro/skills",
        },
        Adapter {
            id: "cursor",
            name: "Cursor",
            edition: ClientEdition::Standard,
            global_relative: ".cursor/skills",
            project_relative: ".cursor/skills",
        },
        Adapter {
            id: "windsurf",
            name: "Windsurf",
            edition: ClientEdition::Standard,
            global_relative: ".codeium/windsurf/skills",
            project_relative: ".windsurf/skills",
        },
        Adapter {
            id: "trae-international",
            name: "TRAE International",
            edition: ClientEdition::TraeInternational,
            global_relative: ".trae/skills",
            project_relative: ".trae/skills",
        },
        Adapter {
            id: "trae-china",
            name: "TRAE China",
            edition: ClientEdition::TraeChina,
            global_relative: ".trae-cn/skills",
            project_relative: ".trae/skills",
        },
    ]
}

pub fn resolve_target(
    adapter: &Adapter,
    home: &Path,
    project: Option<&Path>,
    scope: InstallScope,
    skill_name: &str,
) -> Result<PathBuf, String> {
    let base = match scope {
        InstallScope::Global => home.join(adapter.global_relative),
        InstallScope::Project => project
            .ok_or_else(|| "项目安装需要选择项目目录".to_string())?
            .join(adapter.project_relative),
    };
    Ok(base.join(skill_name))
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
    fn trae_editions_share_project_path_but_not_global_path() {
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
        let project = Path::new("/work/project");

        let int_project = resolve_target(
            international,
            home,
            Some(project),
            InstallScope::Project,
            "demo",
        )
        .unwrap();
        let cn_project =
            resolve_target(china, home, Some(project), InstallScope::Project, "demo").unwrap();
        assert_eq!(int_project, cn_project);

        let int_global =
            resolve_target(international, home, None, InstallScope::Global, "demo").unwrap();
        let cn_global = resolve_target(china, home, None, InstallScope::Global, "demo").unwrap();
        assert_ne!(int_global, cn_global);
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
            project_skills_path: String::new(),
            supports_skills: true,
            notes: Vec::new(),
        };
        assert!(!passive_discovery_supported(&client("3.5.43")));
        assert!(passive_discovery_supported(&client("3.5.44")));
    }
}
