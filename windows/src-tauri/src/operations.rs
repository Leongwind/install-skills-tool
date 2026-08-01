use crate::adapters::{adapters, detected_map, passive_consumers_for, resolve_global_target};
use crate::domain::*;
use crate::storage;
use chrono::Utc;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

fn normalized(path: &Path) -> String {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    let text = result.display().to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

pub fn build_install_plan(
    inspection: &SourceInspection,
    assignments: &[SkillAssignment],
    clients: &[DetectedClient],
    user_profile: &Path,
    persisted: &PersistedState,
) -> Result<InstallPlan, String> {
    if assignments.is_empty() {
        return Err("请至少选择一个 Skill".to_string());
    }
    let detected = detected_map(clients);
    let adapter_map = adapters()
        .into_iter()
        .map(|adapter| (adapter.id, adapter))
        .collect::<HashMap<_, _>>();
    let skills_by_id = inspection
        .skills
        .iter()
        .map(|skill| (skill.skill_id.as_str(), skill))
        .collect::<HashMap<_, _>>();
    let mut selected = Vec::new();
    let mut selected_ids = HashSet::new();
    let mut selected_names: HashMap<&str, &str> = HashMap::new();
    let mut groups: BTreeMap<(String, String), (String, Vec<String>)> = BTreeMap::new();

    for assignment in assignments {
        let metadata = skills_by_id
            .get(assignment.skill_id.as_str())
            .ok_or_else(|| format!("来源检查中不存在 Skill: {}", assignment.skill_id))?;
        if assignment.client_ids.is_empty() {
            return Err(format!("请为 {} 至少选择一个 IDE", metadata.name));
        }
        if let Some(existing_hash) = selected_names.insert(&metadata.name, &metadata.content_hash) {
            if existing_hash != metadata.content_hash {
                return Err(format!(
                    "存在同名但内容不同的 Skill: {}，请只保留一个",
                    metadata.name
                ));
            }
        }
        if selected_ids.insert(metadata.skill_id.as_str()) {
            selected.push((*metadata).clone());
        }
        for id in &assignment.client_ids {
            let client = detected.get(id).ok_or_else(|| format!("未知 IDE: {id}"))?;
            if !client.supports_skills {
                return Err(format!("{} 当前不可安装 Skills", client.name));
            }
            let adapter = adapter_map
                .get(id.as_str())
                .ok_or_else(|| format!("尚未适配 IDE: {id}"))?;
            let path = resolve_global_target(adapter, user_profile, &metadata.name);
            let key = (metadata.skill_id.clone(), normalized(&path));
            let group = groups
                .entry(key)
                .or_insert_with(|| (path.display().to_string(), Vec::new()));
            if !group.1.contains(id) {
                group.1.push(id.clone());
            }
        }
    }

    let mut entries = Vec::new();
    for ((skill_id, _), (resolved_path, consumers)) in groups {
        let metadata = skills_by_id
            .get(skill_id.as_str())
            .expect("grouped skill exists");
        let path = Path::new(&resolved_path);
        let existing_hash = path
            .is_dir()
            .then(|| storage::inspect_tree(path).ok().map(|result| result.0))
            .flatten();
        let tracked = persisted
            .installations
            .iter()
            .find(|record| normalized(Path::new(&record.resolved_path)) == normalized(path));
        let conflict = if !storage::writable_target(path) {
            ConflictState::NotWritable
        } else if path.exists() && !path.is_dir() {
            ConflictState::Conflict
        } else if let Some(existing) = existing_hash.as_ref() {
            if existing == &metadata.content_hash {
                ConflictState::Identical
            } else if tracked.is_some_and(|record| existing == &record.content_hash) {
                ConflictState::UpdateAvailable
            } else {
                ConflictState::Conflict
            }
        } else {
            ConflictState::NotInstalled
        };
        let passive_consumers = passive_consumers_for(path, &consumers, clients);
        let mut warnings = Vec::new();
        if consumers.len() > 1 {
            warnings.push("多个 IDE 共用此物理目录，只会写入一次。".to_string());
        }
        if !passive_consumers.is_empty() {
            warnings.push("其他 IDE 可能被动发现此 Skill。".to_string());
        }
        entries.push(InstallPlanEntry {
            entry_id: Uuid::new_v4().to_string(),
            skill_id,
            skill_name: metadata.name.clone(),
            resolved_path,
            consumers,
            passive_consumers,
            conflict,
            existing_hash,
            warnings,
        });
    }
    Ok(InstallPlan {
        plan_id: Uuid::new_v4().to_string(),
        skills: selected,
        entries,
    })
}

fn installation_record(metadata: &SkillMetadata, entry: &InstallPlanEntry) -> PhysicalInstallation {
    PhysicalInstallation {
        id: Uuid::new_v4().to_string(),
        skill_name: metadata.name.clone(),
        resolved_path: entry.resolved_path.clone(),
        source: Some(metadata.source.clone()),
        source_details: metadata.source_details.clone(),
        content_hash: metadata.content_hash.clone(),
        consumers: entry.consumers.clone(),
        passive_consumers: entry.passive_consumers.clone(),
        adapter_version: 1,
        installed_at: Utc::now().to_rfc3339(),
        provenance: InstallationProvenance::Tool,
    }
}

pub fn apply_install_plan(
    pending: PendingPlan,
    overwrite_entry_ids: &[String],
    data_dir: &Path,
) -> Result<Vec<OperationResult>, String> {
    let mut persisted = storage::load_state(data_dir)?;
    let mut results = Vec::new();
    for entry in &pending.public.entries {
        let metadata = pending
            .public
            .skills
            .iter()
            .find(|skill| skill.skill_id == entry.skill_id)
            .ok_or_else(|| format!("安装计划缺少 Skill: {}", entry.skill_name))?;
        let source = pending
            .source_paths
            .get(&entry.skill_id)
            .ok_or_else(|| format!("安装计划缺少来源: {}", entry.skill_name))?;
        let destination = PathBuf::from(&entry.resolved_path);
        let confirmation_required = matches!(
            entry.conflict,
            ConflictState::Conflict | ConflictState::UpdateAvailable
        );
        if entry.conflict == ConflictState::NotWritable {
            results.push(result(entry, false, "notWritable", "目标位置不可写入"));
            continue;
        }
        if confirmation_required && !overwrite_entry_ids.contains(&entry.entry_id) {
            results.push(result(
                entry,
                false,
                "confirmationRequired",
                "需要逐项确认覆盖",
            ));
            continue;
        }
        let operation = (|| -> Result<&'static str, String> {
            if entry.conflict == ConflictState::Identical {
                persisted.installations.retain(|record| {
                    normalized(Path::new(&record.resolved_path)) != normalized(&destination)
                });
                persisted
                    .installations
                    .push(installation_record(metadata, entry));
                storage::save_state(data_dir, &persisted)?;
                return Ok("内容相同，已纳入管理");
            }
            if let Some(backup) = storage::create_backup(data_dir, &destination)? {
                persisted.backups.push(backup);
            }
            storage::atomic_replace(source, &destination)?;
            persisted.installations.retain(|record| {
                normalized(Path::new(&record.resolved_path)) != normalized(&destination)
            });
            persisted
                .installations
                .push(installation_record(metadata, entry));
            storage::save_state(data_dir, &persisted)?;
            Ok("安装完成")
        })();
        match operation {
            Ok(message) => results.push(result(entry, true, "installed", message)),
            Err(message) => results.push(result(entry, false, "failed", &message)),
        }
    }
    Ok(results)
}

