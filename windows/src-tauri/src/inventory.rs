use crate::domain::*;
use regex::Regex;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Deserialize)]
struct InventoryFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

fn frontmatter(path: &Path) -> Result<InventoryFrontmatter, String> {
    let raw = fs::read_to_string(path.join("SKILL.md"))
        .map_err(|error| format!("无法读取 SKILL.md: {error}"))?;
    let body = raw
        .strip_prefix("---")
        .ok_or_else(|| "SKILL.md 缺少 YAML frontmatter".to_string())?;
    let end = body
        .find("\n---")
        .ok_or_else(|| "SKILL.md frontmatter 未结束".to_string())?;
    serde_yaml::from_str(&body[..end]).map_err(|error| format!("YAML 无效: {error}"))
}

fn inventory_entry(
    client: &DetectedClient,
    path: PathBuf,
    state: &PersistedState,
    unsafe_link: bool,
) -> InventorySkill {
    let directory_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    if unsafe_link {
        return InventorySkill {
            inventory_id: Uuid::new_v4().to_string(),
            name: directory_name.clone(),
            directory_name,
            description: None,
            resolved_path: path.display().to_string(),
            content_hash: None,
            validity: SkillValidity::Unsafe,
            management_status: SkillManagementStatus::Unsafe,
            installation_id: None,
            issues: vec!["软链接或 junction 仅供查看，不能纳管或卸载".to_string()],
            consumers: vec![client.id.clone()],
            passive_from_client_id: None,
        };
    }
    let mut issues = Vec::new();
    let (name, description, mut validity) = match frontmatter(&path) {
        Ok(frontmatter) => {
            let mut valid = true;
            let name = frontmatter.name.unwrap_or_else(|| {
                valid = false;
                issues.push("frontmatter 缺少 name".to_string());
                directory_name.clone()
            });
            let pattern = Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("valid regex");
            if name.len() > 64 || !pattern.is_match(&name) {
                valid = false;
                issues.push("name 不符合 Agent Skills 规范".to_string());
            }
            if name != directory_name {
                valid = false;
                issues.push("frontmatter name 与目录名不一致".to_string());
            }
            if frontmatter
                .description
                .as_deref()
                .is_none_or(|description| description.trim().is_empty())
            {
                valid = false;
                issues.push("frontmatter 缺少 description".to_string());
            }
            (
                name,
                frontmatter.description,
                if valid {
                    SkillValidity::Valid
                } else {
                    SkillValidity::NonConforming
                },
            )
        }
        Err(issue) => {
            issues.push(issue);
            (directory_name.clone(), None, SkillValidity::NonConforming)
        }
    };
    let content_hash = match crate::storage::inspect_tree(&path) {
        Ok((hash, _, _, _)) => Some(hash),
        Err(issue) => {
            issues.push(issue);
            validity = SkillValidity::Unsafe;
            None
        }
    };
    let resolved_path = path.display().to_string();
    let tracked = state
        .installations
        .iter()
        .find(|record| record.resolved_path == resolved_path);
    let management_status = if validity == SkillValidity::Unsafe {
        SkillManagementStatus::Unsafe
    } else if let Some(record) = tracked {
        if content_hash.as_deref() != Some(record.content_hash.as_str()) {
            SkillManagementStatus::Modified
        } else if record.provenance == InstallationProvenance::Adopted {
            SkillManagementStatus::Adopted
        } else {
            SkillManagementStatus::ToolManaged
        }
    } else {
        SkillManagementStatus::External
    };
    InventorySkill {
        inventory_id: Uuid::new_v4().to_string(),
        name,
        directory_name,
        description,
        resolved_path,
        content_hash,
        validity,
        management_status,
        installation_id: tracked.map(|record| record.id.clone()),
        issues,
        consumers: tracked
            .map(|record| record.consumers.clone())
            .unwrap_or_else(|| vec![client.id.clone()]),
        passive_from_client_id: None,
    }
}

pub fn inventory_roots(client: &DetectedClient) -> Vec<PathBuf> {
    let primary = PathBuf::from(&client.global_skills_path);
    if client.id != "codex" {
        return vec![primary];
    }
    let legacy = primary
        .parent()
        .and_then(Path::parent)
        .map(|profile| profile.join(".codex/skills"));
    match legacy {
        Some(legacy) if legacy != primary => vec![primary, legacy],
        _ => vec![primary],
    }
}

