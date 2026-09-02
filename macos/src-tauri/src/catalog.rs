//! Public catalog discovery and its small, offline-first cache.
//!
//! Catalog data is deliberately kept separate from the installer core.  A
//! catalog entry is metadata only; selecting one still goes through the
//! existing `inspect_source -> plan_install -> apply_install_plan` pipeline.

use crate::domain::{
    CatalogCacheMetadata, CatalogEntry, CatalogInstallState, CatalogSnapshot, CatalogSource,
    SkillCollection, SkillSource,
};
use crate::storage::{cache_dir, StateRepository};
use chrono::Utc;
use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

pub const SKILLS_SH_URL: &str = "https://skills.sh/api/skills";
pub const CATALOG_TTL_SECS: i64 = 15 * 60;

pub fn builtin_sources() -> Vec<CatalogSource> {
    vec![CatalogSource {
        id: "skills-sh".to_string(),
        name: "skills.sh".to_string(),
        url: SKILLS_SH_URL.to_string(),
        provider: "skills.sh".to_string(),
        enabled: true,
        etag: None,
        last_modified: None,
        last_synced_at: None,
    }]
}

pub fn ensure_sources(state: &mut crate::domain::PersistedState) {
    if state.catalog_sources.is_empty() {
        state.catalog_sources = builtin_sources();
    }
}

pub fn cache_path(data_dir: &Path, source_id: &str) -> PathBuf {
    cache_dir(data_dir).join(format!("catalog-{}.json", safe_file_component(source_id)))
}

fn safe_file_component(value: &str) -> String {
    let mut output: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect();
    if output.is_empty() {
        output = Uuid::new_v4().to_string();
    }
    output
}

pub fn load_snapshot(data_dir: &Path, source_id: &str) -> Result<Option<CatalogSnapshot>, String> {
    let path = cache_path(data_dir, source_id);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("目录缓存无效: {error}"))
}

pub fn cache_is_stale(snapshot: &CatalogSnapshot) -> bool {
    let Ok(fetched) = chrono::DateTime::parse_from_rfc3339(&snapshot.fetched_at) else {
        return true;
    };
    let age = Utc::now()
        .signed_duration_since(fetched.with_timezone(&Utc))
        .to_std()
        .unwrap_or(Duration::ZERO);
    age > Duration::from_secs(CATALOG_TTL_SECS as u64)
}

fn save_snapshot(
    data_dir: &Path,
    snapshot: &CatalogSnapshot,
) -> Result<CatalogCacheMetadata, String> {
    let path = cache_path(data_dir, &snapshot.source_id);
    fs::create_dir_all(cache_dir(data_dir)).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("json.{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(snapshot).map_err(|error| error.to_string())?;
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, &path).map_err(|error| error.to_string())?;
    Ok(CatalogCacheMetadata {
        source_id: snapshot.source_id.clone(),
        cache_path: path.display().to_string(),
        fetched_at: snapshot.fetched_at.clone(),
        etag: snapshot.etag.clone(),
        last_modified: snapshot.last_modified.clone(),
        entry_count: snapshot.entries.len(),
    })
}

fn object_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str).map(str::to_owned))
}

fn object_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|raw| {
            raw.as_u64()
                .or_else(|| raw.as_str().and_then(|text| text.parse().ok()))
        })
    })
}

/// Parse common skills.sh/GitHub catalog response shapes.  Keeping this
/// parser tolerant lets the provider evolve without changing the installer.
pub fn parse_entries(source_id: &str, payload: &Value) -> Vec<CatalogEntry> {
    let values = payload
        .as_array()
        .cloned()
        .or_else(|| {
            ["skills", "items", "data", "results"]
                .iter()
                .find_map(|key| payload.get(*key).and_then(Value::as_array).cloned())
        })
        .unwrap_or_default();
    values
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let name = object_string(&value, &["name", "skill", "slug"])?;
            let description =
                object_string(&value, &["description", "summary"]).unwrap_or_default();
            let owner = object_string(&value, &["owner", "author"]);
            let repository = object_string(&value, &["repository", "repo"]);
            let reference = object_string(&value, &["ref", "reference", "branch"]);
            let path = object_string(&value, &["path", "subpath"]);
            let commit_sha = object_string(&value, &["commitSha", "commit_sha", "sha"]);
            let skill_url = object_string(&value, &["url", "skillUrl", "skill_url"]);
            let id = object_string(&value, &["id", "skillId", "skill_id"]).unwrap_or_else(|| {
                format!(
                    "{}:{}:{}:{}",
                    source_id,
                    owner.as_deref().unwrap_or_default(),
                    repository.as_deref().unwrap_or_default(),
                    path.as_deref().unwrap_or(&name)
                )
            });
            Some(CatalogEntry {
                id: if id.is_empty() {
                    format!("{source_id}:{index}:{name}")
                } else {
                    id
                },
                source_id: source_id.to_string(),
                name,
                description,
                owner,
                repository,
                reference,
                path,
                commit_sha,
                license: object_string(&value, &["license"]),
                stars: object_u64(&value, &["stars", "stargazers_count", "downloads"]),
                updated_at: object_string(
                    &value,
                    &["updatedAt", "updated_at", "updated", "publishedAt"],
                ),
                skill_url,
                has_scripts: value
                    .get("hasScripts")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                installed_state: CatalogInstallState::NotInstalled,
                warnings: Vec::new(),
            })
        })
        .collect()
}