fn result(entry: &InstallPlanEntry, success: bool, status: &str, message: &str) -> OperationResult {
    OperationResult {
        entry_id: Some(entry.entry_id.clone()),
        skill_name: Some(entry.skill_name.clone()),
        path: entry.resolved_path.clone(),
        success,
        status: status.to_string(),
        message: message.to_string(),
    }
}

pub fn adopt_external_skill(
    client_id: &str,
    resolved_path: &str,
    data_dir: &Path,
    clients: &[DetectedClient],
) -> Result<PhysicalInstallation, String> {
    let client = clients
        .iter()
        .find(|client| client.id == client_id)
        .ok_or_else(|| format!("未知 IDE: {client_id}"))?;
    let roots = crate::inventory::inventory_roots(client)
        .into_iter()
        .filter_map(|root| root.canonicalize().ok())
        .collect::<Vec<_>>();
    let path = PathBuf::from(resolved_path)
        .canonicalize()
        .map_err(|error| format!("Skill 路径无效: {error}"))?;
    if !roots
        .iter()
        .any(|root| path.parent() == Some(root.as_path()))
    {
        return Err("只能纳管已知 IDE 全局目录中的直接子目录".to_string());
    }
    let mut persisted = storage::load_state(data_dir)?;
    if persisted
        .installations
        .iter()
        .any(|record| normalized(Path::new(&record.resolved_path)) == normalized(&path))
    {
        return Err("该 Skill 已在管理中".to_string());
    }
    let inventory = crate::inventory::scan_client_inventory(client, &persisted);
    let item = inventory
        .direct_skills
        .into_iter()
        .find(|item| Path::new(&item.resolved_path).canonicalize().ok().as_ref() == Some(&path))
        .ok_or_else(|| "未在 IDE 目录中发现该 Skill".to_string())?;
    if item.validity == SkillValidity::Unsafe {
        return Err("该 Skill 无法安全哈希，不能纳入管理".to_string());
    }
    let installation = PhysicalInstallation {
        id: Uuid::new_v4().to_string(),
        skill_name: item.name,
        resolved_path: path.display().to_string(),
        source: None,
        source_details: SkillSourceDetails::default(),
        content_hash: item
            .content_hash
            .ok_or_else(|| "无法计算 Skill 内容哈希".to_string())?,
        consumers: vec![client.id.clone()],
        passive_consumers: passive_consumers_for(&path, std::slice::from_ref(&client.id), clients),
        adapter_version: 1,
        installed_at: Utc::now().to_rfc3339(),
        provenance: InstallationProvenance::Adopted,
    };
    persisted.installations.push(installation.clone());
    storage::save_state(data_dir, &persisted)?;
    Ok(installation)
}

