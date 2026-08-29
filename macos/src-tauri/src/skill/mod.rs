use crate::domain::{
    RejectedSkill, SkillMetadata, SkillSource, SkillSourceDetails, SourceInspection,
};
use crate::storage::{cache_dir, inspect_tree, MAX_BYTES, MAX_FILES};
use futures_util::StreamExt;
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::io::{Cursor, Read, Seek};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::ZipArchive;

const MAX_DOWNLOAD: u64 = 50 * 1024 * 1024;

#[derive(Deserialize)]
struct Frontmatter {
    name: String,
    description: String,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    compatibility: Option<String>,
    #[serde(default)]
    metadata: Option<serde_yaml::Value>,
    #[serde(rename = "allowed-tools", default)]
    allowed_tools: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct GithubLocation {
    owner: String,
    repository: String,
    reference: String,
    subpath: String,
}

type DiscoveryResult = (Vec<SkillMetadata>, Vec<RejectedSkill>, Vec<String>);

fn parse_github_url(raw: &str) -> Result<GithubLocation, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("GitHub 来源不能为空".to_string());
    }

    // The compact `owner/repository[@ref[:path]]` form is useful when a
    // repository URL is copied from a README or terminal.  Keep it strict so
    // arbitrary local paths are not accidentally treated as public sources.
    if !raw.contains("://") {
        let (repository_spec, shorthand_path) = raw
            .split_once(':')
            .map_or((raw, None), |(left, right)| (left, Some(right)));
        let (repository_spec, reference) = repository_spec
            .split_once('@')
            .map_or((repository_spec, "HEAD"), |(repository, reference)| {
                (repository, reference)
            });
        let mut parts = repository_spec.split('/').filter(|part| !part.is_empty());
        let owner = parts.next().unwrap_or_default();
        let repository = parts.next().unwrap_or_default();
        if owner.is_empty() || repository.is_empty() || parts.next().is_some() {
            return Err("GitHub 简写需要 owner/repository".to_string());
        }
        let repository = repository.trim_end_matches(".git");
        validate_github_component(owner, "owner")?;
        validate_github_component(repository, "repository")?;
        validate_github_reference(reference)?;
        let subpath = normalize_github_subpath(shorthand_path.unwrap_or_default())?;
        return Ok(GithubLocation {
            owner: owner.to_string(),
            repository: repository.to_string(),
            reference: reference.to_string(),
            subpath,
        });
    }

    let url = url::Url::parse(raw).map_err(|_| "GitHub URL 无效".to_string())?;
    if url.host_str() != Some("github.com") {
        return Err("仅支持 github.com 公共仓库".to_string());
    }
    let parts: Vec<_> = url
        .path_segments()
        .ok_or_else(|| "GitHub URL 缺少路径".to_string())?
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 2 {
        return Err("GitHub URL 需要 owner/repository".to_string());
    }
    let owner = parts[0];
    let repository = parts[1].trim_end_matches(".git");
    validate_github_component(owner, "owner")?;
    validate_github_component(repository, "repository")?;
    let (mut reference, mut subpath) =
        if parts.len() >= 4 && (parts[2] == "tree" || parts[2] == "blob") {
            (parts[3].to_string(), parts[4..].join("/"))
        } else if parts.len() >= 4 && parts[2] == "commit" {
            // A commit URL pins the archive to that exact SHA.  Commit pages do
            // not normally contain a directory path, but accepting the optional
            // suffix makes copied deep links deterministic as well.
            (parts[3].to_string(), parts[4..].join("/"))
        } else {
            ("HEAD".to_string(), String::new())
        };
    let query = url.query_pairs().collect::<HashMap<_, _>>();
    if !matches!(
        parts.get(2),
        Some(&"tree") | Some(&"blob") | Some(&"commit")
    ) {
        if let Some(value) = query.get("ref") {
            reference = value.to_string();
        }
        if let Some(value) = query.get("path") {
            subpath = value.to_string();
        }
    }
    validate_github_reference(&reference)?;
    let subpath = normalize_github_subpath(&subpath)?;
    Ok(GithubLocation {
        owner: owner.to_string(),
        repository: repository.to_string(),
        reference,
        subpath,
    })
}

fn validate_github_component(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.chars().any(char::is_control)
        || value.contains('\\')
        || value
            .chars()
            .any(|character| !character.is_ascii_alphanumeric() && !"._-".contains(character))
    {
        return Err(format!("GitHub {label} 无效"));
    }
    Ok(())
}

fn validate_github_reference(reference: &str) -> Result<(), String> {
    if reference.is_empty()
        || reference == "."
        || reference == ".."
        || reference.chars().any(char::is_control)
        || reference
            .chars()
            .any(|character| !character.is_ascii_alphanumeric() && !"._-/".contains(character))
        || reference
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("GitHub ref 无效".to_string());
    }
    Ok(())
}

