use crate::adapters::{
    adapters, passive_consumers_for, resolve_global_target, CURRENT_ADAPTER_VERSION,
};
use crate::catalog;
use crate::domain::*;
use crate::{macos, skill, storage};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::State;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

const PLAN_TTL: ChronoDuration = ChronoDuration::minutes(15);

fn plan_window() -> (String, String) {
    let created = Utc::now();
    (created.to_rfc3339(), (created + PLAN_TTL).to_rfc3339())
}

fn plan_expired(created_at: &str, expires_at: &str) -> bool {
    let now = Utc::now();
    let Ok(created) = DateTime::parse_from_rfc3339(created_at) else {
        return true;
    };
    let Ok(expires) = DateTime::parse_from_rfc3339(expires_at) else {
        return true;
    };
    created.with_timezone(&Utc) > now || expires.with_timezone(&Utc) <= now
}

fn target_guard(path: &Path) -> PlanTargetGuard {
    PlanTargetGuard {
        existed: path.exists(),
        content_hash: if path.is_dir() {
            storage::inspect_tree(path).ok().map(|value| value.0)
        } else if path.is_file() {
            storage::hash_file(path).ok()
        } else {
            None
        },
    }
}

fn target_guard_matches(path: &Path, expected: &PlanTargetGuard) -> bool {
    if path.exists() != expected.existed {
        return false;
    }
    if !expected.existed {
        return true;
    }
    match expected.content_hash.as_deref() {
        Some(expected_hash) if path.is_dir() => storage::inspect_tree(path)
            .ok()
            .is_some_and(|value| value.0 == expected_hash),
        Some(expected_hash) if path.is_file() => storage::hash_file(path)
            .ok()
            .is_some_and(|value| value == expected_hash),
        Some(_) => false,
        None => false,
    }
}

fn stale_install_results(plan: &InstallPlan, message: &str) -> Vec<OperationResult> {
    plan.entries
        .iter()
        .map(|entry| OperationResult {
            entry_id: Some(entry.entry_id.clone()),
            skill_name: Some(entry.skill_name.clone()),
            path: entry.resolved_path.clone(),
            success: false,
            status: "stale".to_string(),
            message: message.to_string(),
        })
        .collect()
}

fn stale_update_results(plan: &UpdatePlan, message: &str) -> Vec<OperationResult> {
    plan.entries
        .iter()
        .map(|entry| OperationResult {
            entry_id: Some(entry.entry_id.clone()),
            skill_name: Some(entry.skill_name.clone()),
            path: entry.resolved_path.clone(),
            success: false,
            status: "stale".to_string(),
            message: message.to_string(),
        })
        .collect()
}

fn validate_install_plan_guards(pending: &PendingPlan, data_dir: &Path) -> Option<String> {
    if plan_expired(&pending.created_at, &pending.expires_at) {
        return Some("安装计划已过期，请重新生成预览".to_string());
    }
    for skill in &pending.public.skills {
        let Some(expected_hash) = pending.source_guards.get(&skill.skill_id) else {
            return Some(format!("{} 缺少来源校验，请重新生成预览", skill.name));
        };
        let source = pending.source_paths.get(&skill.skill_id);
        let actual_hash = source
            .and_then(|path| storage::inspect_tree(path).ok())
            .map(|value| value.0);
        if actual_hash.as_deref() != Some(expected_hash.as_str()) {
            return Some(format!("{} 的来源快照已变化，请重新生成预览", skill.name));
        }
    }
    for entry in &pending.public.entries {
        let Some(expected) = pending.target_guards.get(&entry.entry_id) else {
            return Some(format!("{} 缺少目标校验，请重新生成预览", entry.skill_name));
        };
        if !target_guard_matches(Path::new(&entry.resolved_path), expected) {
            return Some(format!("{} 的目标已变化，请重新生成预览", entry.skill_name));
        }
    }
    let persisted = match storage::load_state(data_dir) {
        Ok(state) => state,
        Err(error) => return Some(format!("无法读取安装状态，计划已失效: {error}")),
    };
    for entry in &pending.public.entries {
        let Some(expected) = pending.installation_guards.get(&entry.entry_id) else {
            return Some(format!(
                "{} 缺少安装记录校验，请重新生成预览",
                entry.skill_name
            ));
        };
        let actual = persisted
            .installations
            .iter()
            .find(|installation| installation.resolved_path == entry.resolved_path)
            .map(|installation| installation.id.clone());
        if &actual != expected {
            return Some(format!(
                "{} 的安装记录已变化，请重新生成预览",
                entry.skill_name
            ));
        }
    }
    None
}