pub fn uninstall_installation(
    installation_id: &str,
    force: bool,
    data_dir: &Path,
) -> Result<OperationResult, String> {
    let mut persisted = storage::load_state(data_dir)?;
    let installation = persisted
        .installations
        .iter()
        .find(|record| record.id == installation_id)
        .cloned()
        .ok_or_else(|| "安装记录不存在".to_string())?;
    let path = PathBuf::from(&installation.resolved_path);
    let modified = storage::inspect_tree(&path)
        .ok()
        .map(|result| result.0)
        .as_deref()
        != Some(installation.content_hash.as_str());
    if modified && !force {
        return Ok(OperationResult {
            entry_id: None,
            skill_name: Some(installation.skill_name),
            path: installation.resolved_path,
            success: false,
            status: "confirmationRequired".to_string(),
            message: "目标已被修改，确认后才能备份并卸载".to_string(),
        });
    }
    if path.exists() {
        if let Some(backup) = storage::create_backup(data_dir, &path)? {
            persisted.backups.push(backup);
        }
        fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    }
    persisted
        .installations
        .retain(|record| record.id != installation_id);
    storage::save_state(data_dir, &persisted)?;
    Ok(OperationResult {
        entry_id: None,
        skill_name: Some(installation.skill_name),
        path: installation.resolved_path,
        success: true,
        status: "uninstalled".to_string(),
        message: "已卸载并保留备份".to_string(),
    })
}