pub fn all_cached_entries(
    data_dir: &Path,
    sources: &[CatalogSource],
) -> Result<Vec<CatalogEntry>, String> {
    let mut entries = Vec::new();
    for source in sources.iter().filter(|source| source.enabled) {
        if let Some(snapshot) = load_snapshot(data_dir, &source.id)? {
            let stale = cache_is_stale(&snapshot);
            entries.extend(snapshot.entries.into_iter().map(|mut entry| {
                if stale {
                    entry
                        .warnings
                        .push("目录缓存已过期，建议重新同步".to_string());
                }
                entry
            }));
        }
    }
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    Ok(entries)
}

/// Reconcile catalog metadata with tracked installations without ever
/// conflating two repositories that happen to use the same Skill name.
/// Installations with no GitHub provenance are intentionally left unmatched.
pub fn reconcile_entries(
    entries: &mut [CatalogEntry],
    installations: &[crate::domain::PhysicalInstallation],
) {
    for entry in entries {
        let matches: Vec<_> = installations
            .iter()
            .filter(|installation| {
                let Some(source) = installation.source.as_ref() else {
                    return false;
                };
                let SkillSource::Github { .. } = source else {
                    return false;
                };
                let details = &installation.source_details;
                let owner_matches = entry.owner.as_deref() == details.owner.as_deref();
                let repository_matches =
                    entry.repository.as_deref() == details.repository.as_deref();
                let path_matches = match (entry.path.as_deref(), details.subpath.as_deref()) {
                    (Some(expected), Some(actual)) => {
                        actual.ends_with(expected) || expected.ends_with(actual)
                    }
                    (None, _) => true,
                    _ => false,
                };
                owner_matches
                    && repository_matches
                    && path_matches
                    && installation.skill_name == entry.name
            })
            .collect();
        if matches.is_empty() {
            continue;
        }
        let has_update = entry.commit_sha.as_deref().is_some_and(|sha| {
            matches.iter().any(|installation| {
                installation
                    .source_details
                    .commit_sha
                    .as_deref()
                    .is_some_and(|installed| installed != sha)
            })
        });
        entry.installed_state = if has_update {
            CatalogInstallState::UpdateAvailable
        } else if matches
            .iter()
            .any(|installation| installation.consumers.len() > 1)
        {
            CatalogInstallState::Installed
        } else {
            CatalogInstallState::Partial
        };
    }
}

pub fn update_cache_metadata(
    repository: &StateRepository,
    metadata: CatalogCacheMetadata,
    source: CatalogSource,
) -> Result<(), String> {
    repository.mutate(|state| {
        ensure_sources(state);
        if let Some(existing) = state
            .catalog_sources
            .iter_mut()
            .find(|item| item.id == source.id)
        {
            *existing = source;
        }
        state
            .catalog_cache
            .retain(|item| item.source_id != metadata.source_id);
        state.catalog_cache.push(metadata);
        Ok(())
    })
}

pub async fn sync_source(
    data_dir: &Path,
    source: &CatalogSource,
) -> Result<(CatalogSnapshot, bool), String> {
    let client = reqwest::Client::builder()
        .user_agent("Skill-Installer/0.6.0")
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client.get(&source.url);
    if let Some(etag) = source.etag.as_deref() {
        request = request.header(IF_NONE_MATCH, etag);
    }
    if let Some(last_modified) = source.last_modified.as_deref() {
        request = request.header(IF_MODIFIED_SINCE, last_modified);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("目录同步失败: {error}"))?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return load_snapshot(data_dir, &source.id)?
            .map(|snapshot| (snapshot, true))
            .ok_or_else(|| "目录返回 304 但本地没有缓存".to_string());
    }
    if !response.status().is_success() {
        return Err(format!("目录返回 {}", response.status()));
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let last_modified = response
        .headers()
        .get(LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let payload = response
        .json::<Value>()
        .await
        .map_err(|error| format!("目录 JSON 无效: {error}"))?;
    let snapshot = CatalogSnapshot {
        source_id: source.id.clone(),
        fetched_at: Utc::now().to_rfc3339(),
        etag,
        last_modified,
        entries: parse_entries(&source.id, &payload),
    };
    let _ = save_snapshot(data_dir, &snapshot)?;
    Ok((snapshot, false))
}

pub fn collection_from_input(
    id: Option<String>,
    name: String,
    description: Option<String>,
    skill_refs: Vec<String>,
    default_client_ids: Vec<String>,
    existing: Option<&SkillCollection>,
) -> Result<SkillCollection, String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 80 {
        return Err("集合名称不能为空且不能超过 80 个字符".to_string());
    }
    if skill_refs.is_empty() {
        return Err("集合至少需要一个 Skill".to_string());
    }
    let now = Utc::now().to_rfc3339();
    Ok(SkillCollection {
        id: id
            .or_else(|| existing.map(|value| value.id.clone()))
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        name: name.to_string(),
        description,
        skill_refs: skill_refs.into_iter().collect(),
        default_client_ids: default_client_ids.into_iter().collect(),
        created_at: existing
            .map(|value| value.created_at.clone())
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
    })
}