fn normalize_github_subpath(path: &str) -> Result<String, String> {
    if path.chars().any(char::is_control) || path.contains('\\') {
        return Err("GitHub Skill 子路径无效".to_string());
    }
    let mut normalized = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(value) => normalized.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err("GitHub Skill 子路径无效".to_string())
            }
        }
    }
    let mut subpath = normalized.join("/");
    if subpath.ends_with("/SKILL.md") {
        subpath.truncate(subpath.len() - "/SKILL.md".len());
    } else if subpath == "SKILL.md" {
        subpath.clear();
    }
    Ok(subpath)
}

fn validate_skill(
    path: &Path,
    relative_path: String,
    source: SkillSource,
    source_details: SkillSourceDetails,
) -> Result<SkillMetadata, String> {
    if !path.is_dir() {
        return Err("Skill 来源必须是目录".to_string());
    }
    let manifest_path = path.join("SKILL.md");
    let raw = fs::read_to_string(&manifest_path)
        .map_err(|_| format!("缺少或无法读取 {}", manifest_path.display()))?;
    let body = raw
        .strip_prefix("---")
        .ok_or_else(|| "SKILL.md 缺少 YAML frontmatter".to_string())?;
    let end = body
        .find("\n---")
        .ok_or_else(|| "SKILL.md frontmatter 未结束".to_string())?;
    let frontmatter: Frontmatter =
        serde_yaml::from_str(&body[..end]).map_err(|error| format!("YAML 无效: {error}"))?;
    let name_pattern = Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("valid skill name regex");
    if !name_pattern.is_match(&frontmatter.name) {
        return Err("name 必须是小写字母、数字和单连字符组成".to_string());
    }
    if frontmatter.name.len() > 64 {
        return Err("name 不能超过 64 个字符".to_string());
    }
    let folder = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Skill 目录名无效".to_string())?;
    if folder != frontmatter.name {
        return Err(format!(
            "Skill name “{}” 必须与目录名 “{}” 一致",
            frontmatter.name, folder
        ));
    }
    if frontmatter.description.trim().is_empty() {
        return Err("description 不能为空".to_string());
    }
    if frontmatter.description.len() > 1024 {
        return Err("description 不能超过 1,024 个字符".to_string());
    }
    let (content_hash, file_count, total_bytes, has_scripts) = inspect_tree(path)?;
    let mut warnings = Vec::new();
    if has_scripts {
        warnings.push("Skill 包含脚本或可执行文件；安装器不会执行它们。".to_string());
    }
    Ok(SkillMetadata {
        skill_id: Uuid::new_v4().to_string(),
        relative_path,
        name: frontmatter.name,
        description: frontmatter.description,
        license: frontmatter.license,
        compatibility: frontmatter.compatibility,
        metadata: frontmatter
            .metadata
            .and_then(|value| serde_json::to_value(value).ok()),
        allowed_tools: frontmatter.allowed_tools,
        source,
        source_details,
        prepared_path: path.display().to_string(),
        content_hash,
        file_count,
        total_bytes,
        has_scripts,
        warnings,
    })
}

fn ignored_entry(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git" | "node_modules" | "target" | "__MACOSX" | ".DS_Store"
            )
        })
}

fn relative_text(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| ".".to_string())
}

fn joined_subpath(base: Option<&str>, relative: &str) -> String {
    [base.unwrap_or_default(), relative]
        .into_iter()
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Copy a local source into the inspection cache.  A plan must read the
/// immutable preview snapshot rather than a mutable user directory at apply
/// time.  The source path is retained in metadata for diagnostics only.
fn snapshot_local_source(root: &Path, data_dir: &Path) -> Result<PathBuf, String> {
    // Validate the complete source before copying so limits and symlink policy
    // apply equally to a single Skill and a multi-Skill parent directory.
    inspect_tree(root)?;
    let snapshot_parent = cache_dir(data_dir).join(format!("local-{}", Uuid::new_v4()));
    let root_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("source");
    let destination = snapshot_parent.join(root_name);
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    let cache_root = cache_dir(data_dir).canonicalize().ok();
    let mut walker = WalkDir::new(root).follow_links(false).into_iter();
    while let Some(item) = walker.next() {
        let item = item.map_err(|error| error.to_string())?;
        if item.path() == snapshot_parent
            || item.path().starts_with(&snapshot_parent)
            || cache_root
                .as_ref()
                .is_some_and(|cache| item.path() == cache || item.path().starts_with(cache))
        {
            if item.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        let metadata = fs::symlink_metadata(item.path()).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!("第一阶段不接受软链接: {}", item.path().display()));
        }
        let relative = item
            .path()
            .strip_prefix(root)
            .map_err(|error| error.to_string())?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        if metadata.is_dir() {
            fs::create_dir_all(&target).map_err(|error| error.to_string())?;
        } else if metadata.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::copy(item.path(), &target).map_err(|error| error.to_string())?;
        }
    }
    Ok(destination)
}

