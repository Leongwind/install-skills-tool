use crate::adapters::{adapters, passive_consumers_for, resolve_global_target};
use crate::domain::*;
use crate::{macos, skill, storage};
use chrono::Utc;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

pub struct AppState {
    pub data_dir: PathBuf,
    pub inspections: Mutex<HashMap<String, SourceInspection>>,
    pub plans: Mutex<HashMap<String, PendingPlan>>,
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "无法确定用户目录".to_string())
}

fn normalized(path: &Path) -> String {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result.display().to_string()
}

fn record_backup(
    data_dir: &Path,
    persisted: &mut PersistedState,
    source: &Path,
) -> Result<Option<String>, String> {
    let id = storage::create_backup(data_dir, source)?.map(|backup| {
        let id = backup.id.clone();
        persisted.backups.push(backup);
        id
    });
    storage::enforce_backup_policy(persisted)?;
    Ok(id)
}

fn update_journal_target(
    persisted: &mut PersistedState,
    journal_id: &str,
    path: &str,
    backup_id: Option<String>,
    completed: bool,
) {
    if let Some(target) = persisted
        .operation_journals
        .iter_mut()
        .find(|journal| journal.id == journal_id)
        .and_then(|journal| {
            journal
                .targets
                .iter_mut()
                .find(|target| target.path == path)
        })
    {
        target.backup_id = backup_id;
        target.completed = completed;
    }
}

fn finish_journal(
    persisted: &mut PersistedState,
    journal_id: &str,
    status: OperationJournalStatus,
    message: Option<String>,
) {
    if let Some(journal) = persisted
        .operation_journals
        .iter_mut()
        .find(|journal| journal.id == journal_id)
    {
        journal.status = status;
        journal.finished_at = Some(Utc::now().to_rfc3339());
        journal.message = message;
    }
}

fn inspected_update<'a>(
    inspection: &'a SourceInspection,
    installation: &PhysicalInstallation,
) -> Result<&'a SkillMetadata, String> {
    let candidates = inspection
        .skills
        .iter()
        .filter(|skill| skill.name == installation.skill_name)
        .collect::<Vec<_>>();
    if let Some(expected) = installation.source_details.subpath.as_deref() {
        return candidates
            .into_iter()
            .find(|skill| skill.source_details.subpath.as_deref() == Some(expected))
            .ok_or_else(|| "来源中已找不到原 Skill 路径".to_string());
    }
    match candidates.as_slice() {
        [skill] => Ok(*skill),
        [] => Err("来源中已找不到该 Skill".to_string()),
        _ => Err("来源包含多个同名 Skill，无法确定更新目标".to_string()),
    }
}

#[tauri::command]
pub fn scan_environment(state: State<'_, AppState>) -> Result<EnvironmentScan, String> {
    let clients = macos::scan_clients();
    let persisted = storage::load_state(&state.data_dir)?;
    Ok(crate::inventory::build_environment_scan(
        clients, &persisted,
    ))
}

#[tauri::command]
pub async fn inspect_source(
    source: SkillSource,
    state: State<'_, AppState>,
) -> Result<SourceInspection, String> {
    let inspection = skill::inspect_source(source, &state.data_dir).await?;
    state
        .inspections
        .lock()
        .map_err(|_| "来源检查锁已损坏".to_string())?
        .insert(inspection.inspection_id.clone(), inspection.clone());
    Ok(inspection)
}