pub fn collection_assignments(collection: &SkillCollection) -> Vec<crate::domain::SkillAssignment> {
    collection
        .skill_refs
        .iter()
        .map(|skill_id| crate::domain::SkillAssignment {
            skill_id: skill_id.clone(),
            client_ids: collection.default_client_ids.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_array_and_metadata_fields() {
        let payload = serde_json::json!({
            "skills": [{
                "name": "rust-review",
                "description": "Review Rust code",
                "owner": "acme",
                "repo": "skills",
                "ref": "main",
                "path": "engineering/rust-review",
                "sha": "abc123",
                "stars": 42,
                "hasScripts": true
            }]
        });
        let entries = parse_entries("test", &payload);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "rust-review");
        assert_eq!(entries[0].stars, Some(42));
        assert!(entries[0].has_scripts);
        assert_eq!(entries[0].commit_sha.as_deref(), Some("abc123"));
    }

    #[test]
    fn collection_requires_name_and_skill() {
        assert!(
            collection_from_input(None, "".into(), None, vec!["x".into()], vec![], None).is_err()
        );
        assert!(collection_from_input(None, "x".into(), None, vec![], vec![], None).is_err());
    }

    #[test]
    fn same_name_from_different_repositories_is_not_reconciled() {
        let mut entries = vec![CatalogEntry {
            id: "catalog:acme:skills:demo".into(),
            source_id: "catalog".into(),
            name: "demo".into(),
            description: String::new(),
            owner: Some("acme".into()),
            repository: Some("skills".into()),
            reference: Some("main".into()),
            path: Some("demo".into()),
            commit_sha: None,
            license: None,
            stars: None,
            updated_at: None,
            skill_url: None,
            has_scripts: false,
            installed_state: CatalogInstallState::NotInstalled,
            warnings: Vec::new(),
        }];
        let installation = crate::domain::PhysicalInstallation {
            id: "i".into(),
            skill_name: "demo".into(),
            resolved_path: "/tmp/demo".into(),
            source: Some(crate::domain::SkillSource::Github {
                url: "https://github.com/other/skills".into(),
            }),
            source_details: crate::domain::SkillSourceDetails {
                owner: Some("other".into()),
                repository: Some("skills".into()),
                subpath: Some("demo".into()),
                ..Default::default()
            },
            content_hash: "hash".into(),
            scope: crate::domain::InstallScope::Global,
            consumers: vec!["codex".into()],
            passive_consumers: Vec::new(),
            adapter_version: 1,
            installed_at: String::new(),
            provenance: crate::domain::InstallationProvenance::Tool,
            legacy_project: false,
        };
        reconcile_entries(&mut entries, &[installation]);
        assert_eq!(
            entries[0].installed_state,
            CatalogInstallState::NotInstalled
        );
    }

    #[test]
    fn cache_age_is_detected_without_touching_content() {
        let snapshot = CatalogSnapshot {
            source_id: "test".into(),
            fetched_at: "2000-01-01T00:00:00Z".into(),
            etag: None,
            last_modified: None,
            entries: Vec::new(),
        };
        assert!(cache_is_stale(&snapshot));
    }

    #[test]
    fn collection_assignments_preserve_order_and_defaults() {
        let collection = SkillCollection {
            id: "c".into(),
            name: "coding".into(),
            description: None,
            skill_refs: vec!["a".into(), "b".into()],
            default_client_ids: vec!["codex".into(), "kiro".into()],
            created_at: String::new(),
            updated_at: String::new(),
        };
        let assignments = collection_assignments(&collection);
        assert_eq!(assignments[0].skill_id, "a");
        assert_eq!(assignments[1].client_ids, collection.default_client_ids);
    }
}
