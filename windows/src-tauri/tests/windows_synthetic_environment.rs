#![cfg(windows)]

use skill_installer_windows_lib::adapters::adapters;
use skill_installer_windows_lib::domain::*;
use skill_installer_windows_lib::{inventory, operations, storage, windows};
use std::fs;
use std::path::{Path, PathBuf};

fn write_skill(root: &Path, id: &str, name: &str, body: &str) -> SkillMetadata {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Synthetic fixture\n---\n{body}"),
    )
    .unwrap();
    let (content_hash, file_count, total_bytes, has_scripts) =
        storage::inspect_tree(&path).unwrap();
    SkillMetadata {
        skill_id: id.to_string(),
        relative_path: name.to_string(),
        name: name.to_string(),
        description: "Synthetic fixture".to_string(),
        source: SkillSource::LocalDirectory {
            path: root.display().to_string(),
        },
        source_details: SkillSourceDetails::default(),
        prepared_path: path.display().to_string(),
        content_hash,
        file_count,
        total_bytes,
        has_scripts,
        warnings: Vec::new(),
    }
}

#[test]
fn synthetic_windows_environment_exercises_scan_assignment_install_inventory_and_lifecycle() {
    let fixture = tempfile::tempdir().unwrap();
    let profile = fixture.path().join("Users/tester");
    let local_appdata = profile.join("AppData/Local");
    let appdata = profile.join("AppData/Roaming");
    for (folder, executable) in [("Cursor", "Cursor.exe"), ("Kiro", "Kiro.exe")] {
        let path = local_appdata.join("Programs").join(folder).join(executable);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"synthetic executable fixture").unwrap();
    }
    let context = windows::ScanContext {
        user_profile: profile.clone(),
        appdata,
        local_appdata,
        program_files: vec![fixture.path().join("Program Files")],
        path_dirs: vec![fixture.path().join("bin")],
        registered_apps: vec![windows::RegisteredApplication {
            display_name: "Cursor".to_string(),
            display_version: Some("1.7.0".to_string()),
            install_location: Some(fixture.path().join("registry/Cursor")),
            display_icon: None,
        }],
    };
    let clients = windows::scan_clients_with_context(&context);
    let selected_clients = clients
        .iter()
        .filter(|client| matches!(client.id.as_str(), "cursor" | "kiro"))
        .cloned()
        .collect::<Vec<_>>();
    assert!(selected_clients.iter().all(|client| client.supports_skills));

    let source = fixture.path().join("source");
    let alpha = write_skill(&source, "alpha-id", "alpha", "alpha v1");
    let beta = write_skill(&source, "beta-id", "beta", "beta v1");
    let inspection = SourceInspection {
        inspection_id: "synthetic-inspection".to_string(),
        source: alpha.source.clone(),
        skills: vec![alpha.clone(), beta.clone()],
        rejected: Vec::new(),
        warnings: Vec::new(),
    };
    let plan = operations::build_install_plan(
        &inspection,
        &[
            SkillAssignment {
                skill_id: alpha.skill_id.clone(),
                client_ids: vec!["cursor".to_string(), "kiro".to_string()],
            },
            SkillAssignment {
                skill_id: beta.skill_id.clone(),
                client_ids: vec!["kiro".to_string()],
            },
        ],
        &selected_clients,
        &profile,
        &PersistedState::default(),
    )
    .unwrap();
    assert_eq!(plan.entries.len(), 3);

    let data_dir = fixture.path().join("state");
    let source_paths = [
        (alpha.skill_id.clone(), PathBuf::from(&alpha.prepared_path)),
        (beta.skill_id.clone(), PathBuf::from(&beta.prepared_path)),
    ]
    .into_iter()
    .collect();
    let results = operations::apply_install_plan(
        PendingPlan {
            public: plan,
            source_paths,
        },
        &[],
        &data_dir,
    )
    .unwrap();
    assert_eq!(results.iter().filter(|result| result.success).count(), 3);

    let state = storage::load_state(&data_dir).unwrap();
    let environment = inventory::build_environment_scan(selected_clients.clone(), &state);
    let cursor_inventory = environment
        .inventories
        .iter()
        .find(|item| item.client_id == "cursor")
        .unwrap();
    let kiro_inventory = environment
        .inventories
        .iter()
        .find(|item| item.client_id == "kiro")
        .unwrap();
    assert_eq!(cursor_inventory.direct_skills.len(), 1);
    assert_eq!(kiro_inventory.direct_skills.len(), 2);
    assert!(environment
        .inventories
        .iter()
        .flat_map(|item| &item.direct_skills)
        .all(|skill| skill.management_status == SkillManagementStatus::ToolManaged));

    let cursor_alpha = state
        .installations
        .iter()
        .find(|record| record.consumers == ["cursor"] && record.skill_name == "alpha")
        .unwrap();
    fs::write(
        Path::new(&cursor_alpha.resolved_path).join("manual.txt"),
        "manual edit",
    )
    .unwrap();
    let first = operations::uninstall_installation(&cursor_alpha.id, false, &data_dir).unwrap();
    assert_eq!(first.status, "confirmationRequired");
    let removed = operations::uninstall_installation(&cursor_alpha.id, true, &data_dir).unwrap();
    assert!(removed.success);
    let backup = storage::load_state(&data_dir).unwrap().backups[0].clone();
    assert!(
        operations::restore_backup(&backup.id, &data_dir)
            .unwrap()
            .success
    );
    assert!(Path::new(&backup.original_path)
        .join("manual.txt")
        .is_file());

    assert_eq!(adapters().len(), 7);
}
