use serde::{Deserialize, Serialize};

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
