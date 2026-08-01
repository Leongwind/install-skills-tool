use crate::domain::{
    RejectedSkill, SkillMetadata, SkillSource, SkillSourceDetails, SourceInspection,
};
use crate::storage::{cache_dir, cleanup_cache, inspect_tree, MAX_BYTES, MAX_FILES};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read, Seek};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::ZipArchive;

const MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Deserialize)]
struct Frontmatter {
    name: String,
    description: String,
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
    let url = url::Url::parse(raw).map_err(|_| "GitHub URL 无效".to_string())?;
    if url.host_str() != Some("github.com") {
        return Err("仅支持 github.com 公共仓库".to_string());
    }
    let parts = url
        .path_segments()
        .ok_or_else(|| "GitHub URL 缺少路径".to_string())?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err("GitHub URL 需要 owner/repository".to_string());
    }
    let (reference, mut subpath) = if parts.len() >= 5 && matches!(parts[2], "tree" | "blob") {
        (parts[3].to_string(), parts[4..].join("/"))
    } else {
        ("HEAD".to_string(), String::new())
    };
    if subpath == "SKILL.md" {
        subpath.clear();
    } else if subpath.ends_with("/SKILL.md") {
        subpath.truncate(subpath.len() - "/SKILL.md".len());
    }
    Ok(GithubLocation {
        owner: parts[0].to_string(),
        repository: parts[1].trim_end_matches(".git").to_string(),
        reference,
        subpath,
    })
}

fn relative_text(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| ".".to_string())
}

fn ignored(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git" | "node_modules" | "target" | "__MACOSX" | ".DS_Store"
            )
        })
}

fn validate_skill(
    path: &Path,
    relative_path: String,
    source: SkillSource,
    mut details: SkillSourceDetails,
) -> Result<SkillMetadata, String> {
    let raw = fs::read_to_string(path.join("SKILL.md"))
        .map_err(|error| format!("缺少或无法读取 SKILL.md: {error}"))?;
    let body = raw
        .strip_prefix("---")
        .ok_or_else(|| "SKILL.md 缺少 YAML frontmatter".to_string())?;
    let end = body
        .find("\n---")
        .ok_or_else(|| "SKILL.md frontmatter 未结束".to_string())?;
    let frontmatter: Frontmatter =
        serde_yaml::from_str(&body[..end]).map_err(|error| format!("YAML 无效: {error}"))?;
    let pattern = Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("valid regex");
    if frontmatter.name.len() > 64 || !pattern.is_match(&frontmatter.name) {
        return Err("name 必须由小写字母、数字和单连字符组成，且不超过 64 字符".to_string());
    }
    let folder = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Skill 目录名无效".to_string())?;
    if folder != frontmatter.name {
        return Err(format!(
            "Skill name {} 必须与目录名 {} 一致",
            frontmatter.name, folder
        ));
    }
    if frontmatter.description.trim().is_empty() || frontmatter.description.len() > 1024 {
        return Err("description 不能为空且不能超过 1,024 字符".to_string());
    }
    let (content_hash, file_count, total_bytes, has_scripts) = inspect_tree(path)?;
    if details.subpath.is_none() {
        details.subpath = Some(relative_path.clone());
    }
    let warnings = has_scripts
        .then(|| "Skill 包含脚本或可执行文件，安装器不会执行它们。".to_string())
        .into_iter()
        .collect();
    Ok(SkillMetadata {
        skill_id: Uuid::new_v4().to_string(),
        relative_path,
        name: frontmatter.name,
        description: frontmatter.description,
        source,
        source_details: details,
        prepared_path: path.display().to_string(),
        content_hash,
        file_count,
        total_bytes,
        has_scripts,
        warnings,
    })
}