fn discover_skills(
    root: &Path,
    source: SkillSource,
    base_details: SkillSourceDetails,
) -> Result<DiscoveryResult, String> {
    if !root.is_dir() {
        return Err("Skill 来源必须是目录".to_string());
    }
    let mut walker = WalkDir::new(root).follow_links(false).into_iter();
    let mut skills = Vec::new();
    let mut rejected = Vec::new();
    let mut warnings = Vec::new();
    let mut visited = 0usize;
    let mut duplicates = HashSet::new();

    while let Some(item) = walker.next() {
        let item = item.map_err(|error| error.to_string())?;
        visited += 1;
        if visited > MAX_FILES {
            return Err(format!("来源中的文件和目录超过限制 {MAX_FILES}"));
        }
        if ignored_entry(item.path()) {
            if item.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        if !item.file_type().is_dir() || !item.path().join("SKILL.md").is_file() {
            continue;
        }
        let relative = relative_text(root, item.path());
        let mut details = base_details.clone();
        details.subpath = Some(joined_subpath(base_details.subpath.as_deref(), &relative));
        match validate_skill(item.path(), relative.clone(), source.clone(), details) {
            Ok(skill) => {
                let duplicate_key = (skill.name.clone(), skill.content_hash.clone());
                if duplicates.insert(duplicate_key) {
                    skills.push(skill);
                } else {
                    warnings.push(format!("已合并重复 Skill: {}", skill.name));
                }
            }
            Err(reason) => rejected.push(RejectedSkill {
                relative_path: relative,
                reason,
            }),
        }
        walker.skip_current_dir();
    }

    if skills.is_empty() && rejected.is_empty() {
        return Err("来源中未发现 SKILL.md".to_string());
    }
    skills.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    rejected.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok((skills, rejected, warnings))
}

fn extract_skill_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    destination: &Path,
    subpath: &str,
    repository: &str,
) -> Result<PathBuf, String> {
    let selected = Path::new(subpath);
    if selected
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("GitHub Skill 子路径无效".to_string());
    }

    let mut archive_root = None;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| error.to_string())?;
        let enclosed = file
            .enclosed_name()
            .ok_or_else(|| "ZIP 包含路径穿越".to_string())?;
        let Some(Component::Normal(root)) = enclosed.components().next() else {
            return Err("GitHub ZIP 根目录无效".to_string());
        };
        match archive_root.as_ref() {
            Some(existing) if existing != root => {
                return Err("GitHub ZIP 包含多个根目录".to_string());
            }
            None => archive_root = Some(root.to_os_string()),
            _ => {}
        }
    }
    let archive_root = archive_root.ok_or_else(|| "GitHub ZIP 为空".to_string())?;
    let selected_prefix = PathBuf::from(archive_root).join(selected);
    let skill_folder = selected
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(repository));
    let skill_destination = destination.join(skill_folder);
    let mut extracted_files = 0usize;
    let mut extracted_bytes = 0u64;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        let enclosed = file
            .enclosed_name()
            .ok_or_else(|| "ZIP 包含路径穿越".to_string())?
            .to_path_buf();
        if !enclosed.starts_with(&selected_prefix) {
            continue;
        }
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            continue;
        }
        let relative = enclosed
            .strip_prefix(&selected_prefix)
            .map_err(|error| error.to_string())?;
        let target = skill_destination.join(relative);
        if file.is_dir() {
            fs::create_dir_all(&target).map_err(|error| error.to_string())?;
        } else {
            extracted_files += 1;
            extracted_bytes = extracted_bytes.saturating_add(file.size());
            if extracted_files > MAX_FILES {
                return Err(format!("文件数量超过限制 {MAX_FILES}"));
            }
            if extracted_bytes > MAX_BYTES {
                return Err("解压内容超过 200 MB".to_string());
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut output = fs::File::create(target).map_err(|error| error.to_string())?;
            std::io::copy(&mut file, &mut output).map_err(|error| error.to_string())?;
        }
    }
    if extracted_files == 0 {
        return Err("GitHub Skill 路径不存在或目录为空".to_string());
    }
    skill_destination
        .canonicalize()
        .map_err(|error| format!("GitHub Skill 路径不存在: {error}"))
}