pub fn build_install_plan(
    inspection: &SourceInspection,
    assignments: &[SkillAssignment],
    clients: &[DetectedClient],
    home: &Path,
    persisted: &PersistedState,
) -> Result<InstallPlan, String> {
    if assignments.is_empty() {
        return Err("请至少选择一个 Skill".to_string());
    }
    let detected = crate::adapters::detected_map(clients);
    let adapter_map: HashMap<_, _> = adapters()
        .into_iter()
        .map(|adapter| (adapter.id, adapter))
        .collect();
    let skills_by_id: HashMap<_, _> = inspection
        .skills
        .iter()
        .map(|skill| (skill.skill_id.as_str(), skill))
        .collect();
    let mut selected = Vec::new();
    let mut selected_names: HashMap<&str, &str> = HashMap::new();
    let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();

    for assignment in assignments {
        let metadata = skills_by_id
            .get(assignment.skill_id.as_str())
            .ok_or_else(|| format!("来源检查中不存在 Skill: {}", assignment.skill_id))?;
        if assignment.client_ids.is_empty() {
            return Err(format!("请为 {} 至少选择一个 Agent", metadata.name));
        }
        if let Some(existing_hash) = selected_names.insert(&metadata.name, &metadata.content_hash) {
            if existing_hash != metadata.content_hash {
                return Err(format!(
                    "存在同名但内容不同的 Skill: {}，请只保留一个",
                    metadata.name
                ));
            }
        }
        selected.push((*metadata).clone());
        for id in &assignment.client_ids {
            let client = detected
                .get(id)
                .ok_or_else(|| format!("未知 Agent: {id}"))?;
            if !client.supports_skills {
                return Err(format!("{} 当前不可安装 Skills", client.name));
            }
            let adapter = adapter_map
                .get(id.as_str())
                .ok_or_else(|| format!("尚未适配 Agent: {id}"))?;
            let path = resolve_global_target(adapter, home, &metadata.name);
            groups
                .entry((metadata.skill_id.clone(), normalized(&path)))
                .or_default()
                .push(id.clone());
        }
    }

    let mut entries = Vec::new();
    for ((skill_id, resolved_path), consumers) in groups {
        let metadata = skills_by_id
            .get(skill_id.as_str())
            .expect("grouped skill exists");
        let path = Path::new(&resolved_path);
        let existing_hash = if path.is_dir() {
            storage::inspect_tree(path).ok().map(|value| value.0)
        } else {
            None
        };
        let tracked = persisted
            .installations
            .iter()
            .find(|item| item.resolved_path == resolved_path);
        let conflict = if !storage::writable_target(path) {
            ConflictState::NotWritable
        } else if path.exists() && !path.is_dir() {
            ConflictState::Conflict
        } else if let Some(existing) = existing_hash.as_ref() {
            if existing == &metadata.content_hash {
                ConflictState::Identical
            } else if let Some(tracked) = tracked {
                if existing == &tracked.content_hash {
                    ConflictState::UpdateAvailable
                } else {
                    ConflictState::Conflict
                }
            } else {
                ConflictState::Conflict
            }
        } else {
            ConflictState::NotInstalled
        };
        let passive = passive_consumers_for(path, &consumers, clients);
        let mut warnings = Vec::new();
        if consumers.len() > 1 {
            warnings.push("多个 Agent 共用此物理目录，将只写入一次。".to_string());
        }
        if !passive.is_empty() {
            warnings.push("其他 Agent 可能自动发现此 Skill。".to_string());
        }
        entries.push(InstallPlanEntry {
            entry_id: Uuid::new_v4().to_string(),
            skill_id,
            skill_name: metadata.name.clone(),
            resolved_path,
            consumers,
            passive_consumers: passive,
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

#[tauri::command]
pub async fn plan_install(
    inspection_id: String,
    assignments: Vec<SkillAssignment>,
    state: State<'_, AppState>,
) -> Result<InstallPlan, String> {
    let inspection = state
        .inspections
        .lock()
        .map_err(|_| "来源检查锁已损坏".to_string())?
        .get(&inspection_id)
        .cloned()
        .ok_or_else(|| "来源检查不存在或已过期".to_string())?;
    let clients = macos::scan_clients();
    let persisted = storage::load_state(&state.data_dir)?;
    let plan = build_install_plan(
        &inspection,
        &assignments,
        &clients,
        &home_dir()?,
        &persisted,
    )?;
    let selected_ids: HashSet<_> = plan
        .skills
        .iter()
        .map(|skill| skill.skill_id.as_str())
        .collect();
    let source_paths = inspection
        .skills
        .iter()
        .filter(|skill| selected_ids.contains(skill.skill_id.as_str()))
        .map(|skill| (skill.skill_id.clone(), PathBuf::from(&skill.prepared_path)))
        .collect();
    state
        .plans
        .lock()
        .map_err(|_| "安装计划锁已损坏".to_string())?
        .insert(
            plan.plan_id.clone(),
            PendingPlan {
                public: plan.clone(),
                source_paths,
            },
        );
    Ok(plan)
}

#[tauri::command]
pub fn apply_install_plan(
    plan_id: String,
    overwrite_entry_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<OperationResult>, String> {
    let pending = state
        .plans
        .lock()
        .map_err(|_| "安装计划锁已损坏".to_string())?
        .remove(&plan_id)
        .ok_or_else(|| "安装计划不存在或已执行".to_string())?;
    let mut persisted = storage::load_state(&state.data_dir)?;
    let mut results = Vec::new();
    let journal_id = Uuid::new_v4().to_string();
    let journal_targets = pending
        .public
        .entries
        .iter()
        .filter(|entry| {
            entry.conflict != ConflictState::NotWritable
                && entry.conflict != ConflictState::Identical
                && (!matches!(
                    entry.conflict,
                    ConflictState::Conflict | ConflictState::UpdateAvailable
                ) || overwrite_entry_ids.contains(&entry.entry_id))
        })
        .map(|entry| OperationJournalTarget {
            path: entry.resolved_path.clone(),
            existed_before: Path::new(&entry.resolved_path).exists(),
            backup_id: None,
            completed: false,
            previous_installation: persisted
                .installations
                .iter()
                .find(|installation| installation.resolved_path == entry.resolved_path)
                .cloned(),
        })
        .collect::<Vec<_>>();
    let has_journal_targets = !journal_targets.is_empty();
    if has_journal_targets {
        persisted.operation_journals.push(OperationJournal {
            id: journal_id.clone(),
            operation_type: "install".to_string(),
            created_at: Utc::now().to_rfc3339(),
            finished_at: None,
            status: OperationJournalStatus::Applying,
            targets: journal_targets,
            message: None,
        });
        storage::save_state(&state.data_dir, &persisted)?;
    }
    for entry in &pending.public.entries {
        let metadata = pending
            .public
            .skills
            .iter()
            .find(|skill| skill.skill_id == entry.skill_id)
            .ok_or_else(|| format!("安装计划缺少 Skill: {}", entry.skill_name))?;
        let source_path = pending
            .source_paths
            .get(&entry.skill_id)
            .ok_or_else(|| format!("安装计划缺少来源目录: {}", entry.skill_name))?;
        let destination = PathBuf::from(&entry.resolved_path);
        let requires_confirmation = matches!(
            entry.conflict,
            ConflictState::Conflict | ConflictState::UpdateAvailable
        );
        if entry.conflict == ConflictState::NotWritable {
            results.push(OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: false,
                status: "notWritable".to_string(),
                message: "目标位置不可写入".to_string(),
            });
            continue;
        }
        if requires_confirmation && !overwrite_entry_ids.contains(&entry.entry_id) {
            results.push(OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: false,
                status: "confirmationRequired".to_string(),
                message: "需要确认覆盖".to_string(),
            });
            continue;
        }
        if entry.conflict == ConflictState::Identical {
            persisted
                .installations
                .retain(|item| item.resolved_path != entry.resolved_path);
            persisted.installations.push(PhysicalInstallation {
                id: Uuid::new_v4().to_string(),
                skill_name: metadata.name.clone(),
                resolved_path: entry.resolved_path.clone(),
                source: Some(metadata.source.clone()),
                source_details: metadata.source_details.clone(),
                content_hash: metadata.content_hash.clone(),
                scope: InstallScope::Global,
                consumers: entry.consumers.clone(),
                passive_consumers: entry.passive_consumers.clone(),
                adapter_version: 1,
                installed_at: Utc::now().to_rfc3339(),
                provenance: InstallationProvenance::Tool,
                legacy_project: false,
            });
            storage::save_state(&state.data_dir, &persisted)?;
            results.push(OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: true,
                status: "tracked".to_string(),
                message: "内容已相同，已纳入管理".to_string(),
            });
            continue;
        }
        let operation = (|| -> Result<(), String> {
            let backup_id = record_backup(&state.data_dir, &mut persisted, &destination)?;
            update_journal_target(
                &mut persisted,
                &journal_id,
                &entry.resolved_path,
                backup_id,
                true,
            );
            storage::save_state(&state.data_dir, &persisted)?;
            storage::atomic_replace(source_path, &destination)?;
            persisted
                .installations
                .retain(|item| item.resolved_path != entry.resolved_path);
            persisted.installations.push(PhysicalInstallation {
                id: Uuid::new_v4().to_string(),
                skill_name: metadata.name.clone(),
                resolved_path: entry.resolved_path.clone(),
                source: Some(metadata.source.clone()),
                source_details: metadata.source_details.clone(),
                content_hash: metadata.content_hash.clone(),
                scope: InstallScope::Global,
                consumers: entry.consumers.clone(),
                passive_consumers: entry.passive_consumers.clone(),
                adapter_version: 1,
                installed_at: Utc::now().to_rfc3339(),
                provenance: InstallationProvenance::Tool,
                legacy_project: false,
            });
            storage::save_state(&state.data_dir, &persisted)
        })();
        results.push(match operation {
            Ok(()) => OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: true,
                status: "installed".to_string(),
                message: "安装完成".to_string(),
            },
            Err(message) => OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: false,
                status: "failed".to_string(),
                message,
            },
        });
    }
    if has_journal_targets {
        let failures = results
            .iter()
            .filter(|result| !result.success && result.status == "failed")
            .count();
        finish_journal(
            &mut persisted,
            &journal_id,
            if failures == 0 {
                OperationJournalStatus::Completed
            } else {
                OperationJournalStatus::Partial
            },
            (failures > 0).then(|| format!("{failures} 个目标失败，可执行恢复")),
        );
        storage::save_state(&state.data_dir, &persisted)?;
    }
    Ok(results)
}

