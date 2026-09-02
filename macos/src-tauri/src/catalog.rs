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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use uuid::Uuid;

/// Public GitHub Contents API endpoint.  skills.sh's private API requires a
/// Vercel OIDC token, so desktop builds intentionally do not pretend it is a
/// directly synchronisable source.
pub const GITHUB_SKILLS_URL: &str =
    "https://api.github.com/repos/anthropics/skills/contents/skills?ref=main";
pub const CATALOG_TTL_SECS: i64 = 15 * 60;
pub const MAX_CATALOG_BODY: usize = 20 * 1024 * 1024;
const GITHUB_PAGE_SIZE: usize = 100;
const GITHUB_MAX_PAGES: usize = 20;

pub fn builtin_sources() -> Vec<CatalogSource> {
    vec![CatalogSource {
        id: "anthropics-skills".to_string(),
        name: "Anthropic Skills (GitHub)".to_string(),
        url: GITHUB_SKILLS_URL.to_string(),
        provider: "github-contents".to_string(),
        enabled: true,
        etag: None,
        last_modified: None,
        last_synced_at: None,
    }]
}

pub fn ensure_sources(state: &mut crate::domain::PersistedState) {
    if state.catalog_sources.is_empty() {
        state.catalog_sources = builtin_sources();
        return;
    }
    // v0.6.0 initially shipped a skills.sh endpoint that is no longer
    // directly callable from a desktop app.  Replace only that known built-in
    // descriptor; user-added sources remain untouched.
    if let Some(legacy) = state
        .catalog_sources
        .iter_mut()
        .find(|source| source.id == "skills-sh")
    {
        let enabled = legacy.enabled;
        *legacy = builtin_sources().remove(0);
        legacy.enabled = enabled;
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

fn retimestamp_snapshot(
    data_dir: &Path,
    mut snapshot: CatalogSnapshot,
) -> Result<CatalogSnapshot, String> {
    snapshot.fetched_at = Utc::now().to_rfc3339();
    let _ = save_snapshot(data_dir, &snapshot)?;
    Ok(snapshot)
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

fn github_contents_page_url(raw: &str, page: usize) -> Result<String, String> {
    let mut url = url::Url::parse(raw).map_err(|_| "GitHub 目录 URL 无效".to_string())?;
    let mut pairs = url
        .query_pairs()
        .filter(|(key, _)| key != "page" && key != "per_page")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    pairs.push(("per_page".to_string(), GITHUB_PAGE_SIZE.to_string()));
    pairs.push(("page".to_string(), page.to_string()));
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(pairs);
    let query = serializer.finish();
    url.set_query(Some(&query));
    Ok(url.to_string())
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
            let mut entry = CatalogEntry {
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
            };
            if entry.skill_url.is_some() && source_for_entry(&entry).is_err() {
                entry
                    .warnings
                    .push("来源元数据与 URL 不一致，仅可查看，安装前请重新确认来源。".to_string());
                entry.skill_url = None;
            }
            Some(entry)
        })
        .collect()
}

pub fn parse_provider_entries(source: &CatalogSource, payload: &Value) -> Vec<CatalogEntry> {
    if source.provider != "github-contents" {
        return parse_entries(&source.id, payload);
    }
    let Some(values) = payload.as_array() else {
        return Vec::new();
    };
    let (owner, repository) = url::Url::parse(&source.url)
        .ok()
        .and_then(|url| {
            let parts: Vec<_> = url.path_segments()?.collect();
            if parts.len() >= 4 && parts[0] == "repos" {
                Some((parts[1].to_string(), parts[2].to_string()))
            } else {
                None
            }
        })
        .unwrap_or_else(|| ("anthropics".to_string(), "skills".to_string()));
    values
        .iter()
        .filter(|value| {
            value
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(|kind| kind == "dir")
        })
        .filter_map(|value| {
            let path = object_string(value, &["path"])?;
            let name = object_string(value, &["name"]).unwrap_or_else(|| path.clone());
            let skill_url = object_string(value, &["html_url"]).or_else(|| {
                Some(format!(
                    "https://github.com/{owner}/{repository}/tree/main/{path}"
                ))
            });
            Some(CatalogEntry {
                id: format!("{}:{owner}/{repository}:{path}", source.id),
                source_id: source.id.clone(),
                name,
                description: "来自 GitHub 公开目录（打开详情后读取 SKILL.md）".to_string(),
                owner: Some(owner.clone()),
                repository: Some(repository.clone()),
                reference: Some("main".to_string()),
                path: Some(path),
                commit_sha: None,
                license: None,
                stars: None,
                updated_at: None,
                skill_url,
                has_scripts: false,
                installed_state: CatalogInstallState::NotInstalled,
                warnings: vec![
                    "目录 API 未提供 SKILL.md 内容，请在安装预览中再次检查来源。".to_string(),
                ],
            })
        })
        .collect()
}

/// Build the only installable source descriptor accepted from a catalog
/// entry.  All duplicated metadata must agree with the public GitHub URL;
/// otherwise the entry remains browse-only and cannot be used to install a
/// different repository under a trusted-looking name.
pub fn source_for_entry(
    entry: &CatalogEntry,
) -> Result<(SkillSource, crate::domain::SkillSourceDetails), String> {
    let raw = entry
        .skill_url
        .as_deref()
        .ok_or_else(|| "目录条目没有公开 Skill URL".to_string())?;
    let parsed = url::Url::parse(raw).map_err(|_| "目录 Skill URL 无效".to_string())?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("github.com") {
        return Err("目录 Skill URL 必须是 github.com HTTPS 地址".to_string());
    }
    let parts: Vec<_> = parsed
        .path_segments()
        .ok_or_else(|| "目录 Skill URL 缺少路径".to_string())?
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 2 {
        return Err("目录 Skill URL 缺少 owner/repository".to_string());
    }
    let owner = parts[0].to_string();
    let repository = parts[1].trim_end_matches(".git").to_string();
    if entry.owner.as_deref().is_some_and(|value| value != owner)
        || entry
            .repository
            .as_deref()
            .is_some_and(|value| value.trim_end_matches(".git") != repository)
    {
        return Err("目录条目的 owner/repository 与 URL 不一致".to_string());
    }
    let (reference, mut subpath) =
        if parts.len() >= 4 && matches!(parts[2], "tree" | "blob" | "commit") {
            (parts[3].to_string(), parts[4..].join("/"))
        } else {
            ("HEAD".to_string(), parts[2..].join("/"))
        };
    if subpath.ends_with("/SKILL.md") {
        subpath.truncate(subpath.len() - "/SKILL.md".len());
    } else if subpath == "SKILL.md" {
        subpath.clear();
    }
    if entry
        .reference
        .as_deref()
        .is_some_and(|value| value != reference)
        || entry.path.as_deref().is_some_and(|value| value != subpath)
        || entry
            .commit_sha
            .as_deref()
            .is_some_and(|sha| sha != reference)
    {
        return Err("目录条目的 path/ref/commit SHA 与 URL 不一致".to_string());
    }
    if reference == "." || reference == ".." || reference.chars().any(char::is_control) {
        return Err("目录 Skill ref 无效".to_string());
    }
    let details = crate::domain::SkillSourceDetails {
        owner: Some(owner),
        repository: Some(repository),
        reference: Some(reference),
        subpath: Some(subpath),
        commit_sha: entry.commit_sha.clone(),
        ..Default::default()
    };
    Ok((
        SkillSource::Github {
            url: raw.to_string(),
        },
        details,
    ))
}

pub fn validate_collection_ref(
    reference: &crate::domain::CollectionSkillRef,
) -> Result<(), String> {
    let SkillSource::Github { url } = &reference.source else {
        return Err("集合只支持公开 GitHub Skill 来源".to_string());
    };
    let temporary = CatalogEntry {
        id: reference.catalog_entry_id.clone(),
        source_id: String::new(),
        name: reference
            .skill_name
            .clone()
            .unwrap_or_else(|| "skill".to_string()),
        description: String::new(),
        owner: reference.source_details.owner.clone(),
        repository: reference.source_details.repository.clone(),
        reference: reference.source_details.reference.clone(),
        path: reference
            .path
            .clone()
            .or_else(|| reference.source_details.subpath.clone()),
        commit_sha: reference
            .commit_sha
            .clone()
            .or_else(|| reference.source_details.commit_sha.clone()),
        license: None,
        stars: None,
        updated_at: None,
        skill_url: Some(url.clone()),
        has_scripts: false,
        installed_state: CatalogInstallState::NotInstalled,
        warnings: Vec::new(),
    };
    let _ = source_for_entry(&temporary)?;
    Ok(())
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

pub async fn sync_source_with_control(
    data_dir: &Path,
    source: &CatalogSource,
    cancel_requested: Option<&AtomicBool>,
) -> Result<(CatalogSnapshot, bool), String> {
    if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err("目录同步已取消".to_string());
    }
    if let Some(snapshot) = load_snapshot(data_dir, &source.id)? {
        if !cache_is_stale(&snapshot) {
            return Ok((snapshot, true));
        }
    }
    let client = reqwest::Client::builder()
        .user_agent("Skill-Installer/0.6.0")
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?;
    let mut payloads = Vec::new();
    let mut total_bytes = 0usize;
    let mut page = 1usize;
    let mut etag = None;
    let mut last_modified = None;
    loop {
        if cancel_requested.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err("目录同步已取消".to_string());
        }
        let page_url = if source.provider == "github-contents" {
            github_contents_page_url(&source.url, page)?
        } else {
            source.url.clone()
        };
        let mut request = client.get(page_url);
        // Validators apply to the first page.  Subsequent pages have their own
        // cache semantics and are bounded by GITHUB_MAX_PAGES.
        if page == 1 {
            if let Some(value) = source.etag.as_deref() {
                request = request.header(IF_NONE_MATCH, value);
            }
            if let Some(value) = source.last_modified.as_deref() {
                request = request.header(IF_MODIFIED_SINCE, value);
            }
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("目录同步失败: {error}"))?;
        if page == 1 && response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return load_snapshot(data_dir, &source.id)?
                .map(|snapshot| retimestamp_snapshot(data_dir, snapshot).map(|next| (next, true)))
                .transpose()?
                .ok_or_else(|| "目录返回 304 但本地没有缓存".to_string());
        }
        if !response.status().is_success() {
            return Err(format!("目录返回 {}", response.status()));
        }
        if page == 1 {
            etag = response
                .headers()
                .get(ETAG)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            last_modified = response
                .headers()
                .get(LAST_MODIFIED)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
        }
        let body = response
            .bytes()
            .await
            .map_err(|error| format!("读取目录响应失败: {error}"))?;
        total_bytes = total_bytes.saturating_add(body.len());
        if total_bytes > MAX_CATALOG_BODY {
            return Err("目录响应超过 20 MB 限制".to_string());
        }
        let payload = serde_json::from_slice::<Value>(&body)
            .map_err(|error| format!("目录 JSON 无效: {error}"))?;
        let page_len = payload.as_array().map_or(0, Vec::len);
        payloads.push(payload);
        if source.provider != "github-contents"
            || page_len < GITHUB_PAGE_SIZE
            || page >= GITHUB_MAX_PAGES
        {
            break;
        }
        page += 1;
    }
    let payload = if payloads.len() == 1 {
        payloads.pop().expect("one payload")
    } else {
        let mut values = Vec::new();
        for payload in payloads {
            if let Some(array) = payload.as_array() {
                values.extend(array.iter().cloned());
            }
        }
        Value::Array(values)
    };
    let snapshot = CatalogSnapshot {
        source_id: source.id.clone(),
        fetched_at: Utc::now().to_rfc3339(),
        etag,
        last_modified,
        entries: parse_provider_entries(source, &payload),
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
        source_refs: existing
            .map(|value| value.source_refs.clone())
            .unwrap_or_default(),
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
    fn github_contents_provider_maps_public_directory_contract() {
        let source = CatalogSource {
            id: "anthropics-skills".into(),
            name: "Anthropic Skills".into(),
            url: GITHUB_SKILLS_URL.into(),
            provider: "github-contents".into(),
            enabled: true,
            etag: None,
            last_modified: None,
            last_synced_at: None,
        };
        let payload = serde_json::json!([
            {"name":"code-review","path":"skills/code-review","type":"dir","html_url":"https://github.com/anthropics/skills/tree/main/skills/code-review"},
            {"name":"README.md","path":"skills/README.md","type":"file","html_url":"https://github.com/anthropics/skills/blob/main/skills/README.md"}
        ]);
        let entries = parse_provider_entries(&source, &payload);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].owner.as_deref(), Some("anthropics"));
        assert_eq!(entries[0].repository.as_deref(), Some("skills"));
        assert_eq!(entries[0].path.as_deref(), Some("skills/code-review"));
        assert_eq!(entries[0].reference.as_deref(), Some("main"));
        assert!(source_for_entry(&entries[0]).is_ok());
    }

    #[test]
    fn catalog_entry_source_descriptor_rejects_metadata_spoofing() {
        let entry = CatalogEntry {
            id: "catalog:spoof".into(),
            source_id: "catalog".into(),
            name: "demo".into(),
            description: String::new(),
            owner: Some("trusted-owner".into()),
            repository: Some("skills".into()),
            reference: Some("main".into()),
            path: Some("demo".into()),
            commit_sha: None,
            license: None,
            stars: None,
            updated_at: None,
            skill_url: Some("https://github.com/other-owner/skills/tree/main/demo".into()),
            has_scripts: false,
            installed_state: CatalogInstallState::NotInstalled,
            warnings: Vec::new(),
        };
        assert!(source_for_entry(&entry).is_err());
    }

    #[test]
    fn github_contents_page_url_preserves_ref_and_replaces_paging() {
        let page = github_contents_page_url(
            "https://api.github.com/repos/acme/skills/contents?ref=main&page=9",
            2,
        )
        .unwrap();
        assert!(page.contains("ref=main"));
        assert!(page.contains("per_page=100"));
        assert!(page.contains("page=2"));
        assert!(!page.contains("page=9"));
    }

    #[test]
    fn legacy_skills_sh_descriptor_is_replaced_by_public_github_provider() {
        let mut state = crate::domain::PersistedState {
            catalog_sources: vec![CatalogSource {
                id: "skills-sh".into(),
                name: "skills.sh".into(),
                url: "https://skills.sh/api/skills".into(),
                provider: "skills-sh".into(),
                enabled: true,
                etag: Some("old".into()),
                last_modified: None,
                last_synced_at: None,
            }],
            ..Default::default()
        };
        ensure_sources(&mut state);
        assert_eq!(state.catalog_sources[0].id, "anthropics-skills");
        assert_eq!(state.catalog_sources[0].provider, "github-contents");
        assert_eq!(state.catalog_sources[0].url, GITHUB_SKILLS_URL);
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
            source_refs: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let assignments = collection_assignments(&collection);
        assert_eq!(assignments[0].skill_id, "a");
        assert_eq!(assignments[1].client_ids, collection.default_client_ids);
    }
}