fn extract_local_archive(
    path: &Path,
    data_dir: &Path,
) -> Result<(PathBuf, SkillSourceDetails), String> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("zip"))
    {
        return Err("首版仅支持本地 ZIP 压缩包".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("本地 ZIP 路径无效: {error}"))?;
    let metadata = fs::metadata(&canonical).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_DOWNLOAD {
        return Err("压缩包超过 50 MB".to_string());
    }
    let file = fs::File::open(&canonical).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| format!("ZIP 无效: {error}"))?;
    let destination = cache_dir(data_dir)
        .join(Uuid::new_v4().to_string())
        .join("archive");
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    let mut extracted_files = 0usize;
    let mut extracted_bytes = 0u64;

    for index in 0..archive.len() {
        let mut item = archive.by_index(index).map_err(|error| error.to_string())?;
        let enclosed = item
            .enclosed_name()
            .ok_or_else(|| "ZIP 包含路径穿越或绝对路径".to_string())?
            .to_path_buf();
        let text = enclosed
            .to_str()
            .ok_or_else(|| "ZIP 包含非 UTF-8 文件名".to_string())?;
        if text.chars().any(char::is_control) {
            return Err("ZIP 包含控制字符文件名".to_string());
        }
        if item
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("ZIP 包含软链接，已拒绝".to_string());
        }
        let target = destination.join(&enclosed);
        if item.is_dir() {
            fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        extracted_files += 1;
        extracted_bytes = extracted_bytes.saturating_add(item.size());
        if extracted_files > MAX_FILES {
            return Err(format!("文件数量超过限制 {MAX_FILES}"));
        }
        if extracted_bytes > MAX_BYTES {
            return Err("解压内容超过 200 MB".to_string());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = fs::File::create(target).map_err(|error| error.to_string())?;
        std::io::copy(&mut item, &mut output).map_err(|error| error.to_string())?;
    }
    if extracted_files == 0 {
        return Err("ZIP 为空".to_string());
    }
    Ok((
        destination,
        SkillSourceDetails {
            archive_path: Some(canonical.display().to_string()),
            ..SkillSourceDetails::default()
        },
    ))
}

async fn download_github_with_transport<C, CFut, D, DFut>(
    raw: &str,
    data_dir: &Path,
    cancel_requested: Option<&AtomicBool>,
    on_progress: Option<&(dyn Fn(usize, Option<usize>) + Send + Sync)>,
    resolve_commit: C,
    download_archive: D,
) -> Result<(PathBuf, SkillSourceDetails), String>
where
    C: Fn(String) -> CFut + Send + Sync,
    CFut: Future<Output = Result<Option<String>, String>> + Send,
    D: Fn(String) -> DFut + Send + Sync,
    DFut: Future<Output = Result<Vec<u8>, String>> + Send,
{
    let location = parse_github_url(raw)?;
    let commit_sha = resolve_commit(format!(
        "https://api.github.com/repos/{}/{}/commits/{}",
        location.owner, location.repository, location.reference
    ))
    .await
    .unwrap_or(None);
    // The commit endpoint is authoritative for a moving ref.  Once it gives
    // us a SHA, the archive request must use that SHA rather than the original
    // branch name, otherwise a ref update between the two requests is a TOCTOU
    // window.
    let archive_reference = commit_sha.as_deref().unwrap_or(location.reference.as_str());
    let download_url = format!(
        "https://codeload.github.com/{}/{}/zip/{}",
        location.owner, location.repository, archive_reference
    );
    if cancel_requested.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("GitHub 下载已取消".to_string());
    }
    let bytes = download_archive(download_url).await?;
    if cancel_requested.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("GitHub 下载已取消".to_string());
    }
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err("下载内容超过 50 MB".to_string());
    }
    if let Some(callback) = on_progress {
        callback(bytes.len(), Some(bytes.len()));
    }

    let destination = cache_dir(data_dir).join(Uuid::new_v4().to_string());
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|error| format!("ZIP 无效: {error}"))?;
    let skill = extract_skill_archive(
        &mut archive,
        &destination,
        &location.subpath,
        &location.repository,
    )?;
    Ok((
        skill,
        SkillSourceDetails {
            owner: Some(location.owner),
            repository: Some(location.repository),
            reference: Some(location.reference),
            subpath: Some(location.subpath),
            commit_sha,
            local_path: None,
            archive_path: None,
        },
    ))
}