#[tauri::command]
pub fn list_installations(state: State<'_, AppState>) -> Result<Vec<PhysicalInstallation>, String> {
    Ok(storage::load_state(&state.data_dir)?.installations)
}

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>) -> Result<Vec<BackupRecord>, String> {
    Ok(storage::load_state(&state.data_dir)?.backups)
}

#[tauri::command]
pub fn get_app_overview(state: State<'_, AppState>) -> Result<AppOverview, String> {
    let persisted = storage::load_state(&state.data_dir)?;
    Ok(AppOverview {
        backup_policy: persisted.backup_policy,
        operation_journals: persisted.operation_journals,
    })
}

#[tauri::command]
pub fn scan_client_inventory(
    client_id: String,
    state: State<'_, AppState>,
) -> Result<ClientSkillInventory, String> {
    let client = macos::scan_clients()
        .into_iter()
        .find(|client| client.id == client_id)
        .ok_or_else(|| format!("未知 Agent: {client_id}"))?;
    let persisted = storage::load_state(&state.data_dir)?;
    Ok(crate::inventory::scan_client_inventory(&client, &persisted))
}

pub fn recover_operation_inner(
    journal_id: &str,
    data_dir: &Path,
) -> Result<Vec<OperationResult>, String> {
    let mut persisted = storage::load_state(data_dir)?;
    let journal = persisted
        .operation_journals
        .iter()
        .find(|journal| journal.id == journal_id)
        .cloned()
        .ok_or_else(|| "恢复记录不存在".to_string())?;
    if !matches!(
        journal.status,
        OperationJournalStatus::RecoveryRequired | OperationJournalStatus::Partial
    ) {
        return Err("该操作当前不需要恢复".to_string());
    }
    let mut results = Vec::new();
    for target in journal
        .targets
        .iter()
        .rev()
        .filter(|target| target.completed)
    {
        let destination = PathBuf::from(&target.path);
        let recovered = if let Some(backup_id) = target.backup_id.as_deref() {
            persisted
                .backups
                .iter()
                .find(|backup| backup.id == backup_id)
                .ok_or_else(|| format!("恢复所需备份不存在: {backup_id}"))
                .and_then(|backup| {
                    storage::atomic_replace(Path::new(&backup.backup_path), &destination)
                })
        } else if !target.existed_before && destination.exists() {
            if destination.is_dir() {
                fs::remove_dir_all(&destination).map_err(|error| error.to_string())
            } else {
                fs::remove_file(&destination).map_err(|error| error.to_string())
            }
        } else {
            Ok(())
        };
        match recovered {
            Ok(()) => {
                persisted
                    .installations
                    .retain(|installation| installation.resolved_path != target.path);
                if let Some(previous) = target.previous_installation.clone() {
                    persisted.installations.push(previous);
                }
                results.push(OperationResult {
                    entry_id: None,
                    skill_name: target
                        .previous_installation
                        .as_ref()
                        .map(|installation| installation.skill_name.clone()),
                    path: target.path.clone(),
                    success: true,
                    status: "rolledBack".to_string(),
                    message: "已恢复到操作前状态".to_string(),
                });
            }
            Err(message) => results.push(OperationResult {
                entry_id: None,
                skill_name: None,
                path: target.path.clone(),
                success: false,
                status: "recoveryFailed".to_string(),
                message,
            }),
        }
    }
    let failures = results.iter().filter(|result| !result.success).count();
    finish_journal(
        &mut persisted,
        journal_id,
        if failures == 0 {
            OperationJournalStatus::RolledBack
        } else {
            OperationJournalStatus::RecoveryRequired
        },
        (failures > 0).then(|| format!("{failures} 个目标恢复失败")),
    );
    storage::save_state(data_dir, &persisted)?;
    Ok(results)
}