fn validate_update_plan_guards(pending: &PendingUpdatePlan, data_dir: &Path) -> Option<String> {
    if plan_expired(&pending.created_at, &pending.expires_at) {
        return Some("更新计划已过期，请重新生成预览".to_string());
    }
    for entry in &pending.public.entries {
        let Some(expected) = pending.target_guards.get(&entry.entry_id) else {
            return Some(format!("{} 缺少目标校验，请重新生成预览", entry.skill_name));
        };
        if !target_guard_matches(Path::new(&entry.resolved_path), expected) {
            return Some(format!("{} 的目标已变化，请重新生成预览", entry.skill_name));
        }
        if let Some(metadata) = pending.metadata_by_entry.get(&entry.entry_id) {
            let actual_hash = storage::inspect_tree(Path::new(&metadata.prepared_path))
                .ok()
                .map(|value| value.0);
            let expected_hash = pending.source_guards.get(&entry.entry_id);
            if expected_hash.and_then(|hash| actual_hash.as_deref().map(|actual| actual == hash))
                != Some(true)
            {
                return Some(format!(
                    "{} 的来源快照已变化，请重新生成预览",
                    entry.skill_name
                ));
            }
        }
    }
    let persisted = match storage::load_state(data_dir) {
        Ok(state) => state,
        Err(error) => return Some(format!("无法读取安装状态，计划已失效: {error}")),
    };
    for entry in &pending.public.entries {
        let Some(expected_id) = pending.installation_guards.get(&entry.entry_id) else {
            return Some(format!(
                "{} 缺少安装记录校验，请重新生成预览",
                entry.skill_name
            ));
        };
        let unchanged = persisted.installations.iter().any(|installation| {
            installation.id == *expected_id && installation.resolved_path == entry.resolved_path
        });
        if !unchanged {
            return Some(format!(
                "{} 的安装记录已变化，请重新生成预览",
                entry.skill_name
            ));
        }
    }
    None
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub inspections: Mutex<HashMap<String, SourceInspection>>,
    pub plans: Mutex<HashMap<String, PendingPlan>>,
    pub update_plans: Mutex<HashMap<String, PendingUpdatePlan>>,
    pub mutation_lock: Mutex<()>,
    pub operation_progress: Mutex<Option<OperationProgress>>,
    pub cancel_requested: AtomicBool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSyncResult {
    pub source_id: String,
    pub entries: Vec<CatalogEntry>,
    pub fetched_at: String,
    pub from_cache: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSearchRequest {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub scripts_only: bool,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub updated_since: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionInstallRequest {
    pub collection_id: String,
    #[serde(default)]
    pub inspection_id: Option<String>,
    #[serde(default)]
    pub assignments: Vec<SkillAssignment>,
}

fn begin_progress(
    state: &AppState,
    operation_id: &str,
    phase: &str,
    total: usize,
    cancellable: bool,
) -> Option<String> {
    let mut progress = state.operation_progress.lock().ok()?;
    if progress.is_some() {
        return None;
    }
    let owner = format!("{operation_id}:{}", Uuid::new_v4());
    state.cancel_requested.store(false, Ordering::Release);
    *progress = Some(OperationProgress {
        operation_id: owner.clone(),
        phase: phase.to_string(),
        completed: 0,
        total,
        cancellable,
        indeterminate: total == 0,
    });
    Some(owner)
}

fn update_progress(state: &AppState, owner: &str, completed: usize) {
    if let Ok(mut progress) = state.operation_progress.lock() {
        if let Some(value) = progress
            .as_mut()
            .filter(|value| value.operation_id == owner)
        {
            value.completed = if value.total == 0 {
                completed
            } else {
                completed.min(value.total)
            };
        }
    }
}

fn set_progress_total(state: &AppState, owner: &str, total: usize) {
    if let Ok(mut progress) = state.operation_progress.lock() {
        if let Some(value) = progress
            .as_mut()
            .filter(|value| value.operation_id == owner)
        {
            value.total = total;
            value.indeterminate = total == 0;
            value.completed = value.completed.min(total);
        }
    }
}

fn finish_progress(state: &AppState, owner: &str) {
    if let Ok(mut progress) = state.operation_progress.lock() {
        if progress
            .as_ref()
            .is_some_and(|value| value.operation_id == owner)
        {
            *progress = None;
            state.cancel_requested.store(false, Ordering::Release);
        }
    }
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

fn set_journal_resulting_hash(
    persisted: &mut PersistedState,
    journal_id: &str,
    path: &str,
    resulting_hash: Option<String>,
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
        target.resulting_hash = resulting_hash;
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

fn content_hash_for_path(path: &Path) -> Option<String> {
    if path.is_dir() {
        storage::inspect_tree(path).ok().map(|value| value.0)
    } else if path.is_file() {
        storage::hash_file(path).ok()
    } else {
        None
    }
}

fn journal_is_superseded(
    journal_index: usize,
    journal: &OperationJournal,
    all: &[OperationJournal],
) -> bool {
    all.iter().skip(journal_index + 1).any(|later| {
        later.id != journal.id
            && later.targets.iter().any(|target| {
                journal
                    .targets
                    .iter()
                    .any(|current| current.path == target.path)
            })
    })
}

fn rollback_availability(
    journal_index: usize,
    journal: &OperationJournal,
    persisted: &PersistedState,
) -> String {
    let recoverable = matches!(
        journal.status,
        OperationJournalStatus::RecoveryRequired | OperationJournalStatus::Partial
    ) || journal.status == OperationJournalStatus::Completed;
    if !recoverable {
        return "unavailable".to_string();
    }
    if journal_is_superseded(journal_index, journal, &persisted.operation_journals) {
        return "superseded".to_string();
    }
    for target in journal.targets.iter().filter(|target| target.completed) {
        if let Some(expected_hash) = target.resulting_hash.as_deref() {
            if content_hash_for_path(Path::new(&target.path)).as_deref() != Some(expected_hash) {
                return "stale".to_string();
            }
        } else if !(journal.operation_type == "uninstall" && !Path::new(&target.path).exists()) {
            // A missing resulting hash is only a valid post-operation state for
            // uninstall, where the target is expected to be absent.  All other
            // operations need an explicit hash before they can be rolled back.
            return "stale".to_string();
        }
        if target.existed_before && journal.operation_type != "adopt" {
            let Some(backup_id) = target.backup_id.as_deref() else {
                return "missingBackup".to_string();
            };
            if !persisted
                .backups
                .iter()
                .any(|backup| backup.id == backup_id && Path::new(&backup.backup_path).exists())
            {
                return "missingBackup".to_string();
            }
        }
    }
    "available".to_string()
}

fn operation_journal_views(persisted: &PersistedState) -> Vec<OperationJournalView> {
    persisted
        .operation_journals
        .iter()
        .enumerate()
        .map(|(index, journal)| OperationJournalView {
            journal: journal.clone(),
            rollback_availability: rollback_availability(index, journal, persisted),
        })
        .collect()
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

fn replace_installation_record(
    persisted: &mut PersistedState,
    entry: &InstallPlanEntry,
    metadata: &SkillMetadata,
    pinned: bool,
) {
    let removed_ids = persisted
        .installations
        .iter()
        .filter(|installation| installation.resolved_path == entry.resolved_path)
        .map(|installation| installation.id.clone())
        .collect::<HashSet<_>>();
    persisted
        .installations
        .retain(|installation| installation.resolved_path != entry.resolved_path);
    persisted
        .pinned_installation_ids
        .retain(|id| !removed_ids.contains(id));
    let installation_id = Uuid::new_v4().to_string();
    persisted.installations.push(PhysicalInstallation {
        id: installation_id.clone(),
        skill_name: metadata.name.clone(),
        resolved_path: entry.resolved_path.clone(),
        source: Some(metadata.source.clone()),
        source_details: metadata.source_details.clone(),
        content_hash: metadata.content_hash.clone(),
        scope: InstallScope::Global,
        consumers: entry.consumers.clone(),
        passive_consumers: entry.passive_consumers.clone(),
        adapter_version: CURRENT_ADAPTER_VERSION,
        installed_at: Utc::now().to_rfc3339(),
        provenance: InstallationProvenance::Tool,
        legacy_project: false,
    });
    if pinned {
        persisted.pinned_installation_ids.push(installation_id);
    }
}

#[tauri::command]
pub fn scan_environment(state: State<'_, AppState>) -> Result<EnvironmentScan, String> {
    let clients = macos::scan_clients();
    let persisted = storage::load_state(&state.data_dir)?;
    let Some(owner) = begin_progress(
        &state,
        "scan-environment",
        "扫描 IDE Skill 库存",
        clients.len(),
        true,
    ) else {
        return Err("已有操作正在进行，请稍后重试".to_string());
    };
    let progress_state = &state;
    let on_progress = |completed: usize| update_progress(progress_state, &owner, completed);
    let scan = crate::inventory::build_environment_scan_with_control(
        clients,
        &persisted,
        Some(&state.cancel_requested),
        Some(&on_progress),
    );
    let cancelled = state.cancel_requested.load(Ordering::Acquire);
    finish_progress(&state, &owner);
    if cancelled {
        return Err("库存扫描已取消".to_string());
    }
    Ok(scan)
}

/// Return configured catalog providers.  The built-in public GitHub provider
/// is added lazily, so upgrading an existing installation never performs a
/// network request or changes the user's state until this page is opened.
#[tauri::command]
pub fn list_catalog_sources(state: State<'_, AppState>) -> Result<Vec<CatalogSource>, String> {
    let repository = storage::StateRepository::new(&state.data_dir);
    repository.mutate(|persisted| {
        catalog::ensure_sources(persisted);
        Ok(persisted.catalog_sources.clone())
    })
}

#[tauri::command]
pub fn save_catalog_source(
    mut source: CatalogSource,
    state: State<'_, AppState>,
) -> Result<Vec<CatalogSource>, String> {
    if source.id.trim().is_empty() || source.name.trim().is_empty() || source.url.trim().is_empty()
    {
        return Err("目录来源需要 id、名称和 URL".to_string());
    }
    let parsed = url::Url::parse(source.url.trim()).map_err(|_| "目录来源 URL 无效".to_string())?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err("目录来源必须是无凭据、无片段的 HTTPS URL".to_string());
    }
    let host = parsed.host_str().unwrap_or_default();
    if !matches!(
        host,
        "github.com" | "api.github.com" | "raw.githubusercontent.com"
    ) {
        return Err("目录来源目前只支持公开 GitHub HTTPS 地址".to_string());
    }
    source.url = parsed.to_string();
    if source.provider.trim().is_empty() {
        source.provider = if host == "api.github.com" && parsed.path().contains("/contents") {
            "github-contents".to_string()
        } else {
            "github-json".to_string()
        };
    }
    if !matches!(source.provider.as_str(), "github-json" | "github-contents") {
        return Err("目录来源 provider 不受支持".to_string());
    }
    let repository = storage::StateRepository::new(&state.data_dir);
    repository.mutate(|persisted| {
        catalog::ensure_sources(persisted);
        if let Some(existing) = persisted
            .catalog_sources
            .iter_mut()
            .find(|item| item.id == source.id)
        {
            *existing = source.clone();
        } else {
            persisted.catalog_sources.push(source.clone());
        }
        Ok(persisted.catalog_sources.clone())
    })
}

#[tauri::command]
pub fn remove_catalog_source(
    source_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<CatalogSource>, String> {
    if source_id == "anthropics-skills" {
        return Err("内置 GitHub 来源不能删除，可在设置中停用".to_string());
    }
    let repository = storage::StateRepository::new(&state.data_dir);
    let sources = repository.mutate(|persisted| {
        catalog::ensure_sources(persisted);
        persisted
            .catalog_sources
            .retain(|source| source.id != source_id);
        persisted
            .catalog_cache
            .retain(|item| item.source_id != source_id);
        Ok(persisted.catalog_sources.clone())
    })?;
    let _ = fs::remove_file(catalog::cache_path(&state.data_dir, &source_id));
    Ok(sources)
}

#[tauri::command]
pub async fn sync_catalog(
    source_id: String,
    state: State<'_, AppState>,
) -> Result<CatalogSyncResult, String> {
    let repository = storage::StateRepository::new(&state.data_dir);
    let sources = repository.mutate(|persisted| {
        catalog::ensure_sources(persisted);
        Ok(persisted.catalog_sources.clone())
    })?;
    let source = sources
        .into_iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| "目录来源不存在".to_string())?;
    let fallback = catalog::load_snapshot(&state.data_dir, &source.id)
        .ok()
        .flatten();
    match catalog::sync_source_with_control(&state.data_dir, &source, Some(&state.cancel_requested))
        .await
    {
        Ok((snapshot, from_cache)) => {
            let mut updated = source.clone();
            updated.etag = snapshot.etag.clone();
            updated.last_modified = snapshot.last_modified.clone();
            updated.last_synced_at = Some(snapshot.fetched_at.clone());
            let metadata = CatalogCacheMetadata {
                source_id: snapshot.source_id.clone(),
                cache_path: catalog::cache_path(&state.data_dir, &snapshot.source_id)
                    .display()
                    .to_string(),
                fetched_at: snapshot.fetched_at.clone(),
                etag: snapshot.etag.clone(),
                last_modified: snapshot.last_modified.clone(),
                entry_count: snapshot.entries.len(),
            };
            catalog::update_cache_metadata(&repository, metadata, updated)?;
            Ok(CatalogSyncResult {
                source_id: snapshot.source_id,
                entries: snapshot.entries,
                fetched_at: snapshot.fetched_at,
                from_cache,
                warning: None,
            })
        }
        Err(error) => {
            if let Some(snapshot) = fallback {
                Ok(CatalogSyncResult {
                    source_id: snapshot.source_id,
                    entries: snapshot.entries,
                    fetched_at: snapshot.fetched_at,
                    from_cache: true,
                    warning: Some(format!("在线同步失败，已使用最近缓存：{error}")),
                })
            } else {
                Err(error)
            }
        }
    }
}

#[tauri::command]
pub fn search_catalog(
    request: CatalogSearchRequest,
    state: State<'_, AppState>,
) -> Result<Vec<CatalogEntry>, String> {
    let mut persisted = storage::load_state(&state.data_dir)?;
    catalog::ensure_sources(&mut persisted);
    let mut entries = catalog::all_cached_entries(&state.data_dir, &persisted.catalog_sources)?;
    catalog::reconcile_entries(&mut entries, &persisted.installations);
    let query = request.query.unwrap_or_default().trim().to_lowercase();
    entries.retain(|entry| {
        let matches_query = query.is_empty()
            || entry.name.to_lowercase().contains(&query)
            || entry.description.to_lowercase().contains(&query)
            || entry
                .repository
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(&query);
        let matches_source = request
            .source_id
            .as_deref()
            .is_none_or(|source_id| source_id == entry.source_id);
        let matches_scripts = !request.scripts_only || entry.has_scripts;
        let matches_category = request
            .category
            .as_deref()
            .is_none_or(|category| entry.path.as_deref().unwrap_or_default().contains(category));
        let matches_updated = request.updated_since.as_deref().is_none_or(|since| {
            entry
                .updated_at
                .as_deref()
                .is_some_and(|updated| updated >= since)
        });
        matches_query && matches_source && matches_scripts && matches_category && matches_updated
    });
    let favorites = persisted.catalog_favorites;
    entries.sort_by_key(|entry| (!favorites.contains(&entry.id), entry.name.to_lowercase()));
    Ok(entries)
}

#[tauri::command]
pub fn set_catalog_favorite(
    entry_id: String,
    favorite: bool,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let repository = storage::StateRepository::new(&state.data_dir);
    repository.mutate(|persisted| {
        if favorite {
            if !persisted.catalog_favorites.contains(&entry_id) {
                persisted.catalog_favorites.push(entry_id.clone());
            }
        } else {
            persisted.catalog_favorites.retain(|id| id != &entry_id);
        }
        Ok(persisted.catalog_favorites.clone())
    })
}

/// Favorites are stored by stable catalog id rather than by the display name.
/// This read seam keeps the selection available after an application restart.
#[tauri::command]
pub fn list_catalog_favorites(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let repository = storage::StateRepository::new(&state.data_dir);
    repository.mutate(|persisted| Ok(persisted.catalog_favorites.clone()))
}

#[tauri::command]
pub fn list_collections(state: State<'_, AppState>) -> Result<Vec<SkillCollection>, String> {
    let repository = storage::StateRepository::new(&state.data_dir);
    repository.mutate(|persisted| Ok(persisted.skill_collections.clone()))
}

#[tauri::command]
pub fn save_collection(
    collection: SkillCollection,
    state: State<'_, AppState>,
) -> Result<Vec<SkillCollection>, String> {
    let repository = storage::StateRepository::new(&state.data_dir);
    repository.mutate(|persisted| {
        for reference in &collection.source_refs {
            catalog::validate_collection_ref(reference)?;
        }
        let existing = persisted
            .skill_collections
            .iter()
            .find(|item| item.id == collection.id);
        let normalized = catalog::collection_from_input(
            Some(collection.id.clone()),
            collection.name.clone(),
            collection.description.clone(),
            collection.skill_refs.clone(),
            collection.default_client_ids.clone(),
            existing,
        )?;
        let mut normalized = normalized;
        normalized.source_refs = collection.source_refs.clone();
        if let Some(item) = persisted
            .skill_collections
            .iter_mut()
            .find(|item| item.id == normalized.id)
        {
            *item = normalized;
        } else {
            persisted.skill_collections.push(normalized);
        }
        Ok(persisted.skill_collections.clone())
    })
}

#[tauri::command]
pub fn delete_collection(
    collection_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<SkillCollection>, String> {
    let repository = storage::StateRepository::new(&state.data_dir);
    repository.mutate(|persisted| {
        persisted
            .skill_collections
            .retain(|item| item.id != collection_id);
        Ok(persisted.skill_collections.clone())
    })
}

fn compose_collection_inspection(
    collection: &SkillCollection,
    inspected_sources: &[SourceInspection],
) -> Result<SourceInspection, String> {
    let mut skills = Vec::new();
    let mut rejected = Vec::new();
    let mut warnings = Vec::new();
    let mut source = None;
    for reference in &collection.source_refs {
        catalog::validate_collection_ref(reference)?;
        let inspected = inspected_sources
            .iter()
            .find(|inspection| inspection.source == reference.source)
            .ok_or_else(|| format!("集合成员 {} 来源检查不存在", reference.catalog_entry_id))?;
        if source.is_none() {
            source = Some(inspected.source.clone());
        }
        rejected.extend(inspected.rejected.clone());
        warnings.extend(inspected.warnings.clone());
        let candidate = inspected.skills.iter().find(|skill| {
            let name_matches = reference
                .skill_name
                .as_deref()
                .is_none_or(|name| name == skill.name);
            let expected_path = reference
                .path
                .as_deref()
                .or(reference.source_details.subpath.as_deref());
            let path_matches = expected_path.is_none_or(|path| {
                skill.relative_path == path
                    || skill
                        .source_details
                        .subpath
                        .as_deref()
                        .is_some_and(|actual| actual == path)
            });
            name_matches && path_matches
        });
        let Some(candidate) = candidate else {
            return Err(format!(
                "集合成员 {} 在固定来源中不存在或不唯一",
                reference.catalog_entry_id
            ));
        };
        if let (Some(expected), Some(actual)) = (
            reference
                .commit_sha
                .as_deref()
                .or(reference.source_details.commit_sha.as_deref()),
            candidate.source_details.commit_sha.as_deref(),
        ) {
            if expected != actual {
                return Err(format!(
                    "集合成员 {} 的 commit SHA 与检查结果不一致",
                    reference.catalog_entry_id
                ));
            }
        }
        skills.push(candidate.clone());
    }
    Ok(SourceInspection {
        inspection_id: Uuid::new_v4().to_string(),
        source: source.ok_or_else(|| "集合没有可安装的 Skill".to_string())?,
        skills,
        rejected,
        warnings,
    })
}

#[tauri::command]
pub async fn plan_collection_install(
    request: CollectionInstallRequest,
    state: State<'_, AppState>,
) -> Result<InstallPlan, String> {
    let repository = storage::StateRepository::new(&state.data_dir);
    let collection = storage::load_state(&state.data_dir)?
        .skill_collections
        .into_iter()
        .find(|item| item.id == request.collection_id)
        .ok_or_else(|| "Skill 集合不存在".to_string())?;
    // A collection stores a stable source descriptor for every catalog member.
    // Re-inspect every descriptor here and compose one inspection snapshot so
    // the normal inspect -> plan -> apply pipeline remains the only installer.
    let (inspection, assignments) = if !collection.source_refs.is_empty() {
        let mut inspected_sources = Vec::new();
        for reference in &collection.source_refs {
            catalog::validate_collection_ref(reference)?;
            let inspected = skill::inspect_source_with_control(
                reference.source.clone(),
                &state.data_dir,
                Some(&state.cancel_requested),
                None,
            )
            .await
            .map_err(|error| {
                format!(
                    "集合成员 {} 来源检查失败：{error}",
                    reference.catalog_entry_id
                )
            })?;
            inspected_sources.push(inspected);
        }
        let inspection = compose_collection_inspection(&collection, &inspected_sources)?;
        state
            .inspections
            .lock()
            .map_err(|_| "来源检查锁已损坏".to_string())?
            .insert(inspection.inspection_id.clone(), inspection.clone());
        let by_catalog_id: HashMap<_, _> = collection
            .source_refs
            .iter()
            .zip(inspection.skills.iter())
            .map(|(reference, skill)| (reference.catalog_entry_id.as_str(), skill.skill_id.clone()))
            .collect();
        let assignments = if request.assignments.is_empty() {
            inspection
                .skills
                .iter()
                .map(|skill| SkillAssignment {
                    skill_id: skill.skill_id.clone(),
                    client_ids: collection.default_client_ids.clone(),
                })
                .collect()
        } else {
            let mut assignments: Vec<SkillAssignment> = request
                .assignments
                .into_iter()
                .map(|mut assignment| {
                    if let Some(skill_id) = by_catalog_id.get(assignment.skill_id.as_str()) {
                        assignment.skill_id = skill_id.clone();
                    }
                    assignment
                })
                .collect();
            // A collection operation may customise the matrix, but it can
            // never silently drop a member.  Missing rows receive the
            // collection defaults and therefore remain in the preview.
            let assigned_ids = assignments
                .iter()
                .map(|assignment: &SkillAssignment| assignment.skill_id.clone())
                .collect::<HashSet<_>>();
            for skill in &inspection.skills {
                if !assigned_ids.contains(&skill.skill_id) {
                    assignments.push(SkillAssignment {
                        skill_id: skill.skill_id.clone(),
                        client_ids: collection.default_client_ids.clone(),
                    });
                }
            }
            assignments
        };
        (inspection, assignments)
    } else {
        let inspection_id = request
            .inspection_id
            .ok_or_else(|| "集合安装需要先检查来源并提供 inspectionId".to_string())?;
        let inspection = state
            .inspections
            .lock()
            .map_err(|_| "来源检查锁已损坏".to_string())?
            .get(&inspection_id)
            .cloned()
            .ok_or_else(|| "来源检查不存在或已过期".to_string())?;
        let assignments = if request.assignments.is_empty() {
            catalog::collection_assignments(&collection)
        } else {
            request.assignments
        };
        (inspection, assignments)
    };
    let clients = macos::scan_clients();
    let persisted = repository.load()?;
    let plan = build_install_plan(
        &inspection,
        &assignments,
        &clients,
        &home_dir()?,
        &persisted,
    )?;
    register_pending_install_plan(&state, &plan, &inspection, &persisted)?;
    Ok(plan)
}

#[tauri::command]
pub async fn inspect_source(
    source: SkillSource,
    state: State<'_, AppState>,
) -> Result<SourceInspection, String> {
    let Some(owner) = begin_progress(&state, "inspect-source", "检查来源", 0, true) else {
        return Err("已有操作正在进行，请稍后重试".to_string());
    };
    let progress_state = &state;
    let on_progress = |bytes: usize, total: Option<usize>| {
        if let Some(total) = total {
            set_progress_total(progress_state, &owner, total);
        }
        update_progress(progress_state, &owner, bytes);
    };
    let inspection = match skill::inspect_source_with_control(
        source,
        &state.data_dir,
        Some(&state.cancel_requested),
        Some(&on_progress),
    )
    .await
    {
        Ok(inspection) => inspection,
        Err(error) => {
            finish_progress(&state, &owner);
            return Err(error);
        }
    };
    if state.cancel_requested.load(Ordering::Acquire) {
        finish_progress(&state, &owner);
        return Err("来源检查已取消".to_string());
    }
    let indeterminate = state
        .operation_progress
        .lock()
        .ok()
        .and_then(|progress| {
            progress
                .as_ref()
                .filter(|value| value.operation_id == owner)
                .map(|value| value.indeterminate)
        })
        .unwrap_or(true);
    if indeterminate {
        update_progress(&state, &owner, 1);
    }
    state
        .inspections
        .lock()
        .map_err(|_| "来源检查锁已损坏".to_string())?
        .insert(inspection.inspection_id.clone(), inspection.clone());
    finish_progress(&state, &owner);
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
    let (created_at, expires_at) = plan_window();
    Ok(InstallPlan {
        plan_id: Uuid::new_v4().to_string(),
        created_at,
        expires_at,
        skills: selected,
        entries,
    })
}

fn register_pending_install_plan(
    state: &AppState,
    plan: &InstallPlan,
    inspection: &SourceInspection,
    persisted: &PersistedState,
) -> Result<(), String> {
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
    let source_guards = plan
        .skills
        .iter()
        .map(|skill| (skill.skill_id.clone(), skill.content_hash.clone()))
        .collect();
    let target_guards = plan
        .entries
        .iter()
        .map(|entry| {
            (
                entry.entry_id.clone(),
                target_guard(Path::new(&entry.resolved_path)),
            )
        })
        .collect();
    let installation_guards = plan
        .entries
        .iter()
        .map(|entry| {
            (
                entry.entry_id.clone(),
                persisted
                    .installations
                    .iter()
                    .find(|installation| installation.resolved_path == entry.resolved_path)
                    .map(|installation| installation.id.clone()),
            )
        })
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
                pinned_skill_ids: HashSet::new(),
                created_at: plan.created_at.clone(),
                expires_at: plan.expires_at.clone(),
                source_guards,
                target_guards,
                installation_guards,
            },
        );
    Ok(())
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
    register_pending_install_plan(&state, &plan, &inspection, &persisted)?;
    Ok(plan)
}

pub fn apply_install_plan_inner(
    pending: PendingPlan,
    overwrite_entry_ids: &[String],
    data_dir: &Path,
) -> Result<Vec<OperationResult>, String> {
    apply_install_plan_inner_controlled(pending, overwrite_entry_ids, data_dir, None, None)
}

fn apply_install_plan_inner_controlled(
    pending: PendingPlan,
    overwrite_entry_ids: &[String],
    data_dir: &Path,
    cancel_requested: Option<&AtomicBool>,
    on_progress: Option<&dyn Fn(usize)>,
) -> Result<Vec<OperationResult>, String> {
    if let Some(message) = validate_install_plan_guards(&pending, data_dir) {
        // A stale plan must not create a journal, backup, or state mutation.
        return Ok(stale_install_results(&pending.public, &message));
    }
    if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Ok(pending
            .public
            .entries
            .iter()
            .map(|entry| OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: false,
                status: "cancelled".to_string(),
                message: "操作已取消，未写入任何目标".to_string(),
            })
            .collect());
    }
    let mut persisted = storage::load_state(data_dir)?;
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
            resulting_hash: None,
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
        storage::save_state(data_dir, &persisted)?;
    }
    for (entry_index, entry) in pending.public.entries.iter().enumerate() {
        if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            results.push(OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: false,
                status: "cancelled".to_string(),
                message: "操作已取消，尚未处理此目标".to_string(),
            });
            if let Some(callback) = on_progress {
                callback(entry_index);
            }
            break;
        }
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
            if let Some(callback) = on_progress {
                callback(entry_index + 1);
            }
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
            if let Some(callback) = on_progress {
                callback(entry_index + 1);
            }
            continue;
        }
        if entry.conflict == ConflictState::Identical {
            replace_installation_record(
                &mut persisted,
                entry,
                metadata,
                pending.pinned_skill_ids.contains(&entry.skill_id),
            );
            storage::save_state(data_dir, &persisted)?;
            results.push(OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: true,
                status: "tracked".to_string(),
                message: "内容已相同，已纳入管理".to_string(),
            });
            if let Some(callback) = on_progress {
                callback(entry_index + 1);
            }
            continue;
        }
        let operation = (|| -> Result<(), String> {
            let backup_id = record_backup(data_dir, &mut persisted, &destination)?;
            update_journal_target(
                &mut persisted,
                &journal_id,
                &entry.resolved_path,
                backup_id,
                true,
            );
            storage::save_state(data_dir, &persisted)?;
            storage::atomic_replace(source_path, &destination)?;
            let resulting_hash = storage::inspect_tree(&destination)
                .ok()
                .map(|value| value.0);
            set_journal_resulting_hash(
                &mut persisted,
                &journal_id,
                &entry.resolved_path,
                resulting_hash,
            );
            replace_installation_record(
                &mut persisted,
                entry,
                metadata,
                pending.pinned_skill_ids.contains(&entry.skill_id),
            );
            storage::save_state(data_dir, &persisted)
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
        if let Some(callback) = on_progress {
            callback(entry_index + 1);
        }
    }
    if has_journal_targets {
        let failures = results
            .iter()
            .filter(|result| {
                !result.success && ["failed", "cancelled"].contains(&result.status.as_str())
            })
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
        storage::save_state(data_dir, &persisted)?;
    }
    Ok(results)
}

