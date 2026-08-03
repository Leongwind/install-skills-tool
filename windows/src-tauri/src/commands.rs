use crate::domain::*;
use crate::{inventory, operations, skill, storage, windows};
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub data_dir: PathBuf,
    pub inspections: Mutex<HashMap<String, SourceInspection>>,
    pub plans: Mutex<HashMap<String, PendingPlan>>,
}

fn user_profile() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .ok_or_else(|| "无法确定 Windows 用户目录".to_string())
}

#[tauri::command]
pub fn scan_clients() -> Result<Vec<DetectedClient>, String> {
    windows::scan_clients()
}

#[tauri::command]
pub fn scan_environment(state: State<'_, AppState>) -> Result<EnvironmentScan, String> {
    let clients = windows::scan_clients()?;
    let persisted = storage::load_state(&state.data_dir)?;
    Ok(inventory::build_environment_scan(clients, &persisted))
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

#[tauri::command]
pub fn plan_install(
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
    let clients = windows::scan_clients()?;
    let persisted = storage::load_state(&state.data_dir)?;
    let plan = operations::build_install_plan(
        &inspection,
        &assignments,
        &clients,
        &user_profile()?,
        &persisted,
    )?;
    let selected_ids = plan
        .skills
        .iter()
        .map(|skill| skill.skill_id.as_str())
        .collect::<HashSet<_>>();
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
    operations::apply_install_plan(pending, &overwrite_entry_ids, &state.data_dir)
}

#[tauri::command]
pub fn adopt_external_skill(
    client_id: String,
    resolved_path: String,
    state: State<'_, AppState>,
) -> Result<PhysicalInstallation, String> {
    operations::adopt_external_skill(
        &client_id,
        &resolved_path,
        &state.data_dir,
        &windows::scan_clients()?,
    )
}

#[tauri::command]
pub fn list_installations(state: State<'_, AppState>) -> Result<Vec<PhysicalInstallation>, String> {
    Ok(storage::load_state(&state.data_dir)?.installations)
}

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>) -> Result<Vec<BackupRecord>, String> {
    Ok(storage::load_state(&state.data_dir)?.backups)
}

fn explorer_select_argument(path: &Path) -> String {
    format!(r#"/select,"{}""#, storage::normalized_path(path))
}

fn reveal_is_allowed(
    requested: &Path,
    environment: &EnvironmentScan,
    persisted: &PersistedState,
) -> bool {
    environment.inventories.iter().any(|inventory| {
        inventory.direct_skills.iter().any(|skill| {
            Path::new(&skill.resolved_path)
                .canonicalize()
                .is_ok_and(|candidate| candidate == requested)
        })
    }) || persisted.backups.iter().any(|backup| {
        Path::new(&backup.backup_path)
            .canonicalize()
            .is_ok_and(|candidate| candidate == requested)
    })
}

#[tauri::command]
pub fn reveal_in_explorer(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let requested = PathBuf::from(&path)
        .canonicalize()
        .map_err(|error| format!("路径无效: {error}"))?;
    let persisted = storage::load_state(&state.data_dir)?;
    let environment = inventory::build_environment_scan(windows::scan_clients()?, &persisted);
    if !reveal_is_allowed(&requested, &environment, &persisted) {
        return Err("只能在资源管理器中显示已扫描的 Skill 或备份".to_string());
    }
    std::process::Command::new("explorer.exe")
        .arg(explorer_select_argument(&requested))
        .spawn()
        .map_err(|error| format!("无法打开资源管理器: {error}"))?;
    Ok(())
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
    if let Some(subpath) = installation.source_details.subpath.as_deref() {
        return candidates
            .into_iter()
            .find(|skill| skill.source_details.subpath.as_deref() == Some(subpath))
            .ok_or_else(|| "来源中已找不到原 Skill 路径".to_string());
    }
    match candidates.as_slice() {
        [skill] => Ok(*skill),
        [] => Err("来源中已找不到该 Skill".to_string()),
        _ => Err("来源包含多个同名 Skill，无法确定更新目标".to_string()),
    }
}

#[tauri::command]
pub async fn check_updates(state: State<'_, AppState>) -> Result<Vec<UpdateStatus>, String> {
    let persisted = storage::load_state(&state.data_dir)?;
    let mut statuses = Vec::new();
    for installation in persisted.installations {
        let current_hash = storage::inspect_tree(Path::new(&installation.resolved_path))
            .ok()
            .map(|result| result.0);
        if current_hash.as_deref() != Some(installation.content_hash.as_str()) {
            statuses.push(UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::TargetModified,
                message: "目标内容已被手工修改".to_string(),
            });
            continue;
        }
        let Some(source) = installation.source.clone() else {
            statuses.push(UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::SourceUnavailable,
                message: "来源未绑定，仅管理本地副本".to_string(),
            });
            continue;
        };
        let inspected = skill::inspect_source(source, &state.data_dir)
            .await
            .and_then(|inspection| inspected_update(&inspection, &installation).cloned());
        statuses.push(match inspected {
            Ok(metadata) if metadata.content_hash != installation.content_hash => UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::SourceChanged,
                message: "来源有新内容".to_string(),
            },
            Ok(_) => UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::Current,
                message: "已是最新".to_string(),
            },
            Err(message) => UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::SourceUnavailable,
                message,
            },
        });
    }
    Ok(statuses)
}

