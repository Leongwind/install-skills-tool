use skill_installer_lib::commands::{rollback_operation_inner, uninstall_installation_inner};
use skill_installer_lib::domain::{
    InstallScope, InstallationProvenance, PersistedState, PhysicalInstallation, SkillSource,
    SkillSourceDetails,
};
use skill_installer_lib::storage;
use std::fs;

/// Exercises the public recovery seam across the durable state, filesystem,
/// backup and journal layers.  This intentionally uses only temporary paths;
/// it does not require an installed IDE or network access.
#[test]
fn uninstall_and_rollback_preserve_a_complete_installation_record() {
    let data = tempfile::tempdir().expect("state directory");
    let home = tempfile::tempdir().expect("skill home");
    let skill_path = home.path().join(".agents/skills/demo");
    fs::create_dir_all(&skill_path).expect("skill directory");
    fs::write(
        skill_path.join("SKILL.md"),
        "---\nname: demo\ndescription: Demo\n---\n",
    )
    .expect("skill manifest");
    let content_hash = storage::inspect_tree(&skill_path)
        .expect("hash installed skill")
        .0;
    let installation = PhysicalInstallation {
        id: "demo-installation".to_string(),
        skill_name: "demo".to_string(),
        resolved_path: skill_path.display().to_string(),
        source: Some(SkillSource::LocalDirectory {
            path: home.path().display().to_string(),
        }),
        source_details: SkillSourceDetails::default(),
        content_hash: content_hash.clone(),
        scope: InstallScope::Global,
        consumers: vec!["codex".to_string()],
        passive_consumers: Vec::new(),
        adapter_version: 2,
        installed_at: "2026-08-29T00:00:00Z".to_string(),
        provenance: InstallationProvenance::Tool,
        legacy_project: false,
    };
    storage::save_state(
        data.path(),
        &PersistedState {
            installations: vec![installation.clone()],
            ..PersistedState::default()
        },
    )
    .expect("save initial state");

    let removed = uninstall_installation_inner(&installation.id, false, data.path())
        .expect("uninstall should succeed");
    assert!(removed.success);
    assert!(!skill_path.exists());
    let after_uninstall = storage::load_state(data.path()).expect("load uninstall state");
    assert!(after_uninstall.installations.is_empty());
    let journal = after_uninstall
        .operation_journals
        .iter()
        .find(|journal| journal.operation_type == "uninstall")
        .expect("uninstall journal");
    assert_eq!(journal.targets[0].resulting_hash, None);

    let restored = rollback_operation_inner(&journal.id, data.path()).expect("rollback");
    assert_eq!(restored.len(), 1);
    assert!(restored[0].success);
    assert_eq!(
        storage::inspect_tree(&skill_path)
            .expect("hash restored skill")
            .0,
        content_hash
    );
    let final_state = storage::load_state(data.path()).expect("load restored state");
    assert_eq!(final_state.installations.len(), 1);
    assert_eq!(final_state.installations[0].id, installation.id);
}