#[tauri::command]
pub fn apply_install_plan(
    plan_id: String,
    overwrite_entry_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<OperationResult>, String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    let pending = state
        .plans
        .lock()
        .map_err(|_| "安装计划锁已损坏".to_string())?
        .remove(&plan_id)
        .ok_or_else(|| "安装计划不存在或已执行".to_string())?;
    let total = pending.public.entries.len();
    let Some(owner) = begin_progress(&state, &plan_id, "写入安装目标", total, true) else {
        state
            .plans
            .lock()
            .map_err(|_| "安装计划锁已损坏".to_string())?
            .insert(plan_id.clone(), pending);
        return Err("已有操作正在进行，请稍后重试".to_string());
    };
    let progress_state = &state;
    let on_progress = |completed: usize| update_progress(progress_state, &owner, completed);
    let result = apply_install_plan_inner_controlled(
        pending,
        &overwrite_entry_ids,
        &state.data_dir,
        Some(&state.cancel_requested),
        Some(&on_progress),
    );
    finish_progress(&state, &owner);
    result
}

#[tauri::command]
pub fn list_installations(state: State<'_, AppState>) -> Result<Vec<PhysicalInstallation>, String> {
    Ok(storage::StateRepository::new(&state.data_dir)
        .load()?
        .installations)
}

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>) -> Result<Vec<BackupRecord>, String> {
    Ok(storage::load_state(&state.data_dir)?.backups)
}