#[tauri::command]
pub fn uninstall_installation(
    installation_id: String,
    force: bool,
    state: State<'_, AppState>,
) -> Result<OperationResult, String> {
    operations::uninstall_installation(&installation_id, force, &state.data_dir)
}

#[tauri::command]
pub fn restore_backup(
    backup_id: String,
    state: State<'_, AppState>,
) -> Result<OperationResult, String> {
    operations::restore_backup(&backup_id, &state.data_dir)
}

#[tauri::command]
pub fn export_diagnostics(state: State<'_, AppState>) -> Result<String, String> {
    let persisted = storage::load_state(&state.data_dir)?;
    let clients = windows::scan_clients()?;
    let environment = inventory::build_environment_scan(clients.clone(), &persisted);
    let payload = json!({
        "generatedAt": Utc::now().to_rfc3339(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "platform": "Windows",
        "clients": clients,
        "inventory": environment.inventories,
        "state": persisted,
        "privacy": "User profile paths are redacted. Skill file contents are not included."
    });
    let raw = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
    Ok(storage::redact_user_profile(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn explorer_select_argument_quotes_the_complete_path() {
        let expected = r#"/select,"c:\users\leong\.codex\skills\imagegen""#;
        assert_eq!(
            explorer_select_argument(Path::new(r"C:\Users\Leong\.codex\skills\imagegen")),
            expected
        );
        assert_eq!(
            explorer_select_argument(Path::new(r"\\?\C:\Users\Leong\.codex\skills\imagegen")),
            expected
        );
    }

    #[test]
    fn reveal_allowlist_accepts_scanned_skills_and_rejects_unrelated_paths() {
        let root = tempfile::tempdir().unwrap();
        let unrelated_root = tempfile::tempdir().unwrap();
        let skill_path = root.path().join("existing");
        fs::create_dir_all(&skill_path).unwrap();
        fs::write(
            skill_path.join("SKILL.md"),
            "---\nname: existing\ndescription: Fixture\n---\n",
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
            supports_skills: true,
            notes: Vec::new(),
        };
        let persisted = PersistedState::default();
        let environment = inventory::build_environment_scan(vec![client], &persisted);

        assert!(reveal_is_allowed(
            &skill_path.canonicalize().unwrap(),
            &environment,
            &persisted
        ));
        assert!(!reveal_is_allowed(
            &unrelated_root.path().canonicalize().unwrap(),
            &environment,
            &persisted
        ));
    }
}
