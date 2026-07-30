use crate::domain::{SkillMetadata, SkillSource, SkillSourceDetails};
use crate::storage::{cache_dir, inspect_tree, MAX_BYTES, MAX_FILES};
use regex::Regex;
use serde::Deserialize;
use std::fs;
use std::io::{Cursor, Read, Seek};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use zip::ZipArchive;

const MAX_DOWNLOAD: u64 = 50 * 1024 * 1024;

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

fn parse_github_url(raw: &str) -> Result<GithubLocation, String> {
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
    let repository = parts[1].trim_end_matches(".git").to_string();
    let (reference, mut subpath) = if parts.len() >= 5 && (parts[2] == "tree" || parts[2] == "blob")
    {
        (parts[3].to_string(), parts[4..].join("/"))
    } else {
        ("main".to_string(), String::new())
    };
    if subpath.ends_with("/SKILL.md") {
        subpath.truncate(subpath.len() - "/SKILL.md".len());
    } else if subpath == "SKILL.md" {
        subpath.clear();
    }
    Ok(GithubLocation {
        owner: parts[0].to_string(),
        repository,
        reference,
        subpath,
    })
}

fn validate_skill(
    path: &Path,
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
        name: frontmatter.name,
        description: frontmatter.description,
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
            return Err("Skill 包含软链接，已拒绝".to_string());
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

async fn download_github(
    raw: &str,
    data_dir: &Path,
) -> Result<(PathBuf, SkillSourceDetails), String> {
    let location = parse_github_url(raw)?;
    let download_url = format!(
        "https://codeload.github.com/{}/{}/zip/{}",
        location.owner, location.repository, location.reference
    );
    let client = reqwest::Client::builder()
        .user_agent("Skill-Installer/0.1")
        .build()
        .map_err(|error| error.to_string())?;
    let commit_sha = client
        .get(format!(
            "https://api.github.com/repos/{}/{}/commits/{}",
            location.owner, location.repository, location.reference
        ))
        .send()
        .await
        .ok()
        .filter(|response| response.status().is_success());
    let commit_sha = match commit_sha {
        Some(response) => response
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|json| {
                json.get("sha")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            }),
        None => None,
    };
    let response = client
        .get(download_url)
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
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取下载内容失败: {error}"))?;
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err("下载内容超过 50 MB".to_string());
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
        },
    ))
}

pub async fn inspect(
    source: SkillSource,
    data_dir: &Path,
) -> Result<(SkillMetadata, PathBuf), String> {
    let (path, source_details) = match &source {
        SkillSource::Local { path } => {
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
        SkillSource::Github { url } => download_github(url, data_dir).await?,
    };
    let metadata = validate_skill(&path, source, source_details)?;
    Ok((metadata, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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
        let tree = parse_github_url("https://github.com/acme/skills/tree/v1/demo").unwrap();
        assert_eq!(tree.subpath, "demo");
        assert_eq!(tree.reference, "v1");
        let file =
            parse_github_url("https://github.com/acme/skills/blob/main/demo/SKILL.md").unwrap();
        assert_eq!(file.subpath, "demo");
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
            SkillSource::Local {
                path: skill.display().to_string()
            },
            SkillSourceDetails::default(),
        )
        .is_ok());
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
    fn rejects_symlinks_inside_selected_skill() {
        let destination = tempfile::tempdir().unwrap();
        let bytes = archive_with_symlink("skills-main/skills/engineering/tdd/linked.md");
        let mut archive = ZipArchive::new(bytes).unwrap();

        let error = extract_skill_archive(
            &mut archive,
            destination.path(),
            "skills/engineering/tdd",
            "skills",
        )
        .unwrap_err();

        assert_eq!(error, "Skill 包含软链接，已拒绝");
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
}
