use crate::adapters::{adapters, passive_consumers_for, resolve_target};
use crate::domain::*;
use crate::{macos, skill, storage};
use chrono::Utc;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;
use uuid::Uuid;

pub struct AppState {
    pub data_dir: PathBuf,
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

#[tauri::command]
pub fn scan_clients() -> Vec<DetectedClient> {
    macos::scan_clients()
}

#[tauri::command]
pub async fn inspect_skill(
    source: SkillSource,
    state: State<'_, AppState>,
) -> Result<SkillMetadata, String> {
    skill::inspect(source, &state.data_dir)
        .await
        .map(|(metadata, _)| metadata)
}

#[tauri::command]
pub async fn plan_install(
    source: SkillSource,
    client_ids: Vec<String>,
    scope: InstallScope,
    project_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<InstallPlan, String> {
    if client_ids.is_empty() {
        return Err("请至少选择一个 Agent".to_string());
    }
    let clients = macos::scan_clients();
    let detected = crate::adapters::detected_map(&clients);
    for id in &client_ids {
        let client = detected
            .get(id)
            .ok_or_else(|| format!("未知 Agent: {id}"))?;
        if !client.supports_skills {
            return Err(format!("{} 当前不可安装 Skills", client.name));
        }
    }

    let (metadata, source_path) = skill::inspect(source, &state.data_dir).await?;
    let home = home_dir()?;
    let project = project_path.as_deref().map(Path::new);
    let adapter_map: HashMap<_, _> = adapters()
        .into_iter()
        .map(|adapter| (adapter.id, adapter))
        .collect();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for id in &client_ids {
        let adapter = adapter_map
            .get(id.as_str())
            .ok_or_else(|| format!("尚未适配 Agent: {id}"))?;
        let path = resolve_target(adapter, &home, project, scope, &metadata.name)?;
        groups
            .entry(normalized(&path))
            .or_default()
            .push(id.clone());
    }
    let persisted = storage::load_state(&state.data_dir)?;
    let mut entries = Vec::new();
    for (resolved_path, consumers) in groups {
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
        let passive = passive_consumers_for(path, &consumers, &clients);
        let mut warnings = Vec::new();
        if consumers.len() > 1 {
            warnings.push("多个 Agent 共用此物理目录，将只写入一次。".to_string());
        }
        if !passive.is_empty() {
            warnings.push("其他 Agent 可能自动发现此 Skill。".to_string());
        }
        entries.push(InstallPlanEntry {
            resolved_path,
            consumers,
            passive_consumers: passive,
            conflict,
            existing_hash,
            warnings,
        });
    }
    let plan = InstallPlan {
        plan_id: Uuid::new_v4().to_string(),
        skill: metadata,
        scope,
        entries,
    };
    state
        .plans
        .lock()
        .map_err(|_| "安装计划锁已损坏".to_string())?
        .insert(
            plan.plan_id.clone(),
            PendingPlan {
                public: plan.clone(),
                source_path,
            },
        );
    Ok(plan)
}

#[tauri::command]
pub fn apply_install_plan(
    plan_id: String,
    overwrite_paths: Vec<String>,
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
    for entry in &pending.public.entries {
        let destination = PathBuf::from(&entry.resolved_path);
        let requires_confirmation = matches!(
            entry.conflict,
            ConflictState::Conflict | ConflictState::UpdateAvailable
        );
        if entry.conflict == ConflictState::NotWritable {
            results.push(OperationResult {
                path: entry.resolved_path.clone(),
                success: false,
                status: "notWritable".to_string(),
                message: "目标位置不可写入".to_string(),
            });
            continue;
        }
        if requires_confirmation && !overwrite_paths.contains(&entry.resolved_path) {
            results.push(OperationResult {
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
                skill_name: pending.public.skill.name.clone(),
                resolved_path: entry.resolved_path.clone(),
                source: pending.public.skill.source.clone(),
                source_details: pending.public.skill.source_details.clone(),
                content_hash: pending.public.skill.content_hash.clone(),
                scope: pending.public.scope,
                consumers: entry.consumers.clone(),
                passive_consumers: entry.passive_consumers.clone(),
                adapter_version: 1,
                installed_at: Utc::now().to_rfc3339(),
            });
            storage::save_state(&state.data_dir, &persisted)?;
            results.push(OperationResult {
                path: entry.resolved_path.clone(),
                success: true,
                status: "tracked".to_string(),
                message: "内容已相同，已纳入管理".to_string(),
            });
            continue;
        }
        let operation = (|| -> Result<(), String> {
            if let Some(backup) = storage::create_backup(&state.data_dir, &destination)? {
                persisted.backups.push(backup);
            }
            storage::atomic_replace(&pending.source_path, &destination)?;
            persisted
                .installations
                .retain(|item| item.resolved_path != entry.resolved_path);
            persisted.installations.push(PhysicalInstallation {
                id: Uuid::new_v4().to_string(),
                skill_name: pending.public.skill.name.clone(),
                resolved_path: entry.resolved_path.clone(),
                source: pending.public.skill.source.clone(),
                source_details: pending.public.skill.source_details.clone(),
                content_hash: pending.public.skill.content_hash.clone(),
                scope: pending.public.scope,
                consumers: entry.consumers.clone(),
                passive_consumers: entry.passive_consumers.clone(),
                adapter_version: 1,
                installed_at: Utc::now().to_rfc3339(),
            });
            storage::save_state(&state.data_dir, &persisted)
        })();
        results.push(match operation {
            Ok(()) => OperationResult {
                path: entry.resolved_path.clone(),
                success: true,
                status: "installed".to_string(),
                message: "安装完成".to_string(),
            },
            Err(message) => OperationResult {
                path: entry.resolved_path.clone(),
                success: false,
                status: "failed".to_string(),
                message,
            },
        });
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
            });
            continue;
        }
        match skill::inspect(installation.source.clone(), &state.data_dir).await {
            Ok((metadata, _)) if metadata.content_hash != installation.content_hash => {
                statuses.push(UpdateStatus {
                    installation_id: installation.id,
                    status: UpdateState::SourceChanged,
                    message: "来源有新内容".to_string(),
                });
            }
            Ok(_) => statuses.push(UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::Current,
                message: "已是最新".to_string(),
            }),
            Err(message) => statuses.push(UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::SourceUnavailable,
                message,
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
            path: installation.resolved_path,
            success: false,
            status: "confirmationRequired".to_string(),
            message: "目标被手工修改；确认后才能备份并卸载".to_string(),
        });
    }
    if path.exists() {
        if let Some(backup) = storage::create_backup(&state.data_dir, &path)? {
            persisted.backups.push(backup);
        }
        fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
    }
    persisted
        .installations
        .retain(|item| item.id != installation_id);
    storage::save_state(&state.data_dir, &persisted)?;
    Ok(OperationResult {
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
    if let Some(new_backup) = storage::create_backup(&state.data_dir, &destination)? {
        persisted.backups.push(new_backup);
    }
    storage::atomic_replace(Path::new(&backup.backup_path), &destination)?;
    storage::save_state(&state.data_dir, &persisted)?;
    Ok(OperationResult {
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
    let payload = json!({
        "generatedAt": Utc::now().to_rfc3339(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "platform": "macOS",
        "clients": clients,
        "state": persisted,
        "privacy": "User home paths are redacted; Skill contents are not included."
    });
    let raw = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
    Ok(storage::redact_home(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_paths_remove_parent_segments() {
        assert_eq!(normalized(Path::new("/tmp/project/../demo")), "/tmp/demo");
    }
}