#[tauri::command]
pub fn get_app_overview(state: State<'_, AppState>) -> Result<AppOverview, String> {
    let persisted = storage::load_state(&state.data_dir)?;
    let operation_journals = operation_journal_views(&persisted);
    Ok(AppOverview {
        backup_policy: persisted.backup_policy,
        operation_journals,
        pinned_installation_ids: persisted.pinned_installation_ids,
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
    let Some(owner) = begin_progress(
        &state,
        &format!("scan-client-{client_id}"),
        "扫描 IDE Skill 库存",
        0,
        true,
    ) else {
        return Err("已有操作正在进行，请稍后重试".to_string());
    };
    let progress_state = &state;
    let on_progress = |completed: usize| update_progress(progress_state, &owner, completed);
    let inventory = crate::inventory::scan_client_inventory_with_control(
        &client,
        &persisted,
        Some(&state.cancel_requested),
        Some(&on_progress),
    );
    let cancelled = state.cancel_requested.load(Ordering::Acquire);
    finish_progress(&state, &owner);
    if cancelled {
        return Err("库存扫描已取消".to_string());
    }
    Ok(inventory)
}

fn rollback_journal(
    journal_id: &str,
    data_dir: &Path,
    allow_completed: bool,
) -> Result<Vec<OperationResult>, String> {
    let mut persisted = storage::load_state(data_dir)?;
    let journal_index = persisted
        .operation_journals
        .iter()
        .position(|journal| journal.id == journal_id)
        .ok_or_else(|| "恢复记录不存在".to_string())?;
    let journal = persisted.operation_journals[journal_index].clone();
    let recoverable = matches!(
        journal.status,
        OperationJournalStatus::RecoveryRequired | OperationJournalStatus::Partial
    ) || (allow_completed && journal.status == OperationJournalStatus::Completed);
    if !recoverable {
        return Err(if allow_completed {
            "该操作不能回滚".to_string()
        } else {
            "该操作当前不需要恢复".to_string()
        });
    }
    // Rollback is destructive.  Perform every guard before touching a target,
    // creating a backup or changing the journal so stale requests are truly
    // zero-write.  A later operation on the same physical path supersedes the
    // old journal and must be handled through the newer journal instead.
    let stale_message =
        if journal_is_superseded(journal_index, &journal, &persisted.operation_journals) {
            Some("该操作已被后续相关操作覆盖，请从最新操作回滚".to_string())
        } else {
            journal
                .targets
                .iter()
                .filter(|target| target.completed)
                .find_map(|target| {
                    if let Some(expected) = target.resulting_hash.as_deref() {
                        let actual = content_hash_for_path(Path::new(&target.path));
                        if actual.as_deref() != Some(expected) {
                            return Some(format!("目标已被修改，无法安全回滚：{}", target.path));
                        }
                    } else if !(journal.operation_type == "uninstall"
                        && !Path::new(&target.path).exists())
                    {
                        return Some(format!(
                            "操作结果缺少完整校验，无法安全回滚：{}",
                            target.path
                        ));
                    }
                    if target.existed_before && journal.operation_type != "adopt" {
                        let Some(backup_id) = target.backup_id.as_deref() else {
                            return Some(format!("恢复所需备份缺失：{}", target.path));
                        };
                        let present = persisted.backups.iter().any(|backup| {
                            backup.id == backup_id && Path::new(&backup.backup_path).exists()
                        });
                        if !present {
                            return Some(format!("恢复所需备份不存在：{backup_id}"));
                        }
                    }
                    None
                })
        };
    if let Some(message) = stale_message {
        return Ok(journal
            .targets
            .iter()
            .map(|target| OperationResult {
                entry_id: None,
                skill_name: target
                    .previous_installation
                    .as_ref()
                    .map(|installation| installation.skill_name.clone()),
                path: target.path.clone(),
                success: false,
                status: "stale".to_string(),
                message: message.clone(),
            })
            .collect());
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

pub fn recover_operation_inner(
    journal_id: &str,
    data_dir: &Path,
) -> Result<Vec<OperationResult>, String> {
    rollback_journal(journal_id, data_dir, false)
}

pub fn rollback_operation_inner(
    journal_id: &str,
    data_dir: &Path,
) -> Result<Vec<OperationResult>, String> {
    rollback_journal(journal_id, data_dir, true)
}

#[tauri::command]
pub fn recover_operation(
    journal_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<OperationResult>, String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    recover_operation_inner(&journal_id, &state.data_dir)
}

#[tauri::command]
pub fn rollback_operation(
    journal_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<OperationResult>, String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    rollback_operation_inner(&journal_id, &state.data_dir)
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
    if selected
        .iter()
        .any(|installation| destination.starts_with(Path::new(&installation.resolved_path)))
    {
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

pub fn export_lockfile_inner(
    installation_ids: &[String],
    destination: &Path,
    data_dir: &Path,
) -> Result<SkillLockfile, String> {
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
    let mut entries = BTreeMap::<String, SkillLockEntry>::new();
    for installation in selected {
        let current_hash = storage::inspect_tree(Path::new(&installation.resolved_path))?.0;
        let pinned = persisted.pinned_installation_ids.contains(&installation.id);
        if let Some(existing) = entries.get_mut(&installation.skill_name) {
            if existing.content_hash != current_hash
                || existing.source != installation.source
                || existing.source_details != installation.source_details
            {
                return Err(format!(
                    "同名 Skill {} 的内容或来源不同，不能写入同一锁文件",
                    installation.skill_name
                ));
            }
            existing.consumers.extend(installation.consumers);
            existing.consumers.sort();
            existing.consumers.dedup();
            existing.pinned |= pinned;
            continue;
        }
        entries.insert(
            installation.skill_name.clone(),
            SkillLockEntry {
                skill_name: installation.skill_name,
                source: installation.source,
                source_details: installation.source_details,
                content_hash: current_hash,
                consumers: installation.consumers,
                pinned,
            },
        );
    }
    let lockfile = SkillLockfile {
        schema_version: 1,
        generated_at: Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        skills: entries.into_values().collect(),
    };
    let parent = destination
        .parent()
        .ok_or_else(|| "锁文件路径无父目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".skill-lock-{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(&lockfile).map_err(|error| error.to_string())?;
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, destination).map_err(|error| error.to_string())?;
    Ok(lockfile)
}

#[tauri::command]
pub fn export_lockfile(
    installation_ids: Vec<String>,
    destination: String,
    state: State<'_, AppState>,
) -> Result<SkillLockfile, String> {
    export_lockfile_inner(&installation_ids, Path::new(&destination), &state.data_dir)
}

fn reproducible_source(entry: &SkillLockEntry) -> Option<SkillSource> {
    match (&entry.source, entry.source_details.commit_sha.as_deref()) {
        (Some(SkillSource::Github { .. }), Some(commit))
            if entry.source_details.owner.is_some()
                && entry.source_details.repository.is_some() =>
        {
            let owner = entry.source_details.owner.as_deref().unwrap_or_default();
            let repository = entry
                .source_details
                .repository
                .as_deref()
                .unwrap_or_default();
            let subpath = entry.source_details.subpath.as_deref().unwrap_or_default();
            let suffix = if subpath.is_empty() {
                String::new()
            } else {
                format!("/{subpath}")
            };
            Some(SkillSource::Github {
                url: format!("https://github.com/{owner}/{repository}/tree/{commit}{suffix}"),
            })
        }
        (source, _) => source.clone(),
    }
}

pub async fn prepare_lockfile_import_inner(
    path: &Path,
    data_dir: &Path,
    clients: &[DetectedClient],
    home: &Path,
) -> Result<PendingLockfilePlan, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("锁文件无效: {error}"))?;
    if metadata.len() > 1024 * 1024 {
        return Err("锁文件不能超过 1 MB".to_string());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let lockfile: SkillLockfile =
        serde_json::from_slice(&bytes).map_err(|error| format!("锁文件 JSON 无效: {error}"))?;
    if lockfile.schema_version != 1 {
        return Err(format!("不支持的锁文件版本: {}", lockfile.schema_version));
    }
    if lockfile.skills.len() > storage::MAX_FILES {
        return Err("锁文件包含过多 Skill".to_string());
    }
    let persisted = storage::load_state(data_dir)?;
    let available_clients = clients
        .iter()
        .filter(|client| client.supports_skills)
        .map(|client| client.id.as_str())
        .collect::<HashSet<_>>();
    let mut missing_client_ids = Vec::new();
    let mut unavailable_skills = Vec::new();
    let mut resolved_skills = Vec::new();
    let mut assignments = Vec::new();
    let mut pinned_skill_ids = HashSet::new();
    let mut inspections = HashMap::<String, SourceInspection>::new();
    let expected_names = lockfile
        .skills
        .iter()
        .map(|entry| entry.skill_name.as_str())
        .collect::<HashSet<_>>();

    for entry in &lockfile.skills {
        let Some(source) = reproducible_source(entry) else {
            unavailable_skills.push(LockfileIssue {
                skill_name: entry.skill_name.clone(),
                reason: "来源未绑定；请改用包含内容的便携 ZIP".to_string(),
            });
            continue;
        };
        let source_key = serde_json::to_string(&source).map_err(|error| error.to_string())?;
        if !inspections.contains_key(&source_key) {
            match skill::inspect_source(source, data_dir).await {
                Ok(inspection) => {
                    inspections.insert(source_key.clone(), inspection);
                }
                Err(reason) => {
                    unavailable_skills.push(LockfileIssue {
                        skill_name: entry.skill_name.clone(),
                        reason,
                    });
                    continue;
                }
            }
        }
        let inspection = inspections.get(&source_key).expect("inspection inserted");
        let candidates = inspection
            .skills
            .iter()
            .filter(|skill| skill.name == entry.skill_name)
            .collect::<Vec<_>>();
        let selected = if let Some(expected) = entry.source_details.subpath.as_deref() {
            candidates
                .into_iter()
                .find(|skill| skill.source_details.subpath.as_deref() == Some(expected))
        } else if candidates.len() == 1 {
            candidates.first().copied()
        } else {
            None
        };
        let Some(skill) = selected else {
            unavailable_skills.push(LockfileIssue {
                skill_name: entry.skill_name.clone(),
                reason: "来源中无法唯一定位该 Skill".to_string(),
            });
            continue;
        };
        if skill.content_hash != entry.content_hash {
            unavailable_skills.push(LockfileIssue {
                skill_name: entry.skill_name.clone(),
                reason: format!(
                    "来源哈希与锁文件不一致（期望 {}，实际 {}）",
                    &entry.content_hash[..entry.content_hash.len().min(10)],
                    &skill.content_hash[..skill.content_hash.len().min(10)]
                ),
            });
            for client_id in &entry.consumers {
                if !available_clients.contains(client_id.as_str()) {
                    missing_client_ids.push(client_id.clone());
                }
            }
            continue;
        }
        let client_ids = entry
            .consumers
            .iter()
            .filter_map(|client_id| {
                if available_clients.contains(client_id.as_str()) {
                    Some(client_id.clone())
                } else {
                    missing_client_ids.push(client_id.clone());
                    None
                }
            })
            .collect::<Vec<_>>();
        if client_ids.is_empty() {
            unavailable_skills.push(LockfileIssue {
                skill_name: entry.skill_name.clone(),
                reason: "锁文件中的目标 IDE 当前均不可用".to_string(),
            });
            continue;
        }
        let metadata = (*skill).clone();
        if entry.pinned {
            pinned_skill_ids.insert(metadata.skill_id.clone());
        }
        assignments.push(SkillAssignment {
            skill_id: metadata.skill_id.clone(),
            client_ids,
        });
        resolved_skills.push(metadata);
    }
    missing_client_ids.sort();
    missing_client_ids.dedup();
    let install_plan = if resolved_skills.is_empty() {
        let (created_at, expires_at) = plan_window();
        InstallPlan {
            plan_id: Uuid::new_v4().to_string(),
            created_at,
            expires_at,
            skills: Vec::new(),
            entries: Vec::new(),
        }
    } else {
        build_install_plan(
            &SourceInspection {
                inspection_id: Uuid::new_v4().to_string(),
                source: SkillSource::LocalDirectory {
                    path: path.display().to_string(),
                },
                skills: resolved_skills,
                rejected: Vec::new(),
                warnings: Vec::new(),
            },
            &assignments,
            clients,
            home,
            &persisted,
        )?
    };
    let extra_installation_ids = persisted
        .installations
        .iter()
        .filter(|installation| {
            !installation.legacy_project
                && !expected_names.contains(installation.skill_name.as_str())
        })
        .map(|installation| installation.id.clone())
        .collect::<Vec<_>>();
    let source_paths = install_plan
        .skills
        .iter()
        .map(|skill| (skill.skill_id.clone(), PathBuf::from(&skill.prepared_path)))
        .collect();
    let (created_at, expires_at) = plan_window();
    let source_guards = install_plan
        .skills
        .iter()
        .map(|skill| (skill.skill_id.clone(), skill.content_hash.clone()))
        .collect();
    let target_guards = install_plan
        .entries
        .iter()
        .map(|entry| {
            (
                entry.entry_id.clone(),
                target_guard(Path::new(&entry.resolved_path)),
            )
        })
        .collect();
    let installation_guards = install_plan
        .entries
        .iter()
        .map(|entry| {
            (
                entry.entry_id.clone(),
                persisted
                    .installations
                    .iter()
                    .find(|installation| installation.resolved_path == entry.resolved_path)
                    .map(|installation| installation.id.clone()),
            )
        })
        .collect();
    Ok(PendingLockfilePlan {
        public: LockfileImportPlan {
            install_plan: install_plan.clone(),
            missing_client_ids,
            unavailable_skills,
            extra_installation_ids,
        },
        pending_install: PendingPlan {
            public: install_plan,
            source_paths,
            pinned_skill_ids,
            created_at,
            expires_at,
            source_guards,
            target_guards,
            installation_guards,
        },
    })
}

#[tauri::command]
pub async fn plan_lockfile_import(
    path: String,
    state: State<'_, AppState>,
) -> Result<LockfileImportPlan, String> {
    let pending = prepare_lockfile_import_inner(
        Path::new(&path),
        &state.data_dir,
        &macos::scan_clients(),
        &home_dir()?,
    )
    .await?;
    let public = pending.public.clone();
    state
        .plans
        .lock()
        .map_err(|_| "安装计划锁已损坏".to_string())?
        .insert(public.install_plan.plan_id.clone(), pending.pending_install);
    Ok(public)
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
        .any(|root| path != *root && path.starts_with(root))
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
        adapter_version: CURRENT_ADAPTER_VERSION,
        installed_at: Utc::now().to_rfc3339(),
        provenance: InstallationProvenance::Adopted,
        legacy_project: false,
    };
    persisted.installations.push(installation.clone());
    persisted.operation_journals.push(OperationJournal {
        id: Uuid::new_v4().to_string(),
        operation_type: "adopt".to_string(),
        created_at: Utc::now().to_rfc3339(),
        finished_at: Some(Utc::now().to_rfc3339()),
        status: OperationJournalStatus::Completed,
        targets: vec![OperationJournalTarget {
            path: installation.resolved_path.clone(),
            existed_before: true,
            backup_id: None,
            completed: true,
            resulting_hash: Some(installation.content_hash.clone()),
            previous_installation: None,
        }],
        message: Some("已纳入管理；回滚只移除管理记录，不删除 Skill".to_string()),
    });
    storage::save_state(data_dir, &persisted)?;
    Ok(installation)
}

#[tauri::command]
pub fn adopt_external_skill(
    client_id: String,
    resolved_path: String,
    state: State<'_, AppState>,
) -> Result<PhysicalInstallation, String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    adopt_external_skill_inner(
        &client_id,
        &resolved_path,
        &state.data_dir,
        &macos::scan_clients(),
    )
}

pub fn set_installation_pinned_inner(
    installation_id: &str,
    pinned: bool,
    data_dir: &Path,
) -> Result<(), String> {
    let mut persisted = storage::load_state(data_dir)?;
    if !persisted
        .installations
        .iter()
        .any(|installation| installation.id == installation_id)
    {
        return Err("安装记录不存在".to_string());
    }
    persisted
        .pinned_installation_ids
        .retain(|id| id != installation_id);
    if pinned {
        persisted
            .pinned_installation_ids
            .push(installation_id.to_string());
    }
    storage::StateRepository::new(data_dir).save(&persisted)
}

#[tauri::command]
pub fn set_installation_pinned(
    installation_id: String,
    pinned: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    set_installation_pinned_inner(&installation_id, pinned, &state.data_dir)
}

pub async fn prepare_update_plan_inner(
    installation_ids: &[String],
    data_dir: &Path,
) -> Result<PendingUpdatePlan, String> {
    if installation_ids.is_empty() {
        return Err("请至少选择一个受管理 Skill".to_string());
    }
    let persisted = storage::load_state(data_dir)?;
    let mut entries = Vec::new();
    let mut metadata_by_entry = HashMap::new();
    let mut seen = HashSet::new();
    for installation_id in installation_ids {
        if !seen.insert(installation_id) {
            continue;
        }
        let installation = persisted
            .installations
            .iter()
            .find(|installation| installation.id == *installation_id)
            .cloned()
            .ok_or_else(|| format!("安装记录不存在: {installation_id}"))?;
        let entry_id = Uuid::new_v4().to_string();
        let target_hash = storage::inspect_tree(Path::new(&installation.resolved_path))
            .ok()
            .map(|value| value.0);
        if persisted.pinned_installation_ids.contains(installation_id) {
            entries.push(UpdatePlanEntry {
                entry_id,
                installation_id: installation.id,
                skill_name: installation.skill_name,
                resolved_path: installation.resolved_path,
                status: UpdateState::Pinned,
                message: "已固定当前版本，不参与更新".to_string(),
                current_hash: target_hash,
                source_hash: None,
                source_revision: None,
                changes: None,
                requires_confirmation: false,
            });
            continue;
        }
        let Some(source) = installation.source.clone() else {
            entries.push(UpdatePlanEntry {
                entry_id,
                installation_id: installation.id,
                skill_name: installation.skill_name,
                resolved_path: installation.resolved_path,
                status: UpdateState::SourceUnavailable,
                message: "来源未绑定，仅管理本地副本".to_string(),
                current_hash: target_hash,
                source_hash: None,
                source_revision: None,
                changes: None,
                requires_confirmation: false,
            });
            continue;
        };
        let inspected = skill::inspect_source(source, data_dir)
            .await
            .and_then(|inspection| inspected_update(&inspection, &installation).cloned());
        match inspected {
            Ok(metadata) => {
                let target_modified =
                    target_hash.as_deref() != Some(installation.content_hash.as_str());
                let source_changed = metadata.content_hash != installation.content_hash;
                let changes = if Path::new(&installation.resolved_path).is_dir() {
                    storage::compare_trees(
                        Path::new(&installation.resolved_path),
                        Path::new(&metadata.prepared_path),
                    )
                    .ok()
                } else {
                    None
                };
                let (status, message, requires_confirmation) = if target_modified {
                    (
                        UpdateState::TargetModified,
                        "目标内容已被手工修改；更新会先备份当前内容".to_string(),
                        true,
                    )
                } else if source_changed {
                    (UpdateState::SourceChanged, "来源有新内容".to_string(), true)
                } else {
                    (UpdateState::Current, "已是最新".to_string(), false)
                };
                if requires_confirmation {
                    metadata_by_entry.insert(entry_id.clone(), metadata.clone());
                }
                entries.push(UpdatePlanEntry {
                    entry_id,
                    installation_id: installation.id,
                    skill_name: installation.skill_name,
                    resolved_path: installation.resolved_path,
                    status,
                    message,
                    current_hash: target_hash,
                    source_hash: Some(metadata.content_hash),
                    source_revision: metadata.source_details.commit_sha,
                    changes,
                    requires_confirmation,
                });
            }
            Err(message) => entries.push(UpdatePlanEntry {
                entry_id,
                installation_id: installation.id,
                skill_name: installation.skill_name,
                resolved_path: installation.resolved_path,
                status: UpdateState::SourceUnavailable,
                message,
                current_hash: target_hash,
                source_hash: None,
                source_revision: None,
                changes: None,
                requires_confirmation: false,
            }),
        }
    }
    let (created_at, expires_at) = plan_window();
    let source_guards = metadata_by_entry
        .iter()
        .map(|(entry_id, metadata)| (entry_id.clone(), metadata.content_hash.clone()))
        .collect();
    let target_guards = entries
        .iter()
        .map(|entry| {
            (
                entry.entry_id.clone(),
                target_guard(Path::new(&entry.resolved_path)),
            )
        })
        .collect();
    let installation_guards = entries
        .iter()
        .map(|entry| (entry.entry_id.clone(), entry.installation_id.clone()))
        .collect();
    Ok(PendingUpdatePlan {
        public: UpdatePlan {
            plan_id: Uuid::new_v4().to_string(),
            created_at: created_at.clone(),
            expires_at: expires_at.clone(),
            entries,
        },
        metadata_by_entry,
        created_at,
        expires_at,
        source_guards,
        target_guards,
        installation_guards,
    })
}

#[tauri::command]
pub async fn plan_updates(
    installation_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<UpdatePlan, String> {
    let pending = prepare_update_plan_inner(&installation_ids, &state.data_dir).await?;
    let plan = pending.public.clone();
    state
        .update_plans
        .lock()
        .map_err(|_| "更新计划锁已损坏".to_string())?
        .insert(plan.plan_id.clone(), pending);
    Ok(plan)
}

pub fn apply_update_plan_inner(
    pending: PendingUpdatePlan,
    approved_entry_ids: &[String],
    data_dir: &Path,
) -> Result<Vec<OperationResult>, String> {
    apply_update_plan_inner_controlled(pending, approved_entry_ids, data_dir, None, None)
}

fn apply_update_plan_inner_controlled(
    pending: PendingUpdatePlan,
    approved_entry_ids: &[String],
    data_dir: &Path,
    cancel_requested: Option<&AtomicBool>,
    on_progress: Option<&dyn Fn(usize)>,
) -> Result<Vec<OperationResult>, String> {
    if let Some(message) = validate_update_plan_guards(&pending, data_dir) {
        return Ok(stale_update_results(&pending.public, &message));
    }
    if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Ok(pending
            .public
            .entries
            .iter()
            .map(|entry| OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: false,
                status: "cancelled".to_string(),
                message: "操作已取消，未写入任何目标".to_string(),
            })
            .collect());
    }
    let mut persisted = storage::load_state(data_dir)?;
    let approved = approved_entry_ids.iter().collect::<HashSet<_>>();
    let journal_id = Uuid::new_v4().to_string();
    let journal_targets = pending
        .public
        .entries
        .iter()
        .filter(|entry| {
            entry.requires_confirmation
                && approved.contains(&entry.entry_id)
                && pending.metadata_by_entry.contains_key(&entry.entry_id)
        })
        .map(|entry| OperationJournalTarget {
            path: entry.resolved_path.clone(),
            existed_before: Path::new(&entry.resolved_path).exists(),
            backup_id: None,
            completed: false,
            resulting_hash: None,
            previous_installation: persisted
                .installations
                .iter()
                .find(|installation| installation.id == entry.installation_id)
                .cloned(),
        })
        .collect::<Vec<_>>();
    if !journal_targets.is_empty() {
        persisted.operation_journals.push(OperationJournal {
            id: journal_id.clone(),
            operation_type: "update".to_string(),
            created_at: Utc::now().to_rfc3339(),
            finished_at: None,
            status: OperationJournalStatus::Applying,
            targets: journal_targets,
            message: None,
        });
        storage::save_state(data_dir, &persisted)?;
    }
    let mut results = Vec::new();
    for (entry_index, entry) in pending.public.entries.iter().enumerate() {
        if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            results.push(OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: false,
                status: "cancelled".to_string(),
                message: "操作已取消，尚未处理此目标".to_string(),
            });
            if let Some(callback) = on_progress {
                callback(entry_index);
            }
            break;
        }
        if !entry.requires_confirmation {
            if let Some(callback) = on_progress {
                callback(entry_index + 1);
            }
            continue;
        }
        if !approved.contains(&entry.entry_id) {
            results.push(OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: false,
                status: "confirmationRequired".to_string(),
                message: "需要确认后才能更新".to_string(),
            });
            if let Some(callback) = on_progress {
                callback(entry_index + 1);
            }
            continue;
        }
        let Some(metadata) = pending.metadata_by_entry.get(&entry.entry_id) else {
            results.push(OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: false,
                status: "failed".to_string(),
                message: "更新来源已失效，请重新生成计划".to_string(),
            });
            if let Some(callback) = on_progress {
                callback(entry_index + 1);
            }
            continue;
        };
        let operation = (|| -> Result<(), String> {
            let destination = PathBuf::from(&entry.resolved_path);
            let backup_id = record_backup(data_dir, &mut persisted, &destination)?;
            update_journal_target(
                &mut persisted,
                &journal_id,
                &entry.resolved_path,
                backup_id,
                true,
            );
            storage::save_state(data_dir, &persisted)?;
            storage::atomic_replace(Path::new(&metadata.prepared_path), &destination)?;
            let resulting_hash = storage::inspect_tree(&destination)
                .ok()
                .map(|value| value.0);
            set_journal_resulting_hash(
                &mut persisted,
                &journal_id,
                &entry.resolved_path,
                resulting_hash,
            );
            let installation = persisted
                .installations
                .iter_mut()
                .find(|installation| installation.id == entry.installation_id)
                .ok_or_else(|| "安装记录不存在".to_string())?;
            installation.source = Some(metadata.source.clone());
            installation.source_details = metadata.source_details.clone();
            installation.content_hash = metadata.content_hash.clone();
            installation.adapter_version = CURRENT_ADAPTER_VERSION;
            storage::save_state(data_dir, &persisted)
        })();
        results.push(match operation {
            Ok(()) => OperationResult {
                entry_id: Some(entry.entry_id.clone()),
                skill_name: Some(entry.skill_name.clone()),
                path: entry.resolved_path.clone(),
                success: true,
                status: "updated".to_string(),
                message: "更新完成，可在操作中心回滚".to_string(),
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
        if let Some(callback) = on_progress {
            callback(entry_index + 1);
        }
    }
    if persisted
        .operation_journals
        .iter()
        .any(|journal| journal.id == journal_id)
    {
        let failures = results
            .iter()
            .filter(|result| {
                !result.success && ["failed", "cancelled"].contains(&result.status.as_str())
            })
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
        storage::save_state(data_dir, &persisted)?;
    }
    Ok(results)
}

#[tauri::command]
pub fn apply_update_plan(
    plan_id: String,
    approved_entry_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<OperationResult>, String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    let pending = state
        .update_plans
        .lock()
        .map_err(|_| "更新计划锁已损坏".to_string())?
        .remove(&plan_id)
        .ok_or_else(|| "更新计划不存在或已执行".to_string())?;
    if let Some(message) = validate_update_plan_guards(&pending, &state.data_dir) {
        // A stale plan must not create a journal, backup, or state mutation.
        return Ok(stale_update_results(&pending.public, &message));
    }
    let total = pending.public.entries.len();
    let Some(owner) = begin_progress(&state, &plan_id, "写入更新目标", total, true) else {
        state
            .update_plans
            .lock()
            .map_err(|_| "更新计划锁已损坏".to_string())?
            .insert(plan_id.clone(), pending);
        return Err("已有操作正在进行，请稍后重试".to_string());
    };
    let progress_state = &state;
    let on_progress = |completed: usize| update_progress(progress_state, &owner, completed);
    let result = apply_update_plan_inner_controlled(
        pending,
        &approved_entry_ids,
        &state.data_dir,
        Some(&state.cancel_requested),
        Some(&on_progress),
    );
    finish_progress(&state, &owner);
    result
}

#[tauri::command]
pub async fn check_updates(state: State<'_, AppState>) -> Result<Vec<UpdateStatus>, String> {
    let persisted = storage::load_state(&state.data_dir)?;
    let total = persisted.installations.len();
    let Some(owner) = begin_progress(&state, "check-updates", "检查来源更新", total, true)
    else {
        return Err("已有操作正在进行，请稍后重试".to_string());
    };
    let mut statuses = Vec::new();
    for (index, installation) in persisted.installations.into_iter().enumerate() {
        if state.cancel_requested.load(Ordering::Acquire) {
            finish_progress(&state, &owner);
            return Err("更新检查已取消".to_string());
        }
        if persisted.pinned_installation_ids.contains(&installation.id) {
            statuses.push(UpdateStatus {
                installation_id: installation.id,
                status: UpdateState::Pinned,
                message: "已固定当前版本".to_string(),
                current_hash: None,
                source_hash: None,
                source_revision: None,
                changes: None,
            });
            update_progress(&state, &owner, index + 1);
            continue;
        }
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
            update_progress(&state, &owner, index + 1);
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
            update_progress(&state, &owner, index + 1);
            continue;
        };
        match skill::inspect_source_with_control(
            source,
            &state.data_dir,
            Some(&state.cancel_requested),
            None,
        )
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
        if state.cancel_requested.load(Ordering::Acquire) {
            finish_progress(&state, &owner);
            return Err("更新检查已取消".to_string());
        }
        update_progress(&state, &owner, index + 1);
    }
    finish_progress(&state, &owner);
    Ok(statuses)
}

#[tauri::command]
pub fn get_operation_progress(
    state: State<'_, AppState>,
) -> Result<Option<OperationProgress>, String> {
    get_operation_progress_inner(&state)
}

pub fn get_operation_progress_inner(state: &AppState) -> Result<Option<OperationProgress>, String> {
    state
        .operation_progress
        .lock()
        .map(|progress| progress.clone())
        .map_err(|_| "操作进度锁已损坏".to_string())
}

#[tauri::command]
pub fn cancel_operation(
    operation_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let Some(owner) = operation_id.as_deref() else {
        // An operation token is required so a stale UI request cannot cancel
        // whichever operation happens to be active now.
        return Ok(false);
    };
    cancel_operation_for_owner_inner(&state, owner)
}

pub fn cancel_operation_inner(state: &AppState) -> Result<bool, String> {
    let owner = state
        .operation_progress
        .lock()
        .map_err(|_| "操作进度锁已损坏".to_string())?
        .as_ref()
        .map(|progress| progress.operation_id.clone());
    let Some(owner) = owner else {
        return Ok(false);
    };
    cancel_operation_for_owner_inner(state, &owner)
}

pub fn cancel_operation_for_owner_inner(state: &AppState, owner: &str) -> Result<bool, String> {
    let cancellable = state
        .operation_progress
        .lock()
        .map_err(|_| "操作进度锁已损坏".to_string())?
        .as_ref()
        .is_some_and(|progress| progress.cancellable && owner == progress.operation_id);
    if cancellable {
        state.cancel_requested.store(true, Ordering::Release);
    }
    Ok(cancellable)
}

pub fn uninstall_installation_inner(
    installation_id: &str,
    force: bool,
    data_dir: &Path,
) -> Result<OperationResult, String> {
    let mut persisted = storage::load_state(data_dir)?;
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
        let journal_id = Uuid::new_v4().to_string();
        persisted.operation_journals.push(OperationJournal {
            id: journal_id.clone(),
            operation_type: "uninstall".to_string(),
            created_at: Utc::now().to_rfc3339(),
            finished_at: None,
            status: OperationJournalStatus::Applying,
            targets: vec![OperationJournalTarget {
                path: installation.resolved_path.clone(),
                existed_before: true,
                backup_id: None,
                completed: false,
                resulting_hash: None,
                previous_installation: Some(installation.clone()),
            }],
            message: None,
        });
        storage::save_state(data_dir, &persisted)?;
        let operation = (|| -> Result<(), String> {
            let backup_id = record_backup(data_dir, &mut persisted, &path)?;
            update_journal_target(
                &mut persisted,
                &journal_id,
                &installation.resolved_path,
                backup_id,
                true,
            );
            set_journal_resulting_hash(
                &mut persisted,
                &journal_id,
                &installation.resolved_path,
                current_hash.clone(),
            );
            storage::save_state(data_dir, &persisted)?;
            fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
            // Uninstall's resulting state is an absent target.  Persist this
            // only after the remove succeeds so rollback can guard against a
            // target that was recreated or modified after the operation.
            set_journal_resulting_hash(
                &mut persisted,
                &journal_id,
                &installation.resolved_path,
                None,
            );
            storage::save_state(data_dir, &persisted)
        })();
        if let Err(message) = operation {
            finish_journal(
                &mut persisted,
                &journal_id,
                OperationJournalStatus::Partial,
                Some(message.clone()),
            );
            storage::save_state(data_dir, &persisted)?;
            return Err(message);
        }
        finish_journal(
            &mut persisted,
            &journal_id,
            OperationJournalStatus::Completed,
            None,
        );
    }
    persisted
        .installations
        .retain(|item| item.id != installation_id);
    persisted
        .pinned_installation_ids
        .retain(|id| id != installation_id);
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

#[tauri::command]
pub fn uninstall_installation(
    installation_id: String,
    force: bool,
    state: State<'_, AppState>,
) -> Result<OperationResult, String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    uninstall_installation_inner(&installation_id, force, &state.data_dir)
}