async fn download_github(
    raw: &str,
    data_dir: &Path,
    cancel_requested: Option<&AtomicBool>,
    on_progress: Option<&(dyn Fn(usize, Option<usize>) + Send + Sync)>,
) -> Result<(PathBuf, SkillSourceDetails), String> {
    let client = reqwest::Client::builder()
        .user_agent("Skill-Installer/0.5.1")
        .build()
        .map_err(|error| error.to_string())?;
    let resolve_commit = {
        let client = client.clone();
        move |url: String| {
            let client = client.clone();
            async move {
                let response = client.get(url).send().await.ok();
                let Some(response) = response.filter(|response| response.status().is_success())
                else {
                    return Ok(None);
                };
                let json = response.json::<serde_json::Value>().await.ok();
                Ok(json.and_then(|json| {
                    json.get("sha")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                }))
            }
        }
    };
    let download_archive = move |url: String| {
        let client = client.clone();
        let cancel_requested = cancel_requested;
        let on_progress = on_progress;
        async move {
            let response = client
                .get(url)
                .send()
                .await
                .map_err(|error| format!("GitHub 下载失败: {error}"))?;
            if !response.status().is_success() {
                return Err(format!("GitHub 返回 {}", response.status()));
            }
            if response
                .content_length()
                .is_some_and(|size| size > MAX_DOWNLOAD)
            {
                return Err("下载内容超过 50 MB".to_string());
            }
            let content_length = response
                .content_length()
                .and_then(|size| usize::try_from(size).ok());
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                if cancel_requested
                    .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire))
                {
                    return Err("GitHub 下载已取消".to_string());
                }
                let chunk = chunk.map_err(|error| format!("读取下载内容失败: {error}"))?;
                bytes.extend_from_slice(&chunk);
                if bytes.len() as u64 > MAX_DOWNLOAD {
                    return Err("下载内容超过 50 MB".to_string());
                }
                if let Some(callback) = on_progress {
                    callback(bytes.len(), content_length);
                }
            }
            Ok(bytes)
        }
    };
    download_github_with_transport(
        raw,
        data_dir,
        cancel_requested,
        on_progress,
        resolve_commit,
        download_archive,
    )
    .await
}

#[cfg(test)]
async fn inspect(source: SkillSource, data_dir: &Path) -> Result<(SkillMetadata, PathBuf), String> {
    let inspection = inspect_source(source, data_dir).await?;
    if inspection.skills.len() != 1 || !inspection.rejected.is_empty() {
        return Err("该来源包含多个 Skill，请使用批量来源检查".to_string());
    }
    let metadata = inspection.skills.into_iter().next().expect("one skill");
    let path = PathBuf::from(&metadata.prepared_path);
    Ok((metadata, path))
}

pub async fn inspect_source(
    source: SkillSource,
    data_dir: &Path,
) -> Result<SourceInspection, String> {
    inspect_source_with_control(source, data_dir, None, None).await
}

