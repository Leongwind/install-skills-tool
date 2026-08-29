use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ClientEdition {
    Standard,
    TraeInternational,
    TraeChina,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DetectionStatus {
    Installed,
    CliOnly,
    ConfigOnly,
    UnsupportedVersion,
    NotInstalled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DetectionEvidenceKind {
    Application,
    Cli,
    Configuration,
    SkillsDirectory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DetectionEvidence {
    pub kind: DetectionEvidenceKind,
    pub path: String,
    pub version: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedClient {
    pub id: String,
    pub name: String,
    pub edition: ClientEdition,
    pub version: Option<String>,
    pub status: DetectionStatus,
    pub application_path: Option<String>,
    pub cli_path: Option<String>,
    pub global_skills_path: String,
    #[serde(default)]
    pub inventory_skills_paths: Vec<String>,
    #[serde(default)]
    pub detection_evidence: Vec<DetectionEvidence>,
    pub supports_skills: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SkillSource {
    #[serde(alias = "local")]
    LocalDirectory {
        path: String,
    },
    LocalArchive {
        path: String,
    },
    Github {
        url: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSourceDetails {
    pub owner: Option<String>,
    pub repository: Option<String>,
    pub reference: Option<String>,
    pub subpath: Option<String>,
    pub commit_sha: Option<String>,
    pub local_path: Option<String>,
    pub archive_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetadata {
    pub skill_id: String,
    pub relative_path: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub compatibility: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub allowed_tools: Option<String>,
    pub source: SkillSource,
    pub source_details: SkillSourceDetails,
    pub prepared_path: String,
    pub content_hash: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub has_scripts: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectedSkill {
    pub relative_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInspection {
    pub inspection_id: String,
    pub source: SkillSource,
    pub skills: Vec<SkillMetadata>,
    pub rejected: Vec<RejectedSkill>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InstallScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictState {
    NotInstalled,
    Identical,
    UpdateAvailable,
    Conflict,
    NotWritable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlanEntry {
    pub entry_id: String,
    pub skill_id: String,
    pub skill_name: String,
    pub resolved_path: String,
    pub consumers: Vec<String>,
    pub passive_consumers: Vec<String>,
    pub conflict: ConflictState,
    pub existing_hash: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub plan_id: String,
    pub created_at: String,
    pub expires_at: String,
    pub skills: Vec<SkillMetadata>,
    pub entries: Vec<InstallPlanEntry>,
}

#[derive(Debug, Clone)]
pub struct PendingPlan {
    pub public: InstallPlan,
    pub source_paths: std::collections::HashMap<String, PathBuf>,
    pub pinned_skill_ids: std::collections::HashSet<String>,
    pub created_at: String,
    pub expires_at: String,
    pub source_guards: std::collections::HashMap<String, String>,
    pub target_guards: std::collections::HashMap<String, PlanTargetGuard>,
    /// The installation record observed while the plan was generated.  The
    /// value is `None` when the target was not tracked at that time.
    pub installation_guards: std::collections::HashMap<String, Option<String>>,
}

/// The immutable filesystem state captured while an operation plan is shown.
/// Applying a plan is only safe when both existence and content still match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTargetGuard {
    pub existed: bool,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillAssignment {
    pub skill_id: String,
    pub client_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    #[serde(default)]
    pub entry_id: Option<String>,
    #[serde(default)]
    pub skill_name: Option<String>,
    pub path: String,
    pub success: bool,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalInstallation {
    pub id: String,
    pub skill_name: String,
    pub resolved_path: String,
    pub source: Option<SkillSource>,
    #[serde(default)]
    pub source_details: SkillSourceDetails,
    pub content_hash: String,
    pub scope: InstallScope,
    pub consumers: Vec<String>,
    pub passive_consumers: Vec<String>,
    pub adapter_version: u32,
    pub installed_at: String,
    #[serde(default)]
    pub provenance: InstallationProvenance,
    #[serde(default)]
    pub legacy_project: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum InstallationProvenance {
    #[default]
    Tool,
    Adopted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub id: String,
    pub original_path: String,
    pub backup_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OperationJournalStatus {
    Preparing,
    Applying,
    Partial,
    Completed,
    RecoveryRequired,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationJournalTarget {
    pub path: String,
    pub existed_before: bool,
    pub backup_id: Option<String>,
    pub completed: bool,
    #[serde(default)]
    pub resulting_hash: Option<String>,
    #[serde(default)]
    pub previous_installation: Option<PhysicalInstallation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationJournal {
    pub id: String,
    pub operation_type: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub status: OperationJournalStatus,
    pub targets: Vec<OperationJournalTarget>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPolicy {
    pub max_backups_per_skill: usize,
    pub max_total_bytes: u64,
    pub retention_days: u32,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        Self {
            max_backups_per_skill: 5,
            max_total_bytes: 1024 * 1024 * 1024,
            retention_days: 90,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub schema_version: u32,
    #[serde(default)]
    pub revision: u64,
    pub installations: Vec<PhysicalInstallation>,
    pub backups: Vec<BackupRecord>,
    #[serde(default)]
    pub operation_journals: Vec<OperationJournal>,
    #[serde(default)]
    pub backup_policy: BackupPolicy,
    #[serde(default)]
    pub pinned_installation_ids: Vec<String>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            schema_version: 5,
            revision: 0,
            installations: Vec::new(),
            backups: Vec::new(),
            operation_journals: Vec::new(),
            backup_policy: BackupPolicy::default(),
            pinned_installation_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UpdateState {
    Current,
    SourceChanged,
    TargetModified,
    SourceUnavailable,
    Pinned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub installation_id: String,
    pub status: UpdateState,
    pub message: String,
    #[serde(default)]
    pub current_hash: Option<String>,
    #[serde(default)]
    pub source_hash: Option<String>,
    #[serde(default)]
    pub source_revision: Option<String>,
    #[serde(default)]
    pub changes: Option<FileChangeSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlanEntry {
    pub entry_id: String,
    pub installation_id: String,
    pub skill_name: String,
    pub resolved_path: String,
    pub status: UpdateState,
    pub message: String,
    pub current_hash: Option<String>,
    pub source_hash: Option<String>,
    pub source_revision: Option<String>,
    pub changes: Option<FileChangeSummary>,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlan {
    pub plan_id: String,
    pub created_at: String,
    pub expires_at: String,
    pub entries: Vec<UpdatePlanEntry>,
}

#[derive(Debug, Clone)]
pub struct PendingUpdatePlan {
    pub public: UpdatePlan,
    pub metadata_by_entry: std::collections::HashMap<String, SkillMetadata>,
    pub created_at: String,
    pub expires_at: String,
    pub source_guards: std::collections::HashMap<String, String>,
    pub target_guards: std::collections::HashMap<String, PlanTargetGuard>,
    pub installation_guards: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeSummary {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableBundleEntry {
    pub skill_name: String,
    pub content_hash: String,
    pub consumers: Vec<String>,
    pub archive_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableBundleManifest {
    pub schema_version: u32,
    pub exported_at: String,
    pub app_version: String,
    pub skills: Vec<PortableBundleEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLockEntry {
    pub skill_name: String,
    pub source: Option<SkillSource>,
    #[serde(default)]
    pub source_details: SkillSourceDetails,
    pub content_hash: String,
    pub consumers: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLockfile {
    pub schema_version: u32,
    pub generated_at: String,
    pub app_version: String,
    pub skills: Vec<SkillLockEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockfileIssue {
    pub skill_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockfileImportPlan {
    pub install_plan: InstallPlan,
    pub missing_client_ids: Vec<String>,
    pub unavailable_skills: Vec<LockfileIssue>,
    pub extra_installation_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PendingLockfilePlan {
    pub public: LockfileImportPlan,
    pub pending_install: PendingPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppOverview {
    pub backup_policy: BackupPolicy,
    pub operation_journals: Vec<OperationJournal>,
    pub pinned_installation_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SkillValidity {
    Valid,
    NonConforming,
    Unsafe,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SkillManagementStatus {
    ToolManaged,
    Adopted,
    External,
    Modified,
    Unsafe,
    Passive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySkill {
    pub inventory_id: String,
    pub name: String,
    pub directory_name: String,
    pub description: Option<String>,
    pub resolved_path: String,
    pub content_hash: Option<String>,
    pub validity: SkillValidity,
    pub management_status: SkillManagementStatus,
    pub installation_id: Option<String>,
    pub issues: Vec<String>,
    pub consumers: Vec<String>,
    pub passive_from_client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSkillInventory {
    pub client_id: String,
    pub root_path: String,
    pub direct_skills: Vec<InventorySkill>,
    pub passive_skills: Vec<InventorySkill>,
    pub scan_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentScan {
    pub clients: Vec<DetectedClient>,
    pub inventories: Vec<ClientSkillInventory>,
}