pub fn restore_backup_inner(backup_id: &str, data_dir: &Path) -> Result<OperationResult, String> {
    let mut persisted = storage::load_state(data_dir)?;
    let backup = persisted
        .backups
        .iter()
        .find(|item| item.id == backup_id)
        .cloned()
        .ok_or_else(|| "备份不存在".to_string())?;
    let destination = PathBuf::from(&backup.original_path);
    let existed_before = destination.exists();
    let journal_id = Uuid::new_v4().to_string();
    persisted.operation_journals.push(OperationJournal {
        id: journal_id.clone(),
        operation_type: "restore".to_string(),
        created_at: Utc::now().to_rfc3339(),
        finished_at: None,
        status: OperationJournalStatus::Applying,
        targets: vec![OperationJournalTarget {
            path: backup.original_path.clone(),
            existed_before,
            backup_id: None,
            completed: false,
            resulting_hash: None,
            previous_installation: existed_before
                .then(|| {
                    persisted
                        .installations
                        .iter()
                        .find(|installation| installation.resolved_path == backup.original_path)
                        .cloned()
                })
                .flatten(),
        }],
        message: None,
    });
    storage::save_state(data_dir, &persisted)?;
    let operation = (|| -> Result<(), String> {
        let previous_backup_id = storage::create_backup(data_dir, &destination)?.map(|backup| {
            let id = backup.id.clone();
            persisted.backups.push(backup);
            id
        });
        update_journal_target(
            &mut persisted,
            &journal_id,
            &backup.original_path,
            previous_backup_id,
            true,
        );
        storage::save_state(data_dir, &persisted)?;
        storage::atomic_replace(Path::new(&backup.backup_path), &destination)?;
        let resulting_hash = storage::inspect_tree(&destination)
            .ok()
            .map(|value| value.0);
        set_journal_resulting_hash(
            &mut persisted,
            &journal_id,
            &backup.original_path,
            resulting_hash,
        );
        if let Ok((hash, _, _, _)) = storage::inspect_tree(&destination) {
            if let Some(installation) = persisted
                .installations
                .iter_mut()
                .find(|installation| installation.resolved_path == backup.original_path)
            {
                installation.content_hash = hash;
                installation.adapter_version = CURRENT_ADAPTER_VERSION;
            }
        }
        storage::enforce_backup_policy(&mut persisted)?;
        Ok(())
    })();
    match operation {
        Ok(()) => finish_journal(
            &mut persisted,
            &journal_id,
            OperationJournalStatus::Completed,
            None,
        ),
        Err(message) => {
            finish_journal(
                &mut persisted,
                &journal_id,
                OperationJournalStatus::Partial,
                Some(message.clone()),
            );
            storage::save_state(data_dir, &persisted)?;
            return Err(message);
        }
    }
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

#[tauri::command]
pub fn restore_backup(
    backup_id: String,
    state: State<'_, AppState>,
) -> Result<OperationResult, String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    restore_backup_inner(&backup_id, &state.data_dir)
}

