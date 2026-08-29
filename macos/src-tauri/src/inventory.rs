use crate::domain::*;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;
use walkdir::WalkDir;

const MAX_INVENTORY_DEPTH: usize = 16;
const MAX_INVENTORY_ENTRIES: usize = 10_000;

#[derive(Deserialize)]
struct InventoryFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

fn read_frontmatter(path: &Path) -> Result<InventoryFrontmatter, String> {
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

fn ignored_inventory_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | "node_modules" | "target" | "__MACOSX"))
}

fn inventory_entry(
    client: &DetectedClient,
    path: PathBuf,
    state: &PersistedState,
    is_symlink: bool,
) -> InventorySkill {
    let directory_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut issues = Vec::new();
    if is_symlink {
        issues.push("软链接仅供查看，不能纳入管理或卸载".to_string());
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
            issues,
            consumers: vec![client.id.clone()],
            passive_from_client_id: None,
        };
    }
    let frontmatter = read_frontmatter(&path);
    let (name, description, mut validity) = match frontmatter {
        Ok(frontmatter) => {
            let mut valid = true;
            let name = match frontmatter.name {
                Some(name) if !name.trim().is_empty() => name,
                _ => {
                    issues.push("frontmatter 缺少 name".to_string());
                    valid = false;
                    directory_name.clone()
                }
            };
            let pattern =
                Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("valid skill name regex");
            if name.len() > 64 || !pattern.is_match(&name) {
                issues.push("name 不符合 Agent Skills 规范".to_string());
                valid = false;
            }
            if name != directory_name {
                issues.push(format!(
                    "frontmatter name “{name}” 与目录名 “{directory_name}” 不一致"
                ));
                valid = false;
            }
            if frontmatter
                .description
                .as_deref()
                .is_none_or(|description| description.trim().is_empty())
            {
                issues.push("frontmatter 缺少 description".to_string());
                valid = false;
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
    let tracked = state.installations.iter().find(|installation| {
        !installation.legacy_project && installation.resolved_path == resolved_path
    });
    let management_status = if validity == SkillValidity::Unsafe {
        SkillManagementStatus::Unsafe
    } else if let Some(installation) = tracked {
        if content_hash.as_deref() != Some(installation.content_hash.as_str()) {
            SkillManagementStatus::Modified
        } else {
            match installation.provenance {
                InstallationProvenance::Tool => SkillManagementStatus::ToolManaged,
                InstallationProvenance::Adopted => SkillManagementStatus::Adopted,
            }
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
        installation_id: tracked.map(|installation| installation.id.clone()),
        issues,
        consumers: tracked
            .map(|installation| installation.consumers.clone())
            .unwrap_or_else(|| vec![client.id.clone()]),
        passive_from_client_id: None,
    }
}

pub fn scan_client_inventory(
    client: &DetectedClient,
    state: &PersistedState,
) -> ClientSkillInventory {
    scan_client_inventory_with_control(client, state, None, None)
}

pub fn scan_client_inventory_with_control(
    client: &DetectedClient,
    state: &PersistedState,
    cancel_requested: Option<&AtomicBool>,
    on_progress: Option<&dyn Fn(usize)>,
) -> ClientSkillInventory {
    let mut direct_skills = Vec::new();
    let mut scan_errors = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut scanned_entries = 0usize;
    let mut capped = false;
    for root in inventory_roots(client) {
        if capped {
            break;
        }
        if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            scan_errors.push("扫描已取消".to_string());
            break;
        }
        if !root.exists() {
            continue;
        }
        let metadata = match fs::symlink_metadata(&root) {
            Ok(metadata) => metadata,
            Err(error) => {
                scan_errors.push(format!("{}: {error}", root.display()));
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            scan_errors.push(format!("{}: 根目录是软链接", root.display()));
            continue;
        }
        let mut walker = WalkDir::new(&root).follow_links(false).into_iter();
        while let Some(item) = walker.next() {
            let item = match item {
                Ok(item) => item,
                Err(error) => {
                    scan_errors.push(error.to_string());
                    continue;
                }
            };
            if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                scan_errors.push("扫描已取消".to_string());
                break;
            }
            scanned_entries = scanned_entries.saturating_add(1);
            if let Some(callback) = on_progress {
                callback(scanned_entries);
            }
            if scanned_entries > MAX_INVENTORY_ENTRIES {
                scan_errors.push(format!(
                    "{}: 库存条目超过限制 {}，其余内容未扫描",
                    root.display(),
                    MAX_INVENTORY_ENTRIES
                ));
                capped = true;
                break;
            }
            let path = item.path();
            if path == root {
                continue;
            }
            if item.depth() > MAX_INVENTORY_DEPTH {
                if item.file_type().is_dir() {
                    walker.skip_current_dir();
                }
                if item.depth() == MAX_INVENTORY_DEPTH + 1 {
                    scan_errors.push(format!(
                        "{}: 库存嵌套深度超过限制 {}，其余内容未扫描",
                        root.display(),
                        MAX_INVENTORY_DEPTH
                    ));
                }
                continue;
            }
            if item.file_type().is_dir() && ignored_inventory_directory(path) {
                walker.skip_current_dir();
                continue;
            }
            let metadata = match fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    scan_errors.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                if seen_paths.insert(path.display().to_string()) {
                    direct_skills.push(inventory_entry(client, path.to_path_buf(), state, true));
                }
                if metadata.is_dir() {
                    walker.skip_current_dir();
                }
                continue;
            }
            if metadata.is_dir() && path.join("SKILL.md").is_file() {
                let identity = path
                    .canonicalize()
                    .map(|candidate| candidate.display().to_string())
                    .unwrap_or_else(|_| path.display().to_string());
                if seen_paths.insert(identity) {
                    direct_skills.push(inventory_entry(client, path.to_path_buf(), state, false));
                }
                // A discovered Skill is a root; don't mistake examples under
                // references/ or assets/ for nested Skills.
                walker.skip_current_dir();
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
        scan_error: (!scan_errors.is_empty()).then(|| scan_errors.join("\n")),
    }
}

pub fn inventory_roots(client: &DetectedClient) -> Vec<PathBuf> {
    if !client.inventory_skills_paths.is_empty() {
        return client
            .inventory_skills_paths
            .iter()
            .map(PathBuf::from)
            .collect();
    }
    let primary = PathBuf::from(&client.global_skills_path);
    let mut roots = vec![primary.clone()];
    if client.id == "codex" {
        if let Some(home) = primary.parent().and_then(Path::parent) {
            let legacy = home.join(".codex/skills");
            if legacy != primary {
                roots.push(legacy);
            }
        }
    }
    roots
}

pub fn build_environment_scan(
    clients: Vec<DetectedClient>,
    state: &PersistedState,
) -> EnvironmentScan {
    build_environment_scan_with_control(clients, state, None, None)
}

pub fn build_environment_scan_with_control(
    clients: Vec<DetectedClient>,
    state: &PersistedState,
    cancel_requested: Option<&AtomicBool>,
    on_progress: Option<&dyn Fn(usize)>,
) -> EnvironmentScan {
    let mut inventories = clients
        .iter()
        .enumerate()
        .map(|(index, client)| {
            let inventory =
                scan_client_inventory_with_control(client, state, cancel_requested, None);
            if let Some(callback) = on_progress {
                callback(index + 1);
            }
            inventory
        })
        .collect::<Vec<_>>();
    let codex_skills = inventories
        .iter()
        .find(|inventory| inventory.client_id == "codex")
        .map(|inventory| {
            inventory
                .direct_skills
                .iter()
                .filter(|skill| skill.validity != SkillValidity::Unsafe)
                .cloned()
                .collect::<Vec<InventorySkill>>()
        })
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
                if inventory
                    .direct_skills
                    .iter()
                    .any(|item| item.resolved_path == skill.resolved_path)
                {
                    continue;
                }
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
    use std::fs;

    #[test]
    fn client_inventory_lists_external_and_managed_skills() {
        let root = tempfile::tempdir().unwrap();
        for name in ["external", "managed"] {
            let path = root.path().join(name);
            fs::create_dir(&path).unwrap();
            fs::write(
                path.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name}\n---\n"),
            )
            .unwrap();
        }
        let managed_path = root.path().join("managed");
        let managed_hash = crate::storage::inspect_tree(&managed_path).unwrap().0;
        let state = PersistedState {
            schema_version: 3,
            revision: 0,
            installations: vec![PhysicalInstallation {
                id: "managed-id".to_string(),
                skill_name: "managed".to_string(),
                resolved_path: managed_path.display().to_string(),
                source: None,
                source_details: SkillSourceDetails::default(),
                content_hash: managed_hash,
                scope: InstallScope::Global,
                consumers: vec!["kiro".to_string()],
                passive_consumers: Vec::new(),
                adapter_version: 1,
                installed_at: "2026-01-01T00:00:00Z".to_string(),
                provenance: InstallationProvenance::Adopted,
                legacy_project: false,
            }],
            backups: Vec::new(),
            operation_journals: Vec::new(),
            backup_policy: BackupPolicy::default(),
            pinned_installation_ids: Vec::new(),
        };
        let client = DetectedClient {
            id: "kiro".to_string(),
            name: "Kiro".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: root.path().display().to_string(),
            inventory_skills_paths: vec![root.path().display().to_string()],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let inventory = scan_client_inventory(&client, &state);

        assert_eq!(inventory.direct_skills.len(), 2);
        assert_eq!(
            inventory
                .direct_skills
                .iter()
                .find(|skill| skill.name == "external")
                .unwrap()
                .management_status,
            SkillManagementStatus::External
        );
        assert_eq!(
            inventory
                .direct_skills
                .iter()
                .find(|skill| skill.name == "managed")
                .unwrap()
                .management_status,
            SkillManagementStatus::Adopted
        );
    }

    #[test]
    fn client_inventory_discovers_nested_skills_and_deduplicates_roots() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("catalog/engineering/demo");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();
        let client = DetectedClient {
            id: "cursor".to_string(),
            name: "Cursor".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: root.path().display().to_string(),
            inventory_skills_paths: vec![
                root.path().display().to_string(),
                root.path().join("catalog").display().to_string(),
            ],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let inventory = scan_client_inventory(&client, &PersistedState::default());

        assert_eq!(inventory.direct_skills.len(), 1);
        assert_eq!(inventory.direct_skills[0].name, "demo");
    }

    #[test]
    fn client_inventory_reports_depth_limit_instead_of_walking_unbounded_tree() {
        let root = tempfile::tempdir().unwrap();
        let mut deep = root.path().to_path_buf();
        for index in 0..(MAX_INVENTORY_DEPTH + 3) {
            deep = deep.join(format!("level-{index}"));
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(
            deep.join("SKILL.md"),
            "---\nname: deep\ndescription: Deep\n---\n",
        )
        .unwrap();
        let client = DetectedClient {
            id: "kiro".to_string(),
            name: "Kiro".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: root.path().display().to_string(),
            inventory_skills_paths: vec![root.path().display().to_string()],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let inventory = scan_client_inventory(&client, &PersistedState::default());

        assert!(inventory.direct_skills.is_empty());
        assert!(inventory
            .scan_error
            .as_deref()
            .is_some_and(|error| error.contains("嵌套深度超过限制")));
    }

    #[test]
    fn client_inventory_honors_cancellation_between_entries() {
        let root = tempfile::tempdir().unwrap();
        for name in ["first", "second"] {
            let skill = root.path().join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name}\n---\n"),
            )
            .unwrap();
        }
        let client = DetectedClient {
            id: "kiro".to_string(),
            name: "Kiro".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: root.path().display().to_string(),
            inventory_skills_paths: vec![root.path().display().to_string()],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };
        let cancelled = AtomicBool::new(false);
        let on_progress = |completed: usize| {
            if completed >= 2 {
                cancelled.store(true, Ordering::Release);
            }
        };

        let inventory = scan_client_inventory_with_control(
            &client,
            &PersistedState::default(),
            Some(&cancelled),
            Some(&on_progress),
        );

        assert!(cancelled.load(Ordering::Acquire));
        assert!(inventory
            .scan_error
            .as_deref()
            .is_some_and(|error| error.contains("扫描已取消")));
        assert!(inventory.direct_skills.len() < 2);
    }

    #[cfg(unix)]
    #[test]
    fn client_inventory_marks_root_symlinks_as_unsafe() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(
            outside.path().join("SKILL.md"),
            "---\nname: linked\ndescription: Linked\n---\n",
        )
        .unwrap();
        symlink(outside.path(), root.path().join("linked")).unwrap();
        let client = DetectedClient {
            id: "kiro".to_string(),
            name: "Kiro".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: root.path().display().to_string(),
            inventory_skills_paths: vec![root.path().display().to_string()],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let inventory = scan_client_inventory(&client, &PersistedState::default());

        assert_eq!(inventory.direct_skills.len(), 1);
        assert_eq!(inventory.direct_skills[0].validity, SkillValidity::Unsafe);
        assert_eq!(
            inventory.direct_skills[0].management_status,
            SkillManagementStatus::Unsafe
        );
    }

    #[test]
    fn client_inventory_keeps_readable_nonconforming_skills() {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("readable");
        fs::create_dir(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "---\nname: readable\n---\n").unwrap();
        let client = DetectedClient {
            id: "kiro".to_string(),
            name: "Kiro".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: root.path().display().to_string(),
            inventory_skills_paths: vec![root.path().display().to_string()],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let inventory = scan_client_inventory(&client, &PersistedState::default());

        assert_eq!(
            inventory.direct_skills[0].validity,
            SkillValidity::NonConforming
        );
        assert_eq!(
            inventory.direct_skills[0].management_status,
            SkillManagementStatus::External
        );
        assert!(inventory.direct_skills[0]
            .issues
            .iter()
            .any(|issue| issue.contains("description")));
    }

    #[test]
    fn codex_inventory_includes_current_and_legacy_global_roots() {
        let home = tempfile::tempdir().unwrap();
        let current_root = home.path().join(".agents/skills");
        let legacy_root = home.path().join(".codex/skills");
        for (root, name) in [(&current_root, "current"), (&legacy_root, "legacy")] {
            let skill = root.join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name}\n---\n"),
            )
            .unwrap();
        }
        let client = DetectedClient {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: current_root.display().to_string(),
            inventory_skills_paths: vec![
                current_root.display().to_string(),
                legacy_root.display().to_string(),
            ],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let inventory = scan_client_inventory(&client, &PersistedState::default());

        assert_eq!(
            inventory
                .direct_skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["current", "legacy"]
        );
        assert!(inventory.direct_skills.iter().any(|skill| {
            skill.resolved_path == legacy_root.join("legacy").display().to_string()
        }));
    }

    #[test]
    fn codex_shared_skills_are_reported_as_passive_for_cursor() {
        let home = tempfile::tempdir().unwrap();
        let shared_root = home.path().join(".agents/skills");
        let skill = shared_root.join("shared");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: shared\ndescription: Shared\n---\n",
        )
        .unwrap();
        let codex = DetectedClient {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: shared_root.display().to_string(),
            inventory_skills_paths: vec![shared_root.display().to_string()],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };
        let cursor_root = home.path().join(".cursor/skills");
        fs::create_dir_all(&cursor_root).unwrap();
        let cursor = DetectedClient {
            id: "cursor".to_string(),
            name: "Cursor".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: cursor_root.display().to_string(),
            inventory_skills_paths: vec![cursor_root.display().to_string()],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let scan = build_environment_scan(vec![codex, cursor], &PersistedState::default());
        let cursor_inventory = scan
            .inventories
            .iter()
            .find(|inventory| inventory.client_id == "cursor")
            .unwrap();
        assert!(cursor_inventory.direct_skills.is_empty());
        assert_eq!(cursor_inventory.passive_skills.len(), 1);
        assert_eq!(
            cursor_inventory.passive_skills[0]
                .passive_from_client_id
                .as_deref(),
            Some("codex")
        );
    }
}