#[tauri::command]
pub fn recover_operation(
    journal_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<OperationResult>, String> {
    recover_operation_inner(&journal_id, &state.data_dir)
}

pub fn export_skill_bundle_inner(
    installation_ids: &[String],
    destination: &Path,
    data_dir: &Path,
) -> Result<PortableBundleManifest, String> {
    if installation_ids.is_empty() {
        return Err("请至少选择一个受管理 Skill".to_string());
    }
    let persisted = storage::load_state(data_dir)?;
    let selected = installation_ids
        .iter()
        .map(|id| {
            persisted
                .installations
                .iter()
                .find(|installation| installation.id == *id && !installation.legacy_project)
                .cloned()
                .ok_or_else(|| format!("安装记录不存在或不可导出: {id}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let unique_names = selected
        .iter()
        .map(|installation| installation.skill_name.as_str())
        .collect::<HashSet<_>>();
    if unique_names.len() != selected.len() {
        return Err("所选记录包含同名 Skill，不能写入同一个便携包".to_string());
    }
    if selected.iter().any(|installation| {
        destination.starts_with(Path::new(&installation.resolved_path))
    }) {
        return Err("便携包不能保存到被导出的 Skill 目录中".to_string());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "导出路径无父目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".skill-bundle-{}.tmp", Uuid::new_v4()));
    let file = fs::File::create(&temporary).map_err(|error| error.to_string())?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut manifest = PortableBundleManifest {
        schema_version: 1,
        exported_at: Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        skills: Vec::new(),
    };
    for installation in selected {
        let root = PathBuf::from(&installation.resolved_path);
        let (current_hash, _, _, _) = storage::inspect_tree(&root)?;
        for item in WalkDir::new(&root).follow_links(false) {
            let item = item.map_err(|error| error.to_string())?;
            if item.file_type().is_symlink() {
                return Err(format!("便携包不接受软链接: {}", item.path().display()));
            }
            if !item.file_type().is_file() {
                continue;
            }
            let relative = item
                .path()
                .strip_prefix(&root)
                .map_err(|error| error.to_string())?
                .to_str()
                .ok_or_else(|| "文件名不是有效 UTF-8".to_string())?
                .replace('\\', "/");
            let archive_path = format!("{}/{relative}", installation.skill_name);
            writer
                .start_file(archive_path, options)
                .map_err(|error| error.to_string())?;
            let mut source = fs::File::open(item.path()).map_err(|error| error.to_string())?;
            std::io::copy(&mut source, &mut writer).map_err(|error| error.to_string())?;
        }
        manifest.skills.push(PortableBundleEntry {
            skill_name: installation.skill_name.clone(),
            content_hash: current_hash,
            consumers: installation.consumers,
            archive_path: installation.skill_name,
        });
    }
    writer
        .start_file("skill-installer-manifest.json", options)
        .map_err(|error| error.to_string())?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    writer
        .write_all(&manifest_bytes)
        .map_err(|error| error.to_string())?;
    writer.finish().map_err(|error| error.to_string())?;
    if destination.exists() {
        fs::remove_file(destination).map_err(|error| error.to_string())?;
    }
    fs::rename(temporary, destination).map_err(|error| error.to_string())?;
    Ok(manifest)
}

#[tauri::command]
pub fn export_skill_bundle(
    installation_ids: Vec<String>,
    destination: String,
    state: State<'_, AppState>,
) -> Result<PortableBundleManifest, String> {
    export_skill_bundle_inner(&installation_ids, Path::new(&destination), &state.data_dir)
}

pub fn adopt_external_skill_inner(
    client_id: &str,
    resolved_path: &str,
    data_dir: &Path,
    clients: &[DetectedClient],
) -> Result<PhysicalInstallation, String> {
    let client = clients
        .iter()
        .find(|client| client.id == client_id)
        .ok_or_else(|| format!("未知 Agent: {client_id}"))?;
    let roots = crate::inventory::inventory_roots(client)
        .into_iter()
        .filter_map(|root| root.canonicalize().ok())
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Err("Agent Skills 目录无效".to_string());
    }
    let path = PathBuf::from(resolved_path)
        .canonicalize()
        .map_err(|error| format!("Skill 路径无效: {error}"))?;
    if !roots
        .iter()
        .any(|root| path.parent() == Some(root.as_path()))
    {
        return Err("只能纳管 Agent 全局目录中的直接子目录".to_string());
    }
    let mut persisted = storage::load_state(data_dir)?;
    let inventory = crate::inventory::scan_client_inventory(client, &persisted);
    let item = inventory
        .direct_skills
        .into_iter()
        .find(|item| {
            Path::new(&item.resolved_path)
                .canonicalize()
                .is_ok_and(|candidate| candidate == path)
        })
        .ok_or_else(|| "未在 Agent 目录中发现该 Skill".to_string())?;
    if item.validity == SkillValidity::Unsafe {
        return Err("该 Skill 无法安全哈希，不能纳入管理".to_string());
    }
    if item.installation_id.is_some() {
        return Err("该 Skill 已在管理中".to_string());
    }
    let content_hash = item
        .content_hash
        .ok_or_else(|| "无法计算 Skill 内容哈希".to_string())?;
    let installation = PhysicalInstallation {
        id: Uuid::new_v4().to_string(),
        skill_name: item.name,
        resolved_path: path.display().to_string(),
        source: None,
        source_details: SkillSourceDetails::default(),
        content_hash,
        scope: InstallScope::Global,
        consumers: vec![client.id.clone()],
        passive_consumers: passive_consumers_for(&path, std::slice::from_ref(&client.id), clients),
        adapter_version: 1,
        installed_at: Utc::now().to_rfc3339(),
        provenance: InstallationProvenance::Adopted,
        legacy_project: false,
    };
    persisted.installations.push(installation.clone());
    storage::save_state(data_dir, &persisted)?;
    Ok(installation)
}

#[tauri::command]
pub fn adopt_external_skill(
    client_id: String,
    resolved_path: String,
    state: State<'_, AppState>,
) -> Result<PhysicalInstallation, String> {
    adopt_external_skill_inner(
        &client_id,
        &resolved_path,
        &state.data_dir,
        &macos::scan_clients(),
    )
}

#[tauri::command]
pub async fn check_updates(state: State<'_, AppState>) -> Result<Vec<UpdateStatus>, String> {
    let persisted = storage::load_state(&state.data_dir)?;
    let mut statuses = Vec::new();
    for installation in persisted.installations {
        let target_hash = storage::inspect_tree(Path::new(&installation.resolved_path))
            .ok()
            .map(|value| value.0);
        if target_hash.as_deref() != Some(installation.content_hash.as_str()) {
            statuses.push(UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::TargetModified,
                message: "目标内容已被手工修改".to_string(),
                current_hash: target_hash,
                source_hash: None,
                source_revision: None,
                changes: None,
            });
            continue;
        }
        let Some(source) = installation.source.clone() else {
            statuses.push(UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::SourceUnavailable,
                message: "来源未绑定，仅管理本地副本".to_string(),
                current_hash: target_hash,
                source_hash: None,
                source_revision: None,
                changes: None,
            });
            continue;
        };
        match skill::inspect_source(source, &state.data_dir)
            .await
            .and_then(|inspection| inspected_update(&inspection, &installation).cloned())
        {
            Ok(metadata) if metadata.content_hash != installation.content_hash => {
                statuses.push(UpdateStatus {
                    installation_id: installation.id,
                    status: UpdateState::SourceChanged,
                    message: "来源有新内容".to_string(),
                    current_hash: target_hash,
                    source_hash: Some(metadata.content_hash.clone()),
                    source_revision: metadata.source_details.commit_sha.clone(),
                    changes: storage::compare_trees(
                        Path::new(&installation.resolved_path),
                        Path::new(&metadata.prepared_path),
                    )
                    .ok(),
                });
            }
            Ok(metadata) => statuses.push(UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::Current,
                message: "已是最新".to_string(),
                current_hash: target_hash,
                source_hash: Some(metadata.content_hash),
                source_revision: metadata.source_details.commit_sha,
                changes: Some(FileChangeSummary::default()),
            }),
            Err(message) => statuses.push(UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::SourceUnavailable,
                message,
                current_hash: target_hash,
                source_hash: None,
                source_revision: None,
                changes: None,
            }),
        }
    }
    Ok(statuses)
}

#[tauri::command]
pub fn uninstall_installation(
    installation_id: String,
    force: bool,
    state: State<'_, AppState>,
) -> Result<OperationResult, String> {
    let mut persisted = storage::load_state(&state.data_dir)?;
    let installation = persisted
        .installations
        .iter()
        .find(|item| item.id == installation_id)
        .cloned()
        .ok_or_else(|| "安装记录不存在".to_string())?;
    let path = PathBuf::from(&installation.resolved_path);
    let current_hash = storage::inspect_tree(&path).ok().map(|value| value.0);
    let modified = current_hash.as_deref() != Some(installation.content_hash.as_str());
    if modified && !force {
        return Ok(OperationResult {
            entry_id: None,
            skill_name: Some(installation.skill_name.clone()),
            path: installation.resolved_path,
            success: false,
            status: "confirmationRequired".to_string(),
            message: "目标被手工修改；确认后才能备份并卸载".to_string(),
        });
    }
    if path.exists() {
        record_backup(&state.data_dir, &mut persisted, &path)?;
        fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
    }
    persisted
        .installations
        .retain(|item| item.id != installation_id);
    storage::save_state(&state.data_dir, &persisted)?;
    Ok(OperationResult {
        entry_id: None,
        skill_name: Some(installation.skill_name),
        path: installation.resolved_path,
        success: true,
        status: "uninstalled".to_string(),
        message: "已卸载并保留备份".to_string(),
    })
}

#[tauri::command]
pub fn restore_backup(
    backup_id: String,
    state: State<'_, AppState>,
) -> Result<OperationResult, String> {
    let mut persisted = storage::load_state(&state.data_dir)?;
    let backup = persisted
        .backups
        .iter()
        .find(|item| item.id == backup_id)
        .cloned()
        .ok_or_else(|| "备份不存在".to_string())?;
    let destination = PathBuf::from(&backup.original_path);
    record_backup(&state.data_dir, &mut persisted, &destination)?;
    storage::atomic_replace(Path::new(&backup.backup_path), &destination)?;
    storage::save_state(&state.data_dir, &persisted)?;
    Ok(OperationResult {
        entry_id: None,
        skill_name: None,
        path: backup.original_path,
        success: true,
        status: "restored".to_string(),
        message: "备份已恢复".to_string(),
    })
}

#[tauri::command]
pub fn export_diagnostics(state: State<'_, AppState>) -> Result<String, String> {
    let persisted = storage::load_state(&state.data_dir)?;
    let clients = macos::scan_clients();
    let environment = crate::inventory::build_environment_scan(clients.clone(), &persisted);
    let payload = json!({
        "generatedAt": Utc::now().to_rfc3339(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "platform": "macOS",
        "clients": clients,
        "inventory": environment.inventories,
        "state": persisted,
        "privacy": "User home paths are redacted; Skill contents are not included."
    });
    let raw = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
    Ok(storage::redact_home(&raw))
}

#[tauri::command]
pub fn reveal_in_finder(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let requested = PathBuf::from(&path)
        .canonicalize()
        .map_err(|error| format!("路径无效: {error}"))?;
    let persisted = storage::load_state(&state.data_dir)?;
    let environment = crate::inventory::build_environment_scan(macos::scan_clients(), &persisted);
    let allowed = environment.inventories.iter().any(|inventory| {
        inventory.direct_skills.iter().any(|skill| {
            Path::new(&skill.resolved_path)
                .canonicalize()
                .is_ok_and(|candidate| candidate == requested)
        })
    }) || persisted.backups.iter().any(|backup| {
        Path::new(&backup.backup_path)
            .canonicalize()
            .is_ok_and(|candidate| candidate == requested)
    });
    if !allowed {
        return Err("只能在 Finder 中显示已扫描的 Skill 或备份".to_string());
    }
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(&requested)
        .status()
        .map_err(|error| format!("无法打开 Finder: {error}"))?;
    if !status.success() {
        return Err("Finder 未能显示该路径".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_paths_remove_parent_segments() {
        assert_eq!(normalized(Path::new("/tmp/project/../demo")), "/tmp/demo");
    }

    #[test]
    fn matrix_assignments_create_entries_per_skill_and_client() {
        let skill = |id: &str, name: &str| SkillMetadata {
            skill_id: id.to_string(),
            relative_path: name.to_string(),
            name: name.to_string(),
            description: name.to_string(),
            source: SkillSource::LocalDirectory {
                path: "/tmp/source".to_string(),
            },
            source_details: SkillSourceDetails::default(),
            prepared_path: format!("/tmp/source/{name}"),
            content_hash: format!("{name}-hash"),
            file_count: 1,
            total_bytes: 10,
            has_scripts: false,
            warnings: Vec::new(),
        };
        let inspection = SourceInspection {
            inspection_id: "inspection".to_string(),
            source: SkillSource::LocalDirectory {
                path: "/tmp/source".to_string(),
            },
            skills: vec![skill("one-id", "one"), skill("two-id", "two")],
            rejected: Vec::new(),
            warnings: Vec::new(),
        };
        let client = |id: &str, root: &str| DetectedClient {
            id: id.to_string(),
            name: id.to_string(),
            edition: ClientEdition::Standard,
            version: Some("1.0.0".to_string()),
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: root.to_string(),
            inventory_skills_paths: vec![root.to_string()],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };
        let assignments = vec![
            SkillAssignment {
                skill_id: "one-id".to_string(),
                client_ids: vec!["codex".to_string(), "kiro".to_string()],
            },
            SkillAssignment {
                skill_id: "two-id".to_string(),
                client_ids: vec!["kiro".to_string()],
            },
        ];

        let plan = build_install_plan(
            &inspection,
            &assignments,
            &[client("codex", "/tmp/agents"), client("kiro", "/tmp/kiro")],
            Path::new("/tmp/home"),
            &PersistedState::default(),
        )
        .unwrap();

        assert_eq!(plan.entries.len(), 3);
        assert!(plan.entries.iter().any(|entry| {
            entry.skill_name == "one" && entry.resolved_path == "/tmp/home/.agents/skills/one"
        }));
        assert!(plan.entries.iter().any(|entry| {
            entry.skill_name == "two" && entry.resolved_path == "/tmp/home/.kiro/skills/two"
        }));
    }

    #[test]
    fn adopting_external_skill_records_a_safe_baseline() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("external");
        fs::create_dir(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: external\ndescription: External\n---\n",
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

        let adopted = adopt_external_skill_inner(
            "kiro",
            &skill.display().to_string(),
            data.path(),
            &[client],
        )
        .unwrap();

        assert_eq!(adopted.provenance, InstallationProvenance::Adopted);
        assert!(adopted.source.is_none());
        assert_eq!(
            storage::load_state(data.path())
                .unwrap()
                .installations
                .len(),
            1
        );
    }

    #[test]
    fn adopting_external_skill_rejects_paths_outside_the_client_root() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(
            outside.path().join("SKILL.md"),
            "---\nname: outside\ndescription: Outside\n---\n",
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

        let error = adopt_external_skill_inner(
            "kiro",
            &outside.path().display().to_string(),
            data.path(),
            &[client],
        )
        .unwrap_err();

        assert_eq!(error, "只能纳管 Agent 全局目录中的直接子目录");
    }

    #[test]
    fn adopting_codex_legacy_skill_records_the_existing_path() {
        let data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_root = home.path().join(".agents/skills");
        let legacy_skill = home.path().join(".codex/skills/legacy");
        fs::create_dir_all(&current_root).unwrap();
        fs::create_dir_all(&legacy_skill).unwrap();
        fs::write(
            legacy_skill.join("SKILL.md"),
            "---\nname: legacy\ndescription: Legacy\n---\n",
        )
        .unwrap();
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
                legacy_skill.parent().unwrap().display().to_string(),
            ],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let adopted = adopt_external_skill_inner(
            "codex",
            &legacy_skill.display().to_string(),
            data.path(),
            &[client],
        )
        .unwrap();

        assert_eq!(
            PathBuf::from(adopted.resolved_path),
            legacy_skill.canonicalize().unwrap()
        );
        assert_eq!(adopted.provenance, InstallationProvenance::Adopted);
    }

    #[test]
    fn matrix_rejects_same_name_with_different_content() {
        let first = SkillMetadata {
            skill_id: "first".to_string(),
            relative_path: "one/demo".to_string(),
            name: "demo".to_string(),
            description: "First".to_string(),
            source: SkillSource::LocalDirectory {
                path: "/tmp/source".to_string(),
            },
            source_details: SkillSourceDetails::default(),
            prepared_path: "/tmp/source/one/demo".to_string(),
            content_hash: "first-hash".to_string(),
            file_count: 1,
            total_bytes: 10,
            has_scripts: false,
            warnings: Vec::new(),
        };
        let mut second = first.clone();
        second.skill_id = "second".to_string();
        second.relative_path = "two/demo".to_string();
        second.prepared_path = "/tmp/source/two/demo".to_string();
        second.content_hash = "second-hash".to_string();
        let inspection = SourceInspection {
            inspection_id: "inspection".to_string(),
            source: first.source.clone(),
            skills: vec![first.clone(), second],
            rejected: Vec::new(),
            warnings: Vec::new(),
        };
        let client = DetectedClient {
            id: "kiro".to_string(),
            name: "Kiro".to_string(),
            edition: ClientEdition::Standard,
            version: None,
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: "/tmp/kiro".to_string(),
            inventory_skills_paths: vec!["/tmp/kiro".to_string()],
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let error = build_install_plan(
            &inspection,
            &[
                SkillAssignment {
                    skill_id: first.skill_id,
                    client_ids: vec!["kiro".to_string()],
                },
                SkillAssignment {
                    skill_id: "second".to_string(),
                    client_ids: vec!["kiro".to_string()],
                },
            ],
            &[client],
            Path::new("/tmp/home"),
            &PersistedState::default(),
        )
        .unwrap_err();

        assert!(error.contains("同名但内容不同"));
    }

    #[test]
    fn recovery_removes_a_new_target_from_an_interrupted_install() {
        let data = tempfile::tempdir().unwrap();
        let target = data.path().join("target/demo");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "new").unwrap();
        let mut persisted = PersistedState::default();
        persisted.operation_journals.push(OperationJournal {
            id: "journal".to_string(),
            operation_type: "install".to_string(),
            created_at: Utc::now().to_rfc3339(),
            finished_at: None,
            status: OperationJournalStatus::RecoveryRequired,
            targets: vec![OperationJournalTarget {
                path: target.display().to_string(),
                existed_before: false,
                backup_id: None,
                completed: true,
                previous_installation: None,
            }],
            message: None,
        });
        storage::save_state(data.path(), &persisted).unwrap();

        let results = recover_operation_inner("journal", data.path()).unwrap();

        assert_eq!(results.len(), 1);
        assert!(!target.exists());
        assert_eq!(
            storage::load_state(data.path()).unwrap().operation_journals[0].status,
            OperationJournalStatus::RolledBack
        );
    }

    #[test]
    fn portable_bundle_contains_skills_and_manifest() {
        let data = tempfile::tempdir().unwrap();
        let skill = data.path().join("installed/demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();
        let hash = storage::inspect_tree(&skill).unwrap().0;
        let mut persisted = PersistedState::default();
        persisted.installations.push(PhysicalInstallation {
            id: "demo-id".to_string(),
            skill_name: "demo".to_string(),
            resolved_path: skill.display().to_string(),
            source: None,
            source_details: SkillSourceDetails::default(),
            content_hash: hash,
            scope: InstallScope::Global,
            consumers: vec!["codex".to_string()],
            passive_consumers: Vec::new(),
            adapter_version: 1,
            installed_at: Utc::now().to_rfc3339(),
            provenance: InstallationProvenance::Adopted,
            legacy_project: false,
        });
        storage::save_state(data.path(), &persisted).unwrap();
        let destination = data.path().join("portable.zip");

        let manifest =
            export_skill_bundle_inner(&["demo-id".to_string()], &destination, data.path()).unwrap();

        assert_eq!(manifest.skills.len(), 1);
        let file = fs::File::open(destination).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert!(archive.by_name("demo/SKILL.md").is_ok());
        assert!(archive.by_name("skill-installer-manifest.json").is_ok());
    }
}