pub fn set_backup_policy_inner(policy: BackupPolicy, data_dir: &Path) -> Result<(), String> {
    if !(1..=100).contains(&policy.max_backups_per_skill) {
        return Err("每个 Skill 的备份数必须在 1 到 100 之间".to_string());
    }
    if policy.max_total_bytes < 1024 * 1024 {
        return Err("备份总空间至少为 1 MB".to_string());
    }
    if !(1..=3650).contains(&policy.retention_days) {
        return Err("保留天数必须在 1 到 3650 之间".to_string());
    }
    storage::StateRepository::new(data_dir).mutate(|persisted| {
        persisted.backup_policy = policy;
        storage::enforce_backup_policy(persisted)
    })
}

#[tauri::command]
pub fn set_backup_policy(policy: BackupPolicy, state: State<'_, AppState>) -> Result<(), String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    set_backup_policy_inner(policy, &state.data_dir)
}

pub fn delete_backup_inner(backup_id: &str, data_dir: &Path) -> Result<(), String> {
    let mut persisted = storage::load_state(data_dir)?;
    let backup = persisted
        .backups
        .iter()
        .find(|backup| backup.id == backup_id)
        .cloned()
        .ok_or_else(|| "备份不存在".to_string())?;
    let used_for_recovery = persisted.operation_journals.iter().any(|journal| {
        matches!(
            journal.status,
            OperationJournalStatus::Preparing
                | OperationJournalStatus::Applying
                | OperationJournalStatus::Partial
                | OperationJournalStatus::RecoveryRequired
        ) && journal
            .targets
            .iter()
            .any(|target| target.backup_id.as_deref() == Some(backup_id))
    });
    if used_for_recovery {
        return Err("该备份仍用于未完成操作的故障恢复，不能删除".to_string());
    }
    let backup_root = data_dir.join("backups");
    fs::create_dir_all(&backup_root).map_err(|error| error.to_string())?;
    let canonical_root = backup_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let path = PathBuf::from(&backup.backup_path);
    if path.exists() {
        let canonical = path.canonicalize().map_err(|error| error.to_string())?;
        if !canonical.starts_with(&canonical_root) || canonical.parent() != Some(&canonical_root) {
            return Err("备份路径超出应用备份目录".to_string());
        }
        if canonical.is_dir() {
            fs::remove_dir_all(&canonical).map_err(|error| error.to_string())?;
        } else {
            fs::remove_file(&canonical).map_err(|error| error.to_string())?;
        }
    }
    persisted.backups.retain(|backup| backup.id != backup_id);
    storage::save_state(data_dir, &persisted)
}

