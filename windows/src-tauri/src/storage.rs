use crate::domain::{BackupRecord, PersistedState};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use uuid::Uuid;
use walkdir::WalkDir;

pub const MAX_FILES: usize = 5_000;
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;

fn link_like(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    false
}

pub fn load_state(data_dir: &Path) -> Result<PersistedState, String> {
    let path = data_dir.join("state.json");
    if !path.exists() {
        return Ok(PersistedState::default());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| format!("状态文件无效: {error}"))
}

pub fn save_state(data_dir: &Path, state: &PersistedState) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let target = data_dir.join("state.json");
    let temporary = data_dir.join(format!(".state-{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    if target.exists() {
        fs::remove_file(&target).map_err(|error| error.to_string())?;
    }
    fs::rename(temporary, target).map_err(|error| error.to_string())
}

pub fn inspect_tree(root: &Path) -> Result<(String, usize, u64, bool), String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("无法读取 Skill 目录: {error}"))?;
    let mut entries = Vec::new();
    let mut count = 0usize;
    let mut bytes = 0u64;
    let mut has_scripts = false;
    for item in WalkDir::new(&canonical_root).follow_links(false) {
        let item = item.map_err(|error| error.to_string())?;
        if item.path() == canonical_root {
            continue;
        }
        let metadata = fs::symlink_metadata(item.path()).map_err(|error| error.to_string())?;
        if link_like(&metadata) {
            return Err(format!("拒绝软链接或 junction: {}", item.path().display()));
        }
        let relative = item
            .path()
            .strip_prefix(&canonical_root)
            .map_err(|error| error.to_string())?;
        let relative_text = relative
            .to_str()
            .ok_or_else(|| format!("拒绝非 UTF-8 文件名: {}", relative.display()))?;
        if relative_text.chars().any(char::is_control) {
            return Err("拒绝含控制字符的文件名".to_string());
        }
        if metadata.is_file() {
            count += 1;
            bytes = bytes.saturating_add(metadata.len());
            if count > MAX_FILES {
                return Err(format!("文件数量超过限制 {MAX_FILES}"));
            }
            if bytes > MAX_BYTES {
                return Err("内容超过 200 MB".to_string());
            }
            let relative_text = relative_text.replace('\\', "/");
            has_scripts |= relative_text.starts_with("scripts/")
                || [".exe", ".cmd", ".bat", ".ps1"]
                    .iter()
                    .any(|extension| relative_text.to_lowercase().ends_with(extension));
            entries.push((relative_text, item.path().to_path_buf()));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    for (relative, path) in entries {
        digest.update(relative.as_bytes());
        digest.update([0]);
        let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
    }
    Ok((hex::encode(digest.finalize()), count, bytes, has_scripts))
}

pub fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for item in WalkDir::new(source).follow_links(false) {
        let item = item.map_err(|error| error.to_string())?;
        let relative = item
            .path()
            .strip_prefix(source)
            .map_err(|error| error.to_string())?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(item.path()).map_err(|error| error.to_string())?;
        if link_like(&metadata) {
            return Err(format!(
                "拒绝复制软链接或 junction: {}",
                item.path().display()
            ));
        }
        let target = destination.join(relative);
        if metadata.is_dir() {
            fs::create_dir_all(target).map_err(|error| error.to_string())?;
        } else if metadata.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::copy(item.path(), target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "目标目录无父级".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".skill-installer-{}.tmp", Uuid::new_v4()));
    copy_tree(source, &temporary)?;
    let displaced = parent.join(format!(".skill-installer-{}.old", Uuid::new_v4()));
    let existed = destination.exists();
    if existed {
        fs::rename(destination, &displaced).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(&temporary, destination) {
        if existed {
            let _ = fs::rename(&displaced, destination);
        }
        return Err(error.to_string());
    }
    if existed {
        fs::remove_dir_all(displaced).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn create_backup(data_dir: &Path, source: &Path) -> Result<Option<BackupRecord>, String> {
    if !source.exists() {
        return Ok(None);
    }
    let id = Uuid::new_v4().to_string();
    let backup = data_dir.join("backups").join(&id);
    copy_tree(source, &backup)?;
    Ok(Some(BackupRecord {
        id,
        original_path: source.display().to_string(),
        backup_path: backup.display().to_string(),
        created_at: Utc::now().to_rfc3339(),
    }))
}

pub fn writable_target(path: &Path) -> bool {
    let mut cursor = path;
    while !cursor.exists() {
        let Some(parent) = cursor.parent() else {
            return false;
        };
        cursor = parent;
    }
    fs::metadata(cursor)
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

pub fn cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("cache")
}

pub fn cleanup_cache(data_dir: &Path, max_age: Duration) -> Result<(), String> {
    let root = cache_dir(data_dir);
    if !root.is_dir() {
        return Ok(());
    }
    let now = SystemTime::now();
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if now.duration_since(modified).unwrap_or_default() <= max_age {
            continue;
        }
        if metadata.is_dir() {
            fs::remove_dir_all(entry.path()).map_err(|error| error.to_string())?;
        } else {
            fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_hash_is_stable_and_marks_windows_scripts() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("scripts")).unwrap();
        fs::write(root.path().join("SKILL.md"), "fixture").unwrap();
        fs::write(root.path().join("scripts/setup.ps1"), "fixture").unwrap();
        let first = inspect_tree(root.path()).unwrap();
        let second = inspect_tree(root.path()).unwrap();
        assert_eq!(first.0, second.0);
        assert!(first.3);
    }

    #[cfg(unix)]
    #[test]
    fn tree_inspection_rejects_links_in_sandbox_fixture() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        symlink("/tmp", root.path().join("escape")).unwrap();
        assert!(inspect_tree(root.path()).unwrap_err().contains("junction"));
    }
}