pub fn restore_backup(backup_id: &str, data_dir: &Path) -> Result<OperationResult, String> {
    let mut persisted = storage::load_state(data_dir)?;
    let backup = persisted
        .backups
        .iter()
        .find(|record| record.id == backup_id)
        .cloned()
        .ok_or_else(|| "备份不存在".to_string())?;
    let destination = PathBuf::from(&backup.original_path);
    if let Some(current) = storage::create_backup(data_dir, &destination)? {
        persisted.backups.push(current);
    }
    storage::atomic_replace(Path::new(&backup.backup_path), &destination)?;
    storage::save_state(data_dir, &persisted)?;
    Ok(OperationResult {
        entry_id: None,
        skill_name: None,
        path: backup.original_path,
        success: true,
        status: "restored".to_string(),
        message: "备份已恢复".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(root: &Path, id: &str, name: &str, content: &str) -> SkillMetadata {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Fixture\n---\n{content}"),
        )
        .unwrap();
        let (hash, count, bytes, scripts) = storage::inspect_tree(&path).unwrap();
        SkillMetadata {
            skill_id: id.to_string(),
            relative_path: name.to_string(),
            name: name.to_string(),
            description: "Fixture".to_string(),
            source: SkillSource::LocalDirectory {
                path: root.display().to_string(),
            },
            source_details: SkillSourceDetails::default(),
            prepared_path: path.display().to_string(),
            content_hash: hash,
            file_count: count,
            total_bytes: bytes,
            has_scripts: scripts,
            warnings: Vec::new(),
        }
    }

    fn client(id: &str, profile: &Path) -> DetectedClient {
        let adapter = adapters().into_iter().find(|item| item.id == id).unwrap();
        DetectedClient {
            id: id.to_string(),
            name: adapter.name.to_string(),
            edition: adapter.edition,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: profile.join(adapter.global_relative).display().to_string(),
            supports_skills: true,
            notes: Vec::new(),
        }
    }

    fn inspection(skills: Vec<SkillMetadata>) -> SourceInspection {
        SourceInspection {
            inspection_id: "inspection".to_string(),
            source: skills[0].source.clone(),
            skills,
            rejected: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn assignment_planning_supports_different_skill_to_ide_combinations() {
        let source = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        let inspection = inspection(vec![
            skill(source.path(), "one", "alpha", "one"),
            skill(source.path(), "two", "beta", "two"),
        ]);
        let clients = vec![
            client("cursor", profile.path()),
            client("kiro", profile.path()),
        ];
        let plan = build_install_plan(
            &inspection,
            &[
                SkillAssignment {
                    skill_id: "one".to_string(),
                    client_ids: vec!["cursor".to_string(), "kiro".to_string()],
                },
                SkillAssignment {
                    skill_id: "two".to_string(),
                    client_ids: vec!["kiro".to_string()],
                },
            ],
            &clients,
            profile.path(),
            &PersistedState::default(),
        )
        .unwrap();
        assert_eq!(plan.entries.len(), 3);
        assert_eq!(
            plan.entries
                .iter()
                .filter(|entry| entry.skill_name == "alpha")
                .count(),
            2
        );
    }

    #[test]
    fn assignment_planning_rejects_unassigned_unknown_and_unavailable_ides() {
        let source = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        let inspection = inspection(vec![skill(source.path(), "one", "alpha", "one")]);
        let mut clients = vec![client("cursor", profile.path())];
        let unassigned = build_install_plan(
            &inspection,
            &[SkillAssignment {
                skill_id: "one".to_string(),
                client_ids: Vec::new(),
            }],
            &clients,
            profile.path(),
            &PersistedState::default(),
        );
        assert!(unassigned.is_err());
        let unknown = build_install_plan(
            &inspection,
            &[SkillAssignment {
                skill_id: "one".to_string(),
                client_ids: vec!["missing".to_string()],
            }],
            &clients,
            profile.path(),
            &PersistedState::default(),
        );
        assert!(unknown.is_err());
        clients[0].supports_skills = false;
        let unavailable = build_install_plan(
            &inspection,
            &[SkillAssignment {
                skill_id: "one".to_string(),
                client_ids: vec!["cursor".to_string()],
            }],
            &clients,
            profile.path(),
            &PersistedState::default(),
        );
        assert!(unavailable.is_err());
    }

    #[test]
    fn synthetic_environment_applies_scans_backs_up_restores_and_uninstalls() {
        let source = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let metadata = skill(source.path(), "one", "alpha", "v1");
        let inspection = inspection(vec![metadata.clone()]);
        let clients = vec![client("cursor", profile.path())];
        let plan = build_install_plan(
            &inspection,
            &[SkillAssignment {
                skill_id: "one".to_string(),
                client_ids: vec!["cursor".to_string()],
            }],
            &clients,
            profile.path(),
            &PersistedState::default(),
        )
        .unwrap();
        let source_paths = [("one".to_string(), PathBuf::from(&metadata.prepared_path))]
            .into_iter()
            .collect();
        let results = apply_install_plan(
            PendingPlan {
                public: plan,
                source_paths,
            },
            &[],
            data.path(),
        )
        .unwrap();
        assert!(results[0].success);
        let state = storage::load_state(data.path()).unwrap();
        let record = state.installations[0].clone();
        assert!(Path::new(&record.resolved_path).join("SKILL.md").is_file());
        fs::write(
            Path::new(&record.resolved_path).join("manual.txt"),
            "changed",
        )
        .unwrap();
        let confirmation = uninstall_installation(&record.id, false, data.path()).unwrap();
        assert_eq!(confirmation.status, "confirmationRequired");
        let removed = uninstall_installation(&record.id, true, data.path()).unwrap();
        assert!(removed.success);
        let backup = storage::load_state(data.path()).unwrap().backups[0].clone();
        let restored = restore_backup(&backup.id, data.path()).unwrap();
        assert!(restored.success);
        assert!(Path::new(&restored.path).join("manual.txt").is_file());
    }
}