#[tauri::command]
pub fn delete_backup(backup_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let _mutation = state
        .mutation_lock
        .lock()
        .map_err(|_| "状态修改锁已损坏".to_string())?;
    delete_backup_inner(&backup_id, &state.data_dir)
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
    fn collection_composes_members_from_multiple_github_sources() {
        let source_a = SkillSource::Github {
            url: "https://github.com/acme/first/tree/main/alpha".into(),
        };
        let source_b = SkillSource::Github {
            url: "https://github.com/other/second/tree/v2/beta".into(),
        };
        let skill = |id: &str, name: &str, source: SkillSource, subpath: &str| SkillMetadata {
            skill_id: id.into(),
            relative_path: String::new(),
            name: name.into(),
            description: name.into(),
            license: None,
            compatibility: None,
            metadata: None,
            allowed_tools: None,
            source,
            source_details: SkillSourceDetails {
                owner: Some(if name == "alpha" { "acme" } else { "other" }.into()),
                repository: Some(if name == "alpha" { "first" } else { "second" }.into()),
                reference: Some(if name == "alpha" { "main" } else { "v2" }.into()),
                subpath: Some(subpath.into()),
                ..Default::default()
            },
            prepared_path: format!("/tmp/{name}"),
            content_hash: format!("{name}-hash"),
            file_count: 1,
            total_bytes: 10,
            has_scripts: false,
            warnings: Vec::new(),
        };
        let collection = SkillCollection {
            id: "collection".into(),
            name: "cross-source".into(),
            description: None,
            skill_refs: vec!["catalog-a".into(), "catalog-b".into()],
            default_client_ids: vec!["codex".into()],
            source_refs: vec![
                CollectionSkillRef {
                    catalog_entry_id: "catalog-a".into(),
                    source: source_a.clone(),
                    source_details: SkillSourceDetails {
                        owner: Some("acme".into()),
                        repository: Some("first".into()),
                        reference: Some("main".into()),
                        subpath: Some("alpha".into()),
                        ..Default::default()
                    },
                    skill_name: Some("alpha".into()),
                    path: Some("alpha".into()),
                    commit_sha: None,
                },
                CollectionSkillRef {
                    catalog_entry_id: "catalog-b".into(),
                    source: source_b.clone(),
                    source_details: SkillSourceDetails {
                        owner: Some("other".into()),
                        repository: Some("second".into()),
                        reference: Some("v2".into()),
                        subpath: Some("beta".into()),
                        ..Default::default()
                    },
                    skill_name: Some("beta".into()),
                    path: Some("beta".into()),
                    commit_sha: None,
                },
            ],
            created_at: String::new(),
            updated_at: String::new(),
        };
        let first = SourceInspection {
            inspection_id: "a".into(),
            source: source_a.clone(),
            skills: vec![skill("a", "alpha", source_a, "alpha")],
            rejected: Vec::new(),
            warnings: Vec::new(),
        };
        let second = SourceInspection {
            inspection_id: "b".into(),
            source: source_b.clone(),
            skills: vec![skill("b", "beta", source_b, "beta")],
            rejected: Vec::new(),
            warnings: Vec::new(),
        };
        let combined = compose_collection_inspection(&collection, &[first, second]).unwrap();
        assert_eq!(
            combined
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
    }

    #[test]
    fn matrix_assignments_create_entries_per_skill_and_client() {
        let skill = |id: &str, name: &str| SkillMetadata {
            skill_id: id.to_string(),
            relative_path: name.to_string(),
            name: name.to_string(),
            description: name.to_string(),
            license: None,
            compatibility: None,
            metadata: None,
            allowed_tools: None,
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
            license: None,
            compatibility: None,
            metadata: None,
            allowed_tools: None,
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
        let resulting_hash = storage::inspect_tree(&target).unwrap().0;
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
                resulting_hash: Some(resulting_hash),
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

    fn tracked_local_installation(
        data_dir: &Path,
        source: &Path,
        target: &Path,
    ) -> PhysicalInstallation {
        let content_hash = storage::inspect_tree(target).unwrap().0;
        let installation = PhysicalInstallation {
            id: "demo-id".to_string(),
            skill_name: "demo".to_string(),
            resolved_path: target.display().to_string(),
            source: Some(SkillSource::LocalDirectory {
                path: source.display().to_string(),
            }),
            source_details: SkillSourceDetails::default(),
            content_hash,
            scope: InstallScope::Global,
            consumers: vec!["codex".to_string()],
            passive_consumers: Vec::new(),
            adapter_version: 1,
            installed_at: Utc::now().to_rfc3339(),
            provenance: InstallationProvenance::Tool,
            legacy_project: false,
        };
        let mut persisted = PersistedState::default();
        persisted.installations.push(installation.clone());
        storage::save_state(data_dir, &persisted).unwrap();
        installation
    }

    #[test]
    fn update_plan_applies_source_changes_and_can_roll_back() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: New\n---\nnew\n",
        )
        .unwrap();
        fs::write(
            target.join("SKILL.md"),
            "---\nname: demo\ndescription: Old\n---\nold\n",
        )
        .unwrap();
        let previous = tracked_local_installation(data.path(), &source, &target);

        let pending = tauri::async_runtime::block_on(prepare_update_plan_inner(
            std::slice::from_ref(&previous.id),
            data.path(),
        ))
        .unwrap();

        assert_eq!(pending.public.entries.len(), 1);
        let entry = &pending.public.entries[0];
        assert_eq!(entry.status, UpdateState::SourceChanged);
        assert!(entry.requires_confirmation);
        assert_eq!(entry.changes.as_ref().unwrap().modified, vec!["SKILL.md"]);

        let results = apply_update_plan_inner(
            pending.clone(),
            std::slice::from_ref(&entry.entry_id),
            data.path(),
        )
        .unwrap();

        assert!(results[0].success);
        assert!(fs::read_to_string(target.join("SKILL.md"))
            .unwrap()
            .contains("New"));
        let updated_state = storage::load_state(data.path()).unwrap();
        let journal = updated_state.operation_journals.last().unwrap();
        assert_eq!(journal.operation_type, "update");
        assert_eq!(journal.status, OperationJournalStatus::Completed);
        assert_eq!(
            journal.targets[0].resulting_hash.as_deref(),
            Some(updated_state.installations[0].content_hash.as_str())
        );
        assert_eq!(updated_state.backups.len(), 1);
        assert_ne!(
            updated_state.installations[0].content_hash,
            previous.content_hash
        );
        assert_eq!(
            updated_state.installations[0].adapter_version,
            CURRENT_ADAPTER_VERSION
        );

        let rollback = rollback_operation_inner(&journal.id, data.path()).unwrap();

        assert!(rollback[0].success);
        assert!(fs::read_to_string(target.join("SKILL.md"))
            .unwrap()
            .contains("Old"));
        let rolled_back = storage::load_state(data.path()).unwrap();
        assert_eq!(
            rolled_back.installations[0].content_hash,
            previous.content_hash
        );
        assert_eq!(
            rolled_back.operation_journals[0].status,
            OperationJournalStatus::RolledBack
        );
    }

    #[test]
    fn rollback_rejects_manual_target_edits_without_writing_state_or_backup() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: New\n---\nnew\n",
        )
        .unwrap();
        fs::write(
            target.join("SKILL.md"),
            "---\nname: demo\ndescription: Old\n---\nold\n",
        )
        .unwrap();
        let previous = tracked_local_installation(data.path(), &source, &target);
        let pending = tauri::async_runtime::block_on(prepare_update_plan_inner(
            std::slice::from_ref(&previous.id),
            data.path(),
        ))
        .unwrap();
        let entry_id = pending.public.entries[0].entry_id.clone();
        apply_update_plan_inner(pending, std::slice::from_ref(&entry_id), data.path()).unwrap();
        fs::write(target.join("SKILL.md"), "manual edit").unwrap();
        let before = fs::read(data.path().join("state.json")).unwrap();
        let result = rollback_operation_inner(
            &storage::load_state(data.path())
                .unwrap()
                .operation_journals
                .last()
                .unwrap()
                .id,
            data.path(),
        )
        .unwrap();
        assert!(result.iter().all(|item| item.status == "stale"));
        assert_eq!(fs::read(data.path().join("state.json")).unwrap(), before);
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "manual edit"
        );
    }

    #[test]
    fn rollback_rejects_rebuilt_target_with_different_hash_without_writes() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: New\n---\nnew\n",
        )
        .unwrap();
        fs::write(
            target.join("SKILL.md"),
            "---\nname: demo\ndescription: Old\n---\nold\n",
        )
        .unwrap();
        let previous = tracked_local_installation(data.path(), &source, &target);
        let pending = tauri::async_runtime::block_on(prepare_update_plan_inner(
            std::slice::from_ref(&previous.id),
            data.path(),
        ))
        .unwrap();
        let entry_id = pending.public.entries[0].entry_id.clone();
        apply_update_plan_inner(pending, std::slice::from_ref(&entry_id), data.path()).unwrap();
        let journal_id = storage::load_state(data.path())
            .unwrap()
            .operation_journals
            .last()
            .unwrap()
            .id
            .clone();
        fs::remove_dir_all(&target).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "rebuilt by user").unwrap();
        let before = fs::read(data.path().join("state.json")).unwrap();
        let result = rollback_operation_inner(&journal_id, data.path()).unwrap();
        assert!(result.iter().all(|item| item.status == "stale"));
        assert_eq!(fs::read(data.path().join("state.json")).unwrap(), before);
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "rebuilt by user"
        );
    }

    #[test]
    fn rollback_rejects_journal_superseded_by_a_later_operation_without_writes() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: New\n---\nnew\n",
        )
        .unwrap();
        fs::write(
            target.join("SKILL.md"),
            "---\nname: demo\ndescription: Old\n---\nold\n",
        )
        .unwrap();
        let previous = tracked_local_installation(data.path(), &source, &target);
        let pending = tauri::async_runtime::block_on(prepare_update_plan_inner(
            std::slice::from_ref(&previous.id),
            data.path(),
        ))
        .unwrap();
        let entry_id = pending.public.entries[0].entry_id.clone();
        apply_update_plan_inner(pending, std::slice::from_ref(&entry_id), data.path()).unwrap();
        let mut state = storage::load_state(data.path()).unwrap();
        let old_id = state.operation_journals.last().unwrap().id.clone();
        let current_hash = storage::inspect_tree(&target).unwrap().0;
        state.operation_journals.push(OperationJournal {
            id: "newer-operation".to_string(),
            operation_type: "restore".to_string(),
            created_at: Utc::now().to_rfc3339(),
            finished_at: Some(Utc::now().to_rfc3339()),
            status: OperationJournalStatus::Completed,
            targets: vec![OperationJournalTarget {
                path: target.display().to_string(),
                existed_before: true,
                backup_id: state.backups.first().map(|backup| backup.id.clone()),
                completed: true,
                resulting_hash: Some(current_hash),
                previous_installation: None,
            }],
            message: None,
        });
        storage::save_state(data.path(), &state).unwrap();
        let before = fs::read(data.path().join("state.json")).unwrap();
        let result = rollback_operation_inner(&old_id, data.path()).unwrap();
        assert!(result.iter().all(|item| item.status == "stale"));
        assert_eq!(fs::read(data.path().join("state.json")).unwrap(), before);
        assert!(target.join("SKILL.md").is_file());
    }

    #[test]
    fn rollback_rejects_adopted_skill_edits_without_writing_state() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("external");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: external\ndescription: External\n---\noriginal\n",
        )
        .unwrap();
        let client = DetectedClient {
            id: "kiro".to_string(),
            name: "Kiro".to_string(),
            edition: ClientEdition::Standard,
            version: Some("1.0.0".to_string()),
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
        let journal_id = storage::load_state(data.path())
            .unwrap()
            .operation_journals
            .last()
            .unwrap()
            .id
            .clone();
        fs::write(skill.join("SKILL.md"), "manual adopt edit\n").unwrap();
        let before = fs::read(data.path().join("state.json")).unwrap();

        let result = rollback_operation_inner(&journal_id, data.path()).unwrap();

        assert!(result.iter().all(|item| item.status == "stale"));
        assert_eq!(fs::read(data.path().join("state.json")).unwrap(), before);
        assert_eq!(
            fs::read_to_string(skill.join("SKILL.md")).unwrap(),
            "manual adopt edit\n"
        );
        assert!(storage::load_state(data.path())
            .unwrap()
            .installations
            .iter()
            .any(|item| item.id == adopted.id));
    }

    #[test]
    fn rollback_rejects_rebuilt_uninstall_target_without_writing_backup() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nsource\n",
        )
        .unwrap();
        fs::write(
            target.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\ninstalled\n",
        )
        .unwrap();
        let installation = tracked_local_installation(data.path(), &source, &target);
        uninstall_installation_inner(&installation.id, false, data.path()).unwrap();
        let state = storage::load_state(data.path()).unwrap();
        let journal = state.operation_journals.last().unwrap().clone();
        let backup = state.backups.last().unwrap().clone();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "rebuilt after uninstall\n").unwrap();
        let before_state = fs::read(data.path().join("state.json")).unwrap();
        let before_backup = storage::inspect_tree(Path::new(&backup.backup_path))
            .unwrap()
            .0;

        let result = rollback_operation_inner(&journal.id, data.path()).unwrap();

        assert!(result.iter().all(|item| item.status == "stale"));
        assert_eq!(
            fs::read(data.path().join("state.json")).unwrap(),
            before_state
        );
        assert_eq!(
            storage::inspect_tree(Path::new(&backup.backup_path))
                .unwrap()
                .0,
            before_backup
        );
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "rebuilt after uninstall\n"
        );
    }

    #[test]
    fn rollback_rejects_restore_target_edits_without_writing_backup() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "before restore\n").unwrap();
        let backup = storage::create_backup(data.path(), &target)
            .unwrap()
            .unwrap();
        let mut state = PersistedState::default();
        state.backups.push(backup.clone());
        storage::save_state(data.path(), &state).unwrap();
        fs::write(target.join("SKILL.md"), "intermediate\n").unwrap();
        restore_backup_inner(&backup.id, data.path()).unwrap();
        let state = storage::load_state(data.path()).unwrap();
        let journal = state.operation_journals.last().unwrap().clone();
        let backup_hashes = state
            .backups
            .iter()
            .map(|item| {
                (
                    item.id.clone(),
                    storage::inspect_tree(Path::new(&item.backup_path))
                        .unwrap()
                        .0,
                )
            })
            .collect::<HashMap<_, _>>();
        fs::write(target.join("SKILL.md"), "manual restore edit\n").unwrap();
        let before_state = fs::read(data.path().join("state.json")).unwrap();

        let result = rollback_operation_inner(&journal.id, data.path()).unwrap();

        assert!(result.iter().all(|item| item.status == "stale"));
        assert_eq!(
            fs::read(data.path().join("state.json")).unwrap(),
            before_state
        );
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "manual restore edit\n"
        );
        for item in storage::load_state(data.path()).unwrap().backups {
            assert_eq!(
                storage::inspect_tree(Path::new(&item.backup_path))
                    .unwrap()
                    .0,
                backup_hashes[&item.id]
            );
        }
    }

    #[test]
    fn update_plan_rejects_target_changes_without_creating_a_journal() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: New\n---\nnew\n",
        )
        .unwrap();
        fs::write(
            target.join("SKILL.md"),
            "---\nname: demo\ndescription: Old\n---\nold\n",
        )
        .unwrap();
        let previous = tracked_local_installation(data.path(), &source, &target);
        let pending = tauri::async_runtime::block_on(prepare_update_plan_inner(
            std::slice::from_ref(&previous.id),
            data.path(),
        ))
        .unwrap();
        let entry_id = pending.public.entries[0].entry_id.clone();
        fs::write(target.join("SKILL.md"), "manual edit\n").unwrap();

        let results = apply_update_plan_inner(pending, &[entry_id], data.path()).unwrap();

        assert_eq!(results[0].status, "stale");
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "manual edit\n"
        );
        let state = storage::load_state(data.path()).unwrap();
        assert_eq!(state.operation_journals.len(), 0);
    }

    #[test]
    fn update_plan_rejects_installation_record_changes_without_writing() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: New\n---\nnew\n",
        )
        .unwrap();
        fs::write(
            target.join("SKILL.md"),
            "---\nname: demo\ndescription: Old\n---\nold\n",
        )
        .unwrap();
        let previous = tracked_local_installation(data.path(), &source, &target);
        let pending = tauri::async_runtime::block_on(prepare_update_plan_inner(
            std::slice::from_ref(&previous.id),
            data.path(),
        ))
        .unwrap();
        let entry_id = pending.public.entries[0].entry_id.clone();

        let mut changed_state = storage::load_state(data.path()).unwrap();
        changed_state.installations.clear();
        storage::save_state(data.path(), &changed_state).unwrap();

        let results = apply_update_plan_inner(pending, &[entry_id], data.path()).unwrap();

        assert_eq!(results[0].status, "stale");
        assert!(fs::read_to_string(target.join("SKILL.md"))
            .unwrap()
            .contains("Old"));
        let state = storage::load_state(data.path()).unwrap();
        assert!(state.installations.is_empty());
        assert!(state.operation_journals.is_empty());
        assert!(state.backups.is_empty());
    }

    #[test]
    fn install_plan_rejects_source_changes_without_writing() {
        let data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("source/demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\noriginal\n",
        )
        .unwrap();
        let client = DetectedClient {
            id: "kiro".to_string(),
            name: "Kiro".to_string(),
            edition: ClientEdition::Standard,
            version: Some("1.0.0".to_string()),
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: home.path().join(".kiro/skills").display().to_string(),
            inventory_skills_paths: Vec::new(),
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };
        let inspection = tauri::async_runtime::block_on(skill::inspect_source(
            SkillSource::LocalDirectory {
                path: source.display().to_string(),
            },
            data.path(),
        ))
        .unwrap();
        let plan = build_install_plan(
            &inspection,
            &[SkillAssignment {
                skill_id: inspection.skills[0].skill_id.clone(),
                client_ids: vec![client.id.clone()],
            }],
            std::slice::from_ref(&client),
            home.path(),
            &PersistedState::default(),
        )
        .unwrap();
        let pending = PendingPlan {
            source_paths: inspection
                .skills
                .iter()
                .map(|skill| (skill.skill_id.clone(), PathBuf::from(&skill.prepared_path)))
                .collect(),
            source_guards: inspection
                .skills
                .iter()
                .map(|skill| (skill.skill_id.clone(), skill.content_hash.clone()))
                .collect(),
            target_guards: plan
                .entries
                .iter()
                .map(|entry| {
                    (
                        entry.entry_id.clone(),
                        target_guard(Path::new(&entry.resolved_path)),
                    )
                })
                .collect(),
            installation_guards: plan
                .entries
                .iter()
                .map(|entry| (entry.entry_id.clone(), None))
                .collect(),
            pinned_skill_ids: HashSet::new(),
            created_at: Utc::now().to_rfc3339(),
            expires_at: (Utc::now() + PLAN_TTL).to_rfc3339(),
            public: plan,
        };
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nchanged\n",
        )
        .unwrap();

        let results = apply_install_plan_inner(pending, &[], data.path()).unwrap();

        assert_eq!(results[0].status, "installed");
        // The source was snapshotted during inspection, so changing the user
        // directory does not invalidate the plan or alter the copied content.
        assert!(results[0].success);
        let installed = home.path().join(".kiro/skills/demo/SKILL.md");
        assert!(installed.is_file());
        assert!(fs::read_to_string(installed).unwrap().contains("original"));
    }

    #[test]
    fn install_plan_rejects_tampered_preview_snapshot_without_writing() {
        let data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("source/demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\noriginal\n",
        )
        .unwrap();
        let inspection = tauri::async_runtime::block_on(skill::inspect_source(
            SkillSource::LocalDirectory {
                path: source.display().to_string(),
            },
            data.path(),
        ))
        .unwrap();
        let client = DetectedClient {
            id: "kiro".to_string(),
            name: "Kiro".to_string(),
            edition: ClientEdition::Standard,
            version: Some("1.0.0".to_string()),
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: home.path().join(".kiro/skills").display().to_string(),
            inventory_skills_paths: Vec::new(),
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };
        let plan = build_install_plan(
            &inspection,
            &[SkillAssignment {
                skill_id: inspection.skills[0].skill_id.clone(),
                client_ids: vec![client.id.clone()],
            }],
            std::slice::from_ref(&client),
            home.path(),
            &PersistedState::default(),
        )
        .unwrap();
        let snapshot = PathBuf::from(&inspection.skills[0].prepared_path);
        let pending = PendingPlan {
            source_paths: inspection
                .skills
                .iter()
                .map(|skill| (skill.skill_id.clone(), PathBuf::from(&skill.prepared_path)))
                .collect(),
            source_guards: inspection
                .skills
                .iter()
                .map(|skill| (skill.skill_id.clone(), skill.content_hash.clone()))
                .collect(),
            target_guards: plan
                .entries
                .iter()
                .map(|entry| {
                    (
                        entry.entry_id.clone(),
                        target_guard(Path::new(&entry.resolved_path)),
                    )
                })
                .collect(),
            installation_guards: plan
                .entries
                .iter()
                .map(|entry| (entry.entry_id.clone(), None))
                .collect(),
            pinned_skill_ids: HashSet::new(),
            created_at: Utc::now().to_rfc3339(),
            expires_at: (Utc::now() + PLAN_TTL).to_rfc3339(),
            public: plan,
        };
        fs::write(
            snapshot.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\ntampered\n",
        )
        .unwrap();

        let results = apply_install_plan_inner(pending, &[], data.path()).unwrap();

        assert_eq!(results[0].status, "stale");
        assert!(!home.path().join(".kiro/skills/demo").exists());
        let state = storage::load_state(data.path()).unwrap();
        assert!(state.installations.is_empty());
        assert!(state.operation_journals.is_empty());
    }

    #[test]
    fn pinned_installation_is_excluded_from_updates() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: New\n---\nnew\n",
        )
        .unwrap();
        fs::write(
            target.join("SKILL.md"),
            "---\nname: demo\ndescription: Old\n---\nold\n",
        )
        .unwrap();
        let installation = tracked_local_installation(data.path(), &source, &target);

        set_installation_pinned_inner(&installation.id, true, data.path()).unwrap();
        let pending = tauri::async_runtime::block_on(prepare_update_plan_inner(
            &[installation.id],
            data.path(),
        ))
        .unwrap();

        assert_eq!(pending.public.entries[0].status, UpdateState::Pinned);
        assert!(pending.metadata_by_entry.is_empty());
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

    #[test]
    fn lockfile_round_trip_preserves_source_targets_hash_and_pin() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        for root in [&source, &target] {
            fs::write(
                root.join("SKILL.md"),
                "---\nname: demo\ndescription: Demo\n---\n",
            )
            .unwrap();
        }
        let installation = tracked_local_installation(data.path(), &source, &target);
        set_installation_pinned_inner(&installation.id, true, data.path()).unwrap();
        let destination = data.path().join("skills.lock.json");

        let exported = export_lockfile_inner(
            std::slice::from_ref(&installation.id),
            &destination,
            data.path(),
        )
        .unwrap();

        assert_eq!(exported.schema_version, 1);
        assert_eq!(exported.skills[0].content_hash, installation.content_hash);
        assert_eq!(exported.skills[0].consumers, vec!["codex"]);
        assert!(exported.skills[0].pinned);
        assert_eq!(
            exported.skills[0].source,
            Some(SkillSource::LocalDirectory {
                path: source.display().to_string()
            })
        );

        let import_data = tempfile::tempdir().unwrap();
        let import_home = tempfile::tempdir().unwrap();
        let client = DetectedClient {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            edition: ClientEdition::Standard,
            version: Some("1.0.0".to_string()),
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: import_home
                .path()
                .join(".agents/skills")
                .display()
                .to_string(),
            inventory_skills_paths: Vec::new(),
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };

        let pending = tauri::async_runtime::block_on(prepare_lockfile_import_inner(
            &destination,
            import_data.path(),
            &[client],
            import_home.path(),
        ))
        .unwrap();

        assert!(pending.public.unavailable_skills.is_empty());
        assert!(pending.public.missing_client_ids.is_empty());
        assert_eq!(pending.public.install_plan.entries.len(), 1);
        assert!(pending
            .pending_install
            .pinned_skill_ids
            .contains(&pending.public.install_plan.skills[0].skill_id));
    }

    #[test]
    fn lockfile_import_reports_changed_sources_instead_of_installing_them() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Changed\n---\n",
        )
        .unwrap();
        let lockfile = SkillLockfile {
            schema_version: 1,
            generated_at: Utc::now().to_rfc3339(),
            app_version: "0.4.0".to_string(),
            skills: vec![SkillLockEntry {
                skill_name: "demo".to_string(),
                source: Some(SkillSource::LocalDirectory {
                    path: source.display().to_string(),
                }),
                source_details: SkillSourceDetails::default(),
                content_hash: "stale-hash".to_string(),
                consumers: vec!["missing-client".to_string()],
                pinned: false,
            }],
        };
        let path = data.path().join("skills.lock.json");
        fs::write(&path, serde_json::to_vec_pretty(&lockfile).unwrap()).unwrap();

        let pending = tauri::async_runtime::block_on(prepare_lockfile_import_inner(
            &path,
            data.path(),
            &[],
            workspace.path(),
        ))
        .unwrap();

        assert!(pending.public.install_plan.entries.is_empty());
        assert_eq!(pending.public.missing_client_ids, vec!["missing-client"]);
        assert_eq!(pending.public.unavailable_skills.len(), 1);
        assert!(pending.public.unavailable_skills[0].reason.contains("哈希"));
    }

    #[test]
    fn uninstall_is_journaled_and_can_restore_the_previous_installation() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source/demo");
        let target = workspace.path().join("installed/demo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        for root in [&source, &target] {
            fs::write(
                root.join("SKILL.md"),
                "---\nname: demo\ndescription: Demo\n---\n",
            )
            .unwrap();
        }
        let installation = tracked_local_installation(data.path(), &source, &target);

        let removed = uninstall_installation_inner(&installation.id, false, data.path()).unwrap();

        assert!(removed.success);
        assert!(!target.exists());
        let state = storage::load_state(data.path()).unwrap();
        let journal = state.operation_journals.last().unwrap();
        assert_eq!(journal.operation_type, "uninstall");
        assert_eq!(journal.status, OperationJournalStatus::Completed);
        assert!(state.installations.is_empty());

        let rollback = rollback_operation_inner(&journal.id, data.path()).unwrap();

        assert!(rollback[0].success);
        assert!(target.join("SKILL.md").is_file());
        assert_eq!(
            storage::load_state(data.path())
                .unwrap()
                .installations
                .len(),
            1
        );
    }

    #[test]
    fn active_recovery_backups_cannot_be_deleted() {
        let data = tempfile::tempdir().unwrap();
        let target = data.path().join("installed/demo");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "demo").unwrap();
        let backup = storage::create_backup(data.path(), &target)
            .unwrap()
            .unwrap();
        let mut persisted = PersistedState::default();
        persisted.backups.push(backup.clone());
        persisted.operation_journals.push(OperationJournal {
            id: "recovery".to_string(),
            operation_type: "update".to_string(),
            created_at: Utc::now().to_rfc3339(),
            finished_at: None,
            status: OperationJournalStatus::RecoveryRequired,
            targets: vec![OperationJournalTarget {
                path: target.display().to_string(),
                existed_before: true,
                backup_id: Some(backup.id.clone()),
                completed: true,
                resulting_hash: None,
                previous_installation: None,
            }],
            message: None,
        });
        storage::save_state(data.path(), &persisted).unwrap();

        let error = delete_backup_inner(&backup.id, data.path()).unwrap_err();

        assert!(error.contains("恢复"));
        assert!(Path::new(&backup.backup_path).exists());
    }

    #[test]
    fn backup_policy_updates_are_validated_and_persisted() {
        let data = tempfile::tempdir().unwrap();
        let policy = BackupPolicy {
            max_backups_per_skill: 8,
            max_total_bytes: 512 * 1024 * 1024,
            retention_days: 120,
        };

        set_backup_policy_inner(policy.clone(), data.path()).unwrap();

        assert_eq!(
            storage::load_state(data.path())
                .unwrap()
                .backup_policy
                .max_backups_per_skill,
            8
        );
        let invalid = BackupPolicy {
            max_backups_per_skill: 0,
            ..policy
        };
        assert!(set_backup_policy_inner(invalid, data.path()).is_err());
    }

    #[test]
    fn cancellable_progress_is_visible_and_finishes_without_leaking_state() {
        let data = tempfile::tempdir().unwrap();
        let state = AppState {
            data_dir: data.path().to_path_buf(),
            inspections: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            update_plans: Mutex::new(HashMap::new()),
            mutation_lock: Mutex::new(()),
            operation_progress: Mutex::new(None),
            cancel_requested: AtomicBool::new(false),
        };

        let owner = begin_progress(&state, "check-updates", "检查来源更新", 3, true).unwrap();
        let snapshot = state.operation_progress.lock().unwrap().clone().unwrap();
        assert_eq!(snapshot.operation_id, owner);
        assert_eq!(snapshot.total, 3);
        assert!(snapshot.cancellable);
        state.cancel_requested.store(true, Ordering::Release);
        assert!(state.cancel_requested.load(Ordering::Acquire));
        update_progress(&state, &owner, 4);
        assert_eq!(
            state
                .operation_progress
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .completed,
            3
        );
        assert!(cancel_operation_inner(&state).unwrap());
        assert!(state.cancel_requested.load(Ordering::Acquire));
        finish_progress(&state, &owner);
        assert!(state.operation_progress.lock().unwrap().is_none());
        assert!(!state.cancel_requested.load(Ordering::Acquire));
    }

    #[test]
    fn non_cancellable_progress_does_not_accept_cancel_requests() {
        let data = tempfile::tempdir().unwrap();
        let state = AppState {
            data_dir: data.path().to_path_buf(),
            inspections: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            update_plans: Mutex::new(HashMap::new()),
            mutation_lock: Mutex::new(()),
            operation_progress: Mutex::new(None),
            cancel_requested: AtomicBool::new(false),
        };

        let owner = begin_progress(&state, "apply", "写入安装目标", 1, false).unwrap();
        assert!(!cancel_operation_inner(&state).unwrap());
        assert!(!state.cancel_requested.load(Ordering::Acquire));
        finish_progress(&state, &owner);
    }

    #[test]
    fn background_scan_blocks_update_and_apply_while_preserving_cancellation() {
        let data = tempfile::tempdir().unwrap();
        let state = std::sync::Arc::new(AppState {
            data_dir: data.path().to_path_buf(),
            inspections: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            update_plans: Mutex::new(HashMap::new()),
            mutation_lock: Mutex::new(()),
            operation_progress: Mutex::new(None),
            cancel_requested: AtomicBool::new(false),
        });

        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let scan_state = std::sync::Arc::clone(&state);
        let scan_thread = std::thread::spawn(move || {
            let owner = begin_progress(&scan_state, "scan", "扫描", 2, true).unwrap();
            entered_tx.send(owner.clone()).unwrap();
            release_rx.recv().unwrap();
            finish_progress(&scan_state, &owner);
        });
        let scan_owner = entered_rx.recv().unwrap();
        assert!(begin_progress(&state, "update", "更新", 1, true).is_none());
        assert!(begin_progress(&state, "apply", "安装", 1, true).is_none());
        update_progress(&state, "not-the-owner", 2);
        assert_eq!(
            state
                .operation_progress
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .completed,
            0
        );
        assert!(!cancel_operation_for_owner_inner(&state, "not-the-owner").unwrap());
        assert!(!state.cancel_requested.load(Ordering::Acquire));
        assert!(cancel_operation_for_owner_inner(&state, &scan_owner).unwrap());
        assert!(state.cancel_requested.load(Ordering::Acquire));

        // A stale owner cannot finish or clear the active operation.  This is
        // the same guard used when a background scan races a user-triggered
        // update/apply request.
        finish_progress(&state, "not-the-owner");
        assert!(state.operation_progress.lock().unwrap().is_some());
        release_tx.send(()).unwrap();
        scan_thread.join().unwrap();
        assert!(state.operation_progress.lock().unwrap().is_none());
        assert!(!state.cancel_requested.load(Ordering::Acquire));
    }

    #[test]
    fn apply_install_reports_progress_and_honors_mid_operation_cancellation() {
        let data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let source_root = home.path().join("source");
        for name in ["first", "second"] {
            let skill = source_root.join(name);
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
            version: Some("1.0.0".to_string()),
            status: DetectionStatus::Installed,
            application_path: None,
            cli_path: None,
            global_skills_path: home.path().join(".kiro/skills").display().to_string(),
            inventory_skills_paths: Vec::new(),
            detection_evidence: Vec::new(),
            supports_skills: true,
            notes: Vec::new(),
        };
        let inspection = tauri::async_runtime::block_on(skill::inspect_source(
            SkillSource::LocalDirectory {
                path: source_root.display().to_string(),
            },
            data.path(),
        ))
        .unwrap();
        let assignments = inspection
            .skills
            .iter()
            .map(|skill| SkillAssignment {
                skill_id: skill.skill_id.clone(),
                client_ids: vec![client.id.clone()],
            })
            .collect::<Vec<_>>();
        let plan = build_install_plan(
            &inspection,
            &assignments,
            std::slice::from_ref(&client),
            home.path(),
            &PersistedState::default(),
        )
        .unwrap();
        let pending = PendingPlan {
            source_paths: inspection
                .skills
                .iter()
                .map(|skill| (skill.skill_id.clone(), PathBuf::from(&skill.prepared_path)))
                .collect(),
            source_guards: inspection
                .skills
                .iter()
                .map(|skill| (skill.skill_id.clone(), skill.content_hash.clone()))
                .collect(),
            target_guards: plan
                .entries
                .iter()
                .map(|entry| {
                    (
                        entry.entry_id.clone(),
                        target_guard(Path::new(&entry.resolved_path)),
                    )
                })
                .collect(),
            installation_guards: plan
                .entries
                .iter()
                .map(|entry| (entry.entry_id.clone(), None))
                .collect(),
            pinned_skill_ids: HashSet::new(),
            created_at: plan.created_at.clone(),
            expires_at: plan.expires_at.clone(),
            public: plan,
        };
        let progress = std::cell::RefCell::new(Vec::new());
        let cancelled = AtomicBool::new(false);
        let on_progress = |completed: usize| {
            progress.borrow_mut().push(completed);
            if completed == 1 {
                cancelled.store(true, Ordering::Release);
            }
        };

        let results = apply_install_plan_inner_controlled(
            pending,
            &[],
            data.path(),
            Some(&cancelled),
            Some(&on_progress),
        )
        .unwrap();

        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert_eq!(results[1].status, "cancelled");
        assert!(!Path::new(&results[1].path).exists());
        assert_eq!(progress.into_inner(), vec![1, 1]);
    }
}