fn discover_skills(
    root: &Path,
    source: SkillSource,
    details: SkillSourceDetails,
) -> Result<DiscoveryResult, String> {
    if !root.is_dir() {
        return Err("Skill 来源必须是目录".to_string());
    }
    let mut walker = WalkDir::new(root).follow_links(false).into_iter();
    let mut skills = Vec::new();
    let mut rejected = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    let mut visited = 0usize;
    while let Some(item) = walker.next() {
        let item = item.map_err(|error| error.to_string())?;
        visited += 1;
        if visited > MAX_FILES {
            return Err(format!("来源中的文件和目录超过限制 {MAX_FILES}"));
        }
        if ignored(item.path()) {
            if item.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        if !item.file_type().is_dir() || !item.path().join("SKILL.md").is_file() {
            continue;
        }
        let relative = relative_text(root, item.path());
        let mut skill_details = details.clone();
        skill_details.subpath = Some(
            [details.subpath.as_deref().unwrap_or_default(), &relative]
                .into_iter()
                .filter(|part| !part.is_empty() && *part != ".")
                .collect::<Vec<_>>()
                .join("/"),
        );
        match validate_skill(item.path(), relative.clone(), source.clone(), skill_details) {
            Ok(skill) => {
                if seen.insert((skill.name.clone(), skill.content_hash.clone())) {
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

fn safe_archive_path(path: &Path) -> Result<(), String> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("ZIP 包含绝对路径或路径穿越".to_string());
    }
    let text = path
        .to_str()
        .ok_or_else(|| "ZIP 包含非 UTF-8 文件名".to_string())?;
    if text.chars().any(char::is_control) {
        return Err("ZIP 包含控制字符文件名".to_string());
    }
    Ok(())
}

fn extract_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    destination: &Path,
    strip_root: bool,
) -> Result<PathBuf, String> {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let root_component = if strip_root {
        let first = archive
            .by_index(0)
            .map_err(|error| error.to_string())?
            .enclosed_name()
            .and_then(|path| match path.components().next() {
                Some(Component::Normal(value)) => Some(value.to_os_string()),
                _ => None,
            })
            .ok_or_else(|| "GitHub ZIP 根目录无效".to_string())?;
        Some(root_component_path(first))
    } else {
        None
    };
    for index in 0..archive.len() {
        let mut item = archive.by_index(index).map_err(|error| error.to_string())?;
        let enclosed = item
            .enclosed_name()
            .ok_or_else(|| "ZIP 包含绝对路径或路径穿越".to_string())?
            .to_path_buf();
        safe_archive_path(&enclosed)?;
        if item
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("ZIP 包含软链接，已拒绝".to_string());
        }
        let relative = match &root_component {
            Some(root) => enclosed
                .strip_prefix(root)
                .map_err(|_| "GitHub ZIP 包含多个根目录".to_string())?,
            None => enclosed.as_path(),
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        if item.is_dir() {
            fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        files += 1;
        bytes = bytes.saturating_add(item.size());
        if files > MAX_FILES || bytes > MAX_BYTES {
            return Err("ZIP 展开后超过 5,000 个文件或 200 MB".to_string());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = fs::File::create(target).map_err(|error| error.to_string())?;
        std::io::copy(&mut item, &mut output).map_err(|error| error.to_string())?;
    }
    if files == 0 {
        return Err("ZIP 为空".to_string());
    }
    Ok(destination.to_path_buf())
}

fn root_component_path(component: std::ffi::OsString) -> PathBuf {
    PathBuf::from(component)
}

fn extract_local_archive(path: &Path, data_dir: &Path) -> Result<PathBuf, String> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("zip"))
    {
        return Err("首版仅支持本地 ZIP 压缩包".to_string());
    }
    if fs::metadata(path).map_err(|error| error.to_string())?.len() > MAX_ARCHIVE_BYTES {
        return Err("压缩包超过 50 MB".to_string());
    }
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| format!("ZIP 无效: {error}"))?;
    let destination = cache_dir(data_dir).join(Uuid::new_v4().to_string());
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    extract_archive(&mut archive, &destination, false)
}

async fn download_github(
    raw: &str,
    data_dir: &Path,
) -> Result<(PathBuf, SkillSourceDetails), String> {
    let location = parse_github_url(raw)?;
    let client = reqwest::Client::builder()
        .user_agent("Skill-Installer-Windows/0.1")
        .build()
        .map_err(|error| error.to_string())?;
    let url = format!(
        "https://codeload.github.com/{}/{}/zip/{}",
        location.owner, location.repository, location.reference
    );
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
        .is_some_and(|length| length > MAX_ARCHIVE_BYTES)
    {
        return Err("下载内容超过 50 MB".to_string());
    }
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err("下载内容超过 50 MB".to_string());
    }
    let destination = cache_dir(data_dir).join(Uuid::new_v4().to_string());
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    extract_archive(&mut archive, &destination, true)?;
    let selected = if location.subpath.is_empty() {
        destination
    } else {
        destination.join(&location.subpath)
    };
    Ok((
        selected,
        SkillSourceDetails {
            owner: Some(location.owner),
            repository: Some(location.repository),
            reference: Some(location.reference),
            subpath: Some(location.subpath),
            ..SkillSourceDetails::default()
        },
    ))
}

pub async fn inspect_source(
    source: SkillSource,
    data_dir: &Path,
) -> Result<SourceInspection, String> {
    cleanup_cache(data_dir, Duration::from_secs(24 * 60 * 60))?;
    let (root, details) = match &source {
        SkillSource::LocalDirectory { path } => {
            let canonical = PathBuf::from(path)
                .canonicalize()
                .map_err(|error| format!("本地路径无效: {error}"))?;
            (
                canonical.clone(),
                SkillSourceDetails {
                    local_path: Some(canonical.display().to_string()),
                    ..SkillSourceDetails::default()
                },
            )
        }
        SkillSource::LocalArchive { path } => (
            extract_local_archive(Path::new(path), data_dir)?,
            SkillSourceDetails {
                archive_path: Some(path.clone()),
                ..SkillSourceDetails::default()
            },
        ),
        SkillSource::Github { url } => download_github(url, data_dir).await?,
    };
    inspect_prepared(source, root, details)
}

fn inspect_prepared(
    source: SkillSource,
    root: PathBuf,
    details: SkillSourceDetails,
) -> Result<SourceInspection, String> {
    let (skills, rejected, warnings) = discover_skills(&root, source.clone(), details)?;
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
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn write_skill(root: &Path, name: &str, valid: bool) {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        let declared = if valid { name } else { "wrong-name" };
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {declared}\ndescription: Fixture\n---\n"),
        )
        .unwrap();
    }

    #[test]
    fn local_directory_discovers_valid_and_rejected_skills_together() {
        let root = tempfile::tempdir().unwrap();
        write_skill(root.path(), "one", true);
        write_skill(root.path(), "two", false);
        let source = SkillSource::LocalDirectory {
            path: root.path().display().to_string(),
        };
        let result = inspect_prepared(
            source,
            root.path().to_path_buf(),
            SkillSourceDetails::default(),
        )
        .unwrap();
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.rejected.len(), 1);
    }

    #[test]
    fn local_zip_discovers_multiple_roots_and_ignores_macos_metadata() {
        let root = tempfile::tempdir().unwrap();
        let archive_path = root.path().join("skills.zip");
        let file = fs::File::create(&archive_path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for name in ["one", "two"] {
            writer
                .start_file(format!("{name}/SKILL.md"), options)
                .unwrap();
            writer
                .write_all(format!("---\nname: {name}\ndescription: Fixture\n---\n").as_bytes())
                .unwrap();
        }
        writer.start_file("__MACOSX/junk", options).unwrap();
        writer.write_all(b"junk").unwrap();
        writer.finish().unwrap();
        let source = SkillSource::LocalArchive {
            path: archive_path.display().to_string(),
        };
        let extracted = extract_local_archive(&archive_path, root.path()).unwrap();
        let result = inspect_prepared(source, extracted, SkillSourceDetails::default()).unwrap();
        assert_eq!(result.skills.len(), 2);
    }

    #[test]
    fn github_url_supports_root_subdirectory_and_manifest() {
        let root = parse_github_url("https://github.com/acme/skills.git").unwrap();
        assert_eq!(root.reference, "HEAD");
        let manifest =
            parse_github_url("https://github.com/acme/skills/blob/dev/tools/review/SKILL.md")
                .unwrap();
        assert_eq!(manifest.reference, "dev");
        assert_eq!(manifest.subpath, "tools/review");
    }

    #[test]
    fn archive_paths_reject_parent_components() {
        assert!(safe_archive_path(Path::new("../escape/SKILL.md")).is_err());
        assert!(safe_archive_path(Path::new("safe/SKILL.md")).is_ok());
    }
}