pub fn scan_client_inventory(
    client: &DetectedClient,
    state: &PersistedState,
) -> ClientSkillInventory {
    let mut direct_skills = Vec::new();
    let mut errors = Vec::new();
    for root in inventory_roots(client) {
        if !root.exists() {
            continue;
        }
        let mut entries = WalkDir::new(&root)
            .min_depth(1)
            .follow_links(false)
            .into_iter();
        while let Some(entry) = entries.next() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(format!("{}: {error}", root.display()));
                    continue;
                }
            };
            let path = entry.path().to_path_buf();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            let has_manifest = path.join("SKILL.md").is_file();
            if metadata.file_type().is_symlink() {
                if has_manifest {
                    direct_skills.push(inventory_entry(client, path, state, true));
                }
                entries.skip_current_dir();
            } else if metadata.is_dir() && has_manifest {
                direct_skills.push(inventory_entry(client, path, state, false));
                entries.skip_current_dir();
            } else if metadata.is_dir()
                && matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some(".git" | "node_modules" | "target")
                )
            {
                entries.skip_current_dir();
            }
        }
    }
    direct_skills.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.resolved_path.cmp(&right.resolved_path))
    });
    ClientSkillInventory {
        client_id: client.id.clone(),
        root_path: client.global_skills_path.clone(),
        direct_skills,
        passive_skills: Vec::new(),
        scan_error: (!errors.is_empty()).then(|| errors.join("\n")),
    }
}

pub fn build_environment_scan(
    clients: Vec<DetectedClient>,
    state: &PersistedState,
) -> EnvironmentScan {
    let mut inventories = clients
        .iter()
        .map(|client| scan_client_inventory(client, state))
        .collect::<Vec<_>>();
    let codex_skills = inventories
        .iter()
        .find(|inventory| inventory.client_id == "codex")
        .map(|inventory| inventory.direct_skills.clone())
        .unwrap_or_default();
    for skill in codex_skills {
        let targets = crate::adapters::passive_consumers_for(
            Path::new(&skill.resolved_path),
            &["codex".to_string()],
            &clients,
        );
        for target in targets {
            if let Some(inventory) = inventories
                .iter_mut()
                .find(|inventory| inventory.client_id == target)
            {
                let mut passive = skill.clone();
                passive.inventory_id = Uuid::new_v4().to_string();
                passive.management_status = SkillManagementStatus::Passive;
                passive.installation_id = None;
                passive.consumers = vec![target];
                passive.passive_from_client_id = Some("codex".to_string());
                inventory.passive_skills.push(passive);
            }
        }
    }
    EnvironmentScan {
        clients,
        inventories,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(root: &Path, name: &str) {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Fixture\n---\n"),
        )
        .unwrap();
    }

    fn client(id: &str, root: &Path) -> DetectedClient {
        DetectedClient {
            id: id.to_string(),
            name: id.to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: root.display().to_string(),
            supports_skills: true,
            notes: Vec::new(),
        }
    }

    #[test]
    fn inventory_distinguishes_managed_external_and_modified() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "managed");
        skill(root.path(), "external");
        let managed = root.path().join("managed");
        let hash = crate::storage::inspect_tree(&managed).unwrap().0;
        let mut state = PersistedState::default();
        state.installations.push(PhysicalInstallation {
            id: "record".to_string(),
            skill_name: "managed".to_string(),
            resolved_path: managed.display().to_string(),
            source: None,
            source_details: SkillSourceDetails::default(),
            content_hash: hash,
            consumers: vec!["kiro".to_string()],
            passive_consumers: Vec::new(),
            adapter_version: 1,
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            provenance: InstallationProvenance::Tool,
        });
        let inventory = scan_client_inventory(&client("kiro", root.path()), &state);
        assert_eq!(inventory.direct_skills.len(), 2);
        assert_eq!(
            inventory
                .direct_skills
                .iter()
                .find(|skill| skill.name == "managed")
                .unwrap()
                .management_status,
            SkillManagementStatus::ToolManaged
        );
        assert_eq!(
            inventory
                .direct_skills
                .iter()
                .find(|skill| skill.name == "external")
                .unwrap()
                .management_status,
            SkillManagementStatus::External
        );
        fs::write(managed.join("changed.txt"), "changed").unwrap();
        let changed = scan_client_inventory(&client("kiro", root.path()), &state);
        assert_eq!(
            changed
                .direct_skills
                .iter()
                .find(|skill| skill.name == "managed")
                .unwrap()
                .management_status,
            SkillManagementStatus::Modified
        );
    }

    #[test]
    fn codex_inventory_scans_current_and_legacy_global_directories() {
        let profile = tempfile::tempdir().unwrap();
        let current = profile.path().join(".agents/skills");
        let legacy = profile.path().join(".codex/skills");
        skill(&current, "current");
        skill(&legacy, "legacy");
        let inventory =
            scan_client_inventory(&client("codex", &current), &PersistedState::default());
        assert_eq!(
            inventory
                .direct_skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["current", "legacy"]
        );
    }

    #[test]
    fn inventory_discovers_existing_skills_below_container_directories() {
        let root = tempfile::tempdir().unwrap();
        skill(&root.path().join(".system"), "existing-system-skill");
        skill(&root.path().join("bundle"), "outer-skill");
        skill(
            &root.path().join("bundle/outer-skill/embedded"),
            "embedded-source-skill",
        );

        let inventory =
            scan_client_inventory(&client("codex", root.path()), &PersistedState::default());
        assert_eq!(
            inventory
                .direct_skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["existing-system-skill", "outer-skill"]
        );
    }
}