pub async fn inspect_source_with_control(
    source: SkillSource,
    data_dir: &Path,
    cancel_requested: Option<&AtomicBool>,
    on_progress: Option<&(dyn Fn(usize, Option<usize>) + Send + Sync)>,
) -> Result<SourceInspection, String> {
    crate::storage::cleanup_cache(data_dir, Duration::from_secs(24 * 60 * 60))?;
    if cancel_requested.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("来源检查已取消".to_string());
    }
    let (root, source_details) = match &source {
        SkillSource::LocalDirectory { path } => {
            let canonical = PathBuf::from(path)
                .canonicalize()
                .map_err(|error| format!("本地路径无效: {error}"))?;
            let snapshot = snapshot_local_source(&canonical, data_dir)?;
            (
                snapshot,
                SkillSourceDetails {
                    local_path: Some(canonical.display().to_string()),
                    ..SkillSourceDetails::default()
                },
            )
        }
        SkillSource::LocalArchive { path } => extract_local_archive(Path::new(path), data_dir)?,
        SkillSource::Github { url } => {
            download_github(url, data_dir, cancel_requested, on_progress).await?
        }
    };
    if cancel_requested.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("来源检查已取消".to_string());
    }
    let github_sha_unavailable =
        matches!(&source, SkillSource::Github { .. }) && source_details.commit_sha.is_none();
    let (skills, rejected, mut warnings) = discover_skills(&root, source.clone(), source_details)?;
    if github_sha_unavailable {
        warnings.push("GitHub API 未返回 commit SHA；本次来源仅按 ref 固定。".to_string());
    }
    Ok(SourceInspection {
        inspection_id: Uuid::new_v4().to_string(),
        source,
        skills,
        rejected,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn archive_with_symlink(symlink_path: &str) -> Cursor<Vec<u8>> {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut bytes);
            let options = SimpleFileOptions::default();
            writer
                .add_symlink(symlink_path, "README.md", options)
                .unwrap();
            writer
                .start_file("skills-main/skills/engineering/tdd/SKILL.md", options)
                .unwrap();
            writer
                .write_all(b"---\nname: tdd\ndescription: TDD\n---\n")
                .unwrap();
            writer
                .start_file("skills-main/skills/engineering/tdd/tests.md", options)
                .unwrap();
            writer.write_all(b"Test guidance").unwrap();
            writer.finish().unwrap();
        }
        bytes.set_position(0);
        bytes
    }

    #[test]
    fn parses_tree_and_skill_urls() {
        let root = parse_github_url("https://github.com/acme/skills").unwrap();
        assert_eq!(root.reference, "HEAD");
        assert_eq!(root.subpath, "");
        let tree = parse_github_url("https://github.com/acme/skills/tree/v1/demo").unwrap();
        assert_eq!(tree.subpath, "demo");
        assert_eq!(tree.reference, "v1");
        let pinned_root =
            parse_github_url("https://github.com/acme/skills/tree/0123456789abcdef").unwrap();
        assert_eq!(pinned_root.reference, "0123456789abcdef");
        assert_eq!(pinned_root.subpath, "");
        let file =
            parse_github_url("https://github.com/acme/skills/blob/main/demo/SKILL.md").unwrap();
        assert_eq!(file.subpath, "demo");
    }

    #[test]
    fn parses_github_shorthand_refs_commits_and_exact_paths() {
        let root = parse_github_url("acme/skills").unwrap();
        assert_eq!(root.owner, "acme");
        assert_eq!(root.repository, "skills");
        assert_eq!(root.reference, "HEAD");
        assert_eq!(root.subpath, "");

        let shorthand =
            parse_github_url("acme/skills@feature/agent:catalog/demo/SKILL.md").unwrap();
        assert_eq!(shorthand.reference, "feature/agent");
        assert_eq!(shorthand.subpath, "catalog/demo");

        let commit =
            parse_github_url("https://github.com/acme/skills/commit/0123456789abcdef").unwrap();
        assert_eq!(commit.reference, "0123456789abcdef");
        assert_eq!(commit.subpath, "");

        let query =
            parse_github_url("https://github.com/acme/skills?ref=v2&path=catalog/demo/SKILL.md")
                .unwrap();
        assert_eq!(query.reference, "v2");
        assert_eq!(query.subpath, "catalog/demo");
    }

    #[test]
    fn rejects_unsafe_github_shorthand_components() {
        assert!(parse_github_url("../skills").is_err());
        assert!(parse_github_url("acme/skills#main").is_err());
        assert!(parse_github_url("acme/skills@../main").is_err());
        assert!(parse_github_url("acme/skills@release+candidate").is_err());
        assert!(parse_github_url("acme/skills@main:../demo").is_err());
    }

    #[test]
    fn validates_frontmatter_and_directory_name() {
        let parent = tempfile::tempdir().unwrap();
        let skill = parent.path().join("demo-skill");
        fs::create_dir(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Demo\n---\nInstructions",
        )
        .unwrap();
        assert!(validate_skill(
            &skill,
            ".".to_string(),
            SkillSource::LocalDirectory {
                path: skill.display().to_string()
            },
            SkillSourceDetails::default(),
        )
        .is_ok());
    }

    #[test]
    fn preserves_optional_agent_skills_frontmatter_fields() {
        let parent = tempfile::tempdir().unwrap();
        let skill = parent.path().join("metadata-skill");
        fs::create_dir(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: metadata-skill\ndescription: Metadata\nlicense: MIT\ncompatibility: Requires git\nmetadata:\n  author: example\nallowed-tools: Bash(git:*)\n---\n",
        )
        .unwrap();

        let metadata = validate_skill(
            &skill,
            ".".to_string(),
            SkillSource::LocalDirectory {
                path: skill.display().to_string(),
            },
            SkillSourceDetails::default(),
        )
        .unwrap();

        assert_eq!(metadata.license.as_deref(), Some("MIT"));
        assert_eq!(metadata.compatibility.as_deref(), Some("Requires git"));
        assert_eq!(metadata.allowed_tools.as_deref(), Some("Bash(git:*)"));
        assert_eq!(
            metadata
                .metadata
                .as_ref()
                .and_then(|value| value["author"].as_str()),
            Some("example")
        );
    }

    #[test]
    fn local_directory_discovers_multiple_skills_and_keeps_rejections() {
        let root = tempfile::tempdir().unwrap();
        let valid = root.path().join("skills").join("valid-skill");
        let invalid = root.path().join("skills").join("invalid-skill");
        fs::create_dir_all(&valid).unwrap();
        fs::create_dir_all(&invalid).unwrap();
        fs::write(
            valid.join("SKILL.md"),
            "---\nname: valid-skill\ndescription: Valid\n---\nInstructions",
        )
        .unwrap();
        fs::write(
            invalid.join("SKILL.md"),
            "---\nname: different-name\ndescription: Invalid\n---\nInstructions",
        )
        .unwrap();

        let inspection = tauri::async_runtime::block_on(inspect_source(
            SkillSource::LocalDirectory {
                path: root.path().display().to_string(),
            },
            root.path(),
        ))
        .unwrap();

        assert_eq!(inspection.skills.len(), 1);
        assert_eq!(inspection.skills[0].name, "valid-skill");
        assert_eq!(inspection.rejected.len(), 1);
        assert_eq!(inspection.rejected[0].relative_path, "skills/invalid-skill");
    }

    #[test]
    fn local_directory_uses_an_immutable_preview_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("demo");
        let data = tempfile::tempdir().unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Original\n---\noriginal",
        )
        .unwrap();

        let inspection = tauri::async_runtime::block_on(inspect_source(
            SkillSource::LocalDirectory {
                path: source.display().to_string(),
            },
            data.path(),
        ))
        .unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Mutated\n---\nmutated",
        )
        .unwrap();

        let metadata = &inspection.skills[0];
        assert_ne!(PathBuf::from(&metadata.prepared_path), source);
        assert!(
            fs::read_to_string(Path::new(&metadata.prepared_path).join("SKILL.md"))
                .unwrap()
                .contains("Original")
        );
    }

    #[test]
    fn local_zip_discovers_skills_across_multiple_roots() {
        let root = tempfile::tempdir().unwrap();
        let archive_path = root.path().join("skills.zip");
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut bytes);
            let options = SimpleFileOptions::default();
            for (path, name) in [
                ("first/SKILL.md", "first"),
                ("nested/second/SKILL.md", "second"),
            ] {
                writer.start_file(path, options).unwrap();
                writer
                    .write_all(format!("---\nname: {name}\ndescription: {name}\n---\n").as_bytes())
                    .unwrap();
            }
            writer.start_file("__MACOSX/.DS_Store", options).unwrap();
            writer.write_all(b"ignored").unwrap();
            writer.finish().unwrap();
        }
        fs::write(&archive_path, bytes.into_inner()).unwrap();

        let inspection = tauri::async_runtime::block_on(inspect_source(
            SkillSource::LocalArchive {
                path: archive_path.display().to_string(),
            },
            root.path(),
        ))
        .unwrap();

        assert_eq!(
            inspection
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
    }

    #[test]
    fn discovered_skill_keeps_the_github_parent_subpath() {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("nested").join("demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();
        let details = SkillSourceDetails {
            subpath: Some("catalog".to_string()),
            ..SkillSourceDetails::default()
        };

        let (skills, _, _) = discover_skills(
            root.path(),
            SkillSource::Github {
                url: "https://github.com/acme/skills/tree/main/catalog".to_string(),
            },
            details,
        )
        .unwrap();

        assert_eq!(
            skills[0].source_details.subpath.as_deref(),
            Some("catalog/nested/demo")
        );
    }

    #[test]
    fn local_zip_rejects_path_traversal() {
        let root = tempfile::tempdir().unwrap();
        let archive_path = root.path().join("unsafe.zip");
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut bytes);
            let options = SimpleFileOptions::default();
            writer.start_file("../escape/SKILL.md", options).unwrap();
            writer
                .write_all(b"---\nname: escape\ndescription: Escape\n---\n")
                .unwrap();
            writer.finish().unwrap();
        }
        fs::write(&archive_path, bytes.into_inner()).unwrap();

        let error = tauri::async_runtime::block_on(inspect_source(
            SkillSource::LocalArchive {
                path: archive_path.display().to_string(),
            },
            root.path(),
        ))
        .unwrap_err();

        assert!(error.contains("路径穿越"));
        assert!(!root.path().join("escape").exists());
    }

    #[test]
    fn local_zip_rejects_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let archive_path = root.path().join("unsafe.zip");
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut bytes);
            writer
                .add_symlink("demo/link", "../../outside", SimpleFileOptions::default())
                .unwrap();
            writer.finish().unwrap();
        }
        fs::write(&archive_path, bytes.into_inner()).unwrap();

        let error = tauri::async_runtime::block_on(inspect_source(
            SkillSource::LocalArchive {
                path: archive_path.display().to_string(),
            },
            root.path(),
        ))
        .unwrap_err();

        assert_eq!(error, "ZIP 包含软链接，已拒绝");
    }

    #[test]
    fn ignores_symlinks_outside_selected_skill() {
        let destination = tempfile::tempdir().unwrap();
        let bytes = archive_with_symlink("skills-main/AGENTS.md");
        let mut archive = ZipArchive::new(bytes).unwrap();

        let extracted = extract_skill_archive(
            &mut archive,
            destination.path(),
            "skills/engineering/tdd",
            "skills",
        )
        .unwrap();

        assert_eq!(extracted.file_name().unwrap(), "tdd");
        assert!(extracted.join("SKILL.md").is_file());
        assert!(!destination.path().join("AGENTS.md").exists());
    }

    #[test]
    fn github_archives_skip_symlinks_inside_selected_paths() {
        let destination = tempfile::tempdir().unwrap();
        let bytes = archive_with_symlink("skills-main/skills/engineering/tdd/linked.md");
        let mut archive = ZipArchive::new(bytes).unwrap();

        let extracted = extract_skill_archive(
            &mut archive,
            destination.path(),
            "skills/engineering/tdd",
            "skills",
        )
        .unwrap();

        assert!(extracted.join("SKILL.md").is_file());
        assert!(!extracted.join("linked.md").exists());
    }

    #[test]
    fn github_ref_is_pinned_to_resolved_commit_before_archive_download() {
        let data = tempfile::tempdir().unwrap();
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut bytes);
            writer
                .start_file(
                    "skills-main/skills/demo/SKILL.md",
                    SimpleFileOptions::default(),
                )
                .unwrap();
            writer
                .write_all(b"---\nname: demo\ndescription: Demo\n---\n")
                .unwrap();
            writer.finish().unwrap();
        }
        let archive = bytes.into_inner();
        let observed_url = Arc::new(Mutex::new(String::new()));
        let observed_for_download = Arc::clone(&observed_url);
        let archive_for_download = archive.clone();
        let (path, details) = tauri::async_runtime::block_on(download_github_with_transport(
            "acme/skills@main",
            data.path(),
            None,
            None,
            |_url| async { Ok(Some("deadbeef0123456789".to_string())) },
            move |url| {
                let observed_for_download = Arc::clone(&observed_for_download);
                let archive_for_download = archive_for_download.clone();
                async move {
                    *observed_for_download.lock().unwrap() = url;
                    Ok(archive_for_download)
                }
            },
        ))
        .unwrap();

        assert_eq!(details.commit_sha.as_deref(), Some("deadbeef0123456789"));
        assert_eq!(
            observed_url.lock().unwrap().as_str(),
            "https://codeload.github.com/acme/skills/zip/deadbeef0123456789"
        );
        assert!(path.join("skills/demo/SKILL.md").is_file());
    }

    #[test]
    fn github_download_honors_cancellation_after_a_slow_transport() {
        let data = tempfile::tempdir().unwrap();
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut bytes);
            writer
                .start_file("skills-main/demo/SKILL.md", SimpleFileOptions::default())
                .unwrap();
            writer
                .write_all(b"---\nname: demo\ndescription: Demo\n---\n")
                .unwrap();
            writer.finish().unwrap();
        }
        let archive = bytes.into_inner();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_for_transport = Arc::clone(&cancelled);
        let result = tauri::async_runtime::block_on(download_github_with_transport(
            "acme/skills@main",
            data.path(),
            Some(cancelled.as_ref()),
            None,
            |_url| async { Ok(Some("0123456789abcdef".to_string())) },
            move |_url| {
                cancel_for_transport.store(true, Ordering::Release);
                let archive = archive.clone();
                async move { Ok(archive) }
            },
        ));

        let error = result.expect_err("cancellation should stop before extraction");
        assert!(error.contains("已取消"));
        assert!(fs::read_dir(data.path().join("cache"))
            .map(|entries| entries.count() == 0)
            .unwrap_or(true));
    }

    #[test]
    #[ignore = "requires public GitHub access"]
    fn downloads_tdd_skill_from_repository_with_root_symlink() {
        let data = tempfile::tempdir().unwrap();
        let source = SkillSource::Github {
            url: "https://github.com/mattpocock/skills/tree/main/skills/engineering/tdd"
                .to_string(),
        };

        let (metadata, path) =
            tauri::async_runtime::block_on(inspect(source, data.path())).unwrap();

        assert_eq!(metadata.name, "tdd");
        assert!(path.join("SKILL.md").is_file());
        assert!(!path.join("AGENTS.md").exists());
    }

    #[test]
    #[ignore = "requires public GitHub access"]
    fn discovers_multiple_skills_from_a_public_repository_root() {
        let data = tempfile::tempdir().unwrap();
        let source = SkillSource::Github {
            url: "https://github.com/mattpocock/skills".to_string(),
        };

        let inspection =
            tauri::async_runtime::block_on(inspect_source(source, data.path())).unwrap();

        assert!(inspection.skills.len() > 1);
        assert!(inspection.skills.iter().any(|skill| skill.name == "tdd"));
        assert!(inspection
            .skills
            .iter()
            .all(|skill| skill.source_details.reference.as_deref() == Some("HEAD")));
    }
}
