use crate::domain::{BackupRecord, PersistedState};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use uuid::Uuid;
use walkdir::WalkDir;

pub const MAX_FILES: usize = 5_000;
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;

pub fn load_state(data_dir: &Path) -> Result<PersistedState, String> {
    let path = data_dir.join("state.json");
    if !path.exists() {
        return Ok(PersistedState::default());
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let mut state: PersistedState =
        serde_json::from_slice(&bytes).map_err(|error| format!("状态文件无效: {error}"))?;
    let original_schema = state.schema_version;
    if state.schema_version == 1 {
        for installation in &mut state.installations {
            installation.legacy_project =
                installation.scope == crate::domain::InstallScope::Project;
            installation.provenance = crate::domain::InstallationProvenance::Tool;
        }
        state.schema_version = 2;
    }
    if state.schema_version == 2 {
        state.schema_version = 3;
    }
    let mut changed = original_schema != state.schema_version;
    for journal in &mut state.operation_journals {
        if matches!(
            journal.status,
            crate::domain::OperationJournalStatus::Preparing
                | crate::domain::OperationJournalStatus::Applying
                | crate::domain::OperationJournalStatus::Partial
        ) {
            journal.status = crate::domain::OperationJournalStatus::RecoveryRequired;
            changed = true;
        }
    }
    if changed {
        save_state(data_dir, &state)?;
    }
    Ok(state)
}

pub fn save_state(data_dir: &Path, state: &PersistedState) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let target = data_dir.join("state.json");
    let temporary = data_dir.join(format!(".state-{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, &target).map_err(|error| error.to_string())
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
        let path = item.path();
        if path == canonical_root {
            continue;
        }
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            let resolved = path
                .canonicalize()
                .map_err(|error| format!("无效软链接 {}: {error}", path.display()))?;
            if !resolved.starts_with(&canonical_root) {
                return Err(format!("拒绝逃逸软链接: {}", path.display()));
            }
            return Err(format!("第一阶段不接受软链接: {}", path.display()));
        }
        let relative = path
            .strip_prefix(&canonical_root)
            .map_err(|error| error.to_string())?;
        if relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        }) {
            return Err("拒绝路径穿越".to_string());
        }
        let relative_text = relative
            .to_str()
            .ok_or_else(|| format!("拒绝非 UTF-8 文件名: {}", relative.display()))?;
        if relative_text.chars().any(char::is_control) {
            return Err(format!("拒绝含控制字符的文件名: {relative_text:?}"));
        }
        if metadata.is_file() {
            count += 1;
            bytes = bytes.saturating_add(metadata.len());
            if count > MAX_FILES {
                return Err(format!("文件数量超过限制 {MAX_FILES}"));
            }
            if bytes > MAX_BYTES {
                return Err("解压内容超过 200 MB".to_string());
            }
            let relative_text = relative_text.replace('\\', "/");
            has_scripts |= relative_text.starts_with("scripts/");
            #[cfg(unix)]
            {
                has_scripts |= metadata.permissions().mode() & 0o111 != 0;
            }
            entries.push((relative_text, path.to_path_buf()));
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
        let target = destination.join(relative);
        if item.file_type().is_dir() {
            fs::create_dir_all(target).map_err(|error| error.to_string())?;
        } else if item.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::copy(item.path(), target).map_err(|error| error.to_string())?;
        } else {
            return Err(format!("拒绝复制软链接: {}", item.path().display()));
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
    let had_destination = destination.exists();
    if had_destination {
        fs::rename(destination, &displaced).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(&temporary, destination) {
        if had_destination {
            let _ = fs::rename(&displaced, destination);
        }
        return Err(error.to_string());
    }
    if had_destination {
        if displaced.is_dir() {
            fs::remove_dir_all(displaced).map_err(|error| error.to_string())?;
        } else {
            fs::remove_file(displaced).map_err(|error| error.to_string())?;
        }
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

pub fn redact_home(input: &str) -> String {
    std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .map(|home| input.replace(&home, "~"))
        .unwrap_or_else(|| input.to_string())
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
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
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
    fn hashing_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SKILL.md"), "---\nname: demo\n---").unwrap();
        let first = inspect_tree(dir.path()).unwrap().0;
        let second = inspect_tree(dir.path()).unwrap().0;
        assert_eq!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_escaping_symlink() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        symlink("/tmp", dir.path().join("escape")).unwrap();
        assert!(inspect_tree(dir.path()).is_err());
    }

    #[test]
    fn replacement_and_backup_preserve_complete_trees() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        let data = root.path().join("data");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("SKILL.md"), "new").unwrap();
        fs::write(target.join("SKILL.md"), "old").unwrap();

        let backup = create_backup(&data, &target).unwrap().unwrap();
        atomic_replace(&source, &target).unwrap();
        assert_eq!(fs::read_to_string(target.join("SKILL.md")).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(Path::new(&backup.backup_path).join("SKILL.md")).unwrap(),
            "old"
        );
    }

    #[test]
    fn loading_v1_state_preserves_global_and_legacy_project_records() {
        let data = tempfile::tempdir().unwrap();
        fs::write(
            data.path().join("state.json"),
            r#"{
              "schemaVersion": 1,
              "installations": [
                {
                  "id": "global",
                  "skillName": "one",
                  "resolvedPath": "/tmp/global/one",
                  "source": {"kind": "local", "path": "/tmp/source/one"},
                  "contentHash": "abc",
                  "scope": "global",
                  "consumers": ["codex"],
                  "passiveConsumers": [],
                  "adapterVersion": 1,
                  "installedAt": "2026-01-01T00:00:00Z"
                },
                {
                  "id": "project",
                  "skillName": "two",
                  "resolvedPath": "/tmp/project/.agents/skills/two",
                  "source": {"kind": "github", "url": "https://github.com/example/skills"},
                  "contentHash": "def",
                  "scope": "project",
                  "consumers": ["codex"],
                  "passiveConsumers": [],
                  "adapterVersion": 1,
                  "installedAt": "2026-01-01T00:00:00Z"
                }
              ],
              "backups": []
            }"#,
        )
        .unwrap();

        let state = load_state(data.path()).unwrap();

        assert_eq!(state.schema_version, 3);
        assert!(!state.installations[0].legacy_project);
        assert!(state.installations[1].legacy_project);
        assert_eq!(
            state.installations[0].provenance,
            crate::domain::InstallationProvenance::Tool
        );
    }

    #[test]
    fn loading_v2_state_adds_v3_recovery_defaults() {
        let data = tempfile::tempdir().unwrap();
        fs::write(
            data.path().join("state.json"),
            r#"{
              "schemaVersion": 2,
              "installations": [],
              "backups": []
            }"#,
        )
        .unwrap();

        let state = load_state(data.path()).unwrap();

        assert_eq!(state.schema_version, 3);
        assert!(state.operation_journals.is_empty());
        assert_eq!(state.backup_policy.max_backups_per_skill, 5);
    }
}
