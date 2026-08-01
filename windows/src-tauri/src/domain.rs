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
pub struct DetectedClient {
    pub id: String,
    pub name: String,
    pub edition: ClientEdition,
    pub version: Option<String>,
    pub status: DetectionStatus,
    pub application_path: Option<String>,
    pub cli_path: Option<String>,
    pub global_skills_path: String,
    pub supports_skills: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SkillSource {
    LocalDirectory { path: String },
    LocalArchive { path: String },
    Github { url: String },
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
    pub skills: Vec<SkillMetadata>,
    pub entries: Vec<InstallPlanEntry>,
}

#[derive(Debug, Clone)]
pub struct PendingPlan {
    pub public: InstallPlan,
    pub source_paths: std::collections::HashMap<String, PathBuf>,
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
    pub entry_id: Option<String>,
    pub skill_name: Option<String>,
    pub path: String,
    pub success: bool,
    pub status: String,
    pub message: String,
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
pub struct PhysicalInstallation {
    pub id: String,
    pub skill_name: String,
    pub resolved_path: String,
    pub source: Option<SkillSource>,
    #[serde(default)]
    pub source_details: SkillSourceDetails,
    pub content_hash: String,
    pub consumers: Vec<String>,
    pub passive_consumers: Vec<String>,
    pub adapter_version: u32,
    pub installed_at: String,
    #[serde(default)]
    pub provenance: InstallationProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub id: String,
    pub original_path: String,
    pub backup_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub schema_version: u32,
    pub installations: Vec<PhysicalInstallation>,
    pub backups: Vec<BackupRecord>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            installations: Vec::new(),
            backups: Vec::new(),
        }
    }
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
