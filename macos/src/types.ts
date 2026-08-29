export type ClientEdition = "standard" | "traeInternational" | "traeChina";
export type DetectionStatus =
  | "installed"
  | "cliOnly"
  | "configOnly"
  | "unsupportedVersion"
  | "notInstalled";
export type DetectionEvidenceKind =
  | "application"
  | "cli"
  | "configuration"
  | "skillsDirectory";
export type ConflictState =
  | "notInstalled"
  | "identical"
  | "updateAvailable"
  | "conflict"
  | "notWritable";
export type InstallScope = "global" | "project";
export type SkillValidity = "valid" | "nonConforming" | "unsafe";
export type SkillManagementStatus =
  | "toolManaged"
  | "adopted"
  | "external"
  | "modified"
  | "unsafe"
  | "passive";

export interface DetectedClient {
  id: string;
  name: string;
  edition: ClientEdition;
  version?: string;
  status: DetectionStatus;
  applicationPath?: string;
  cliPath?: string;
  globalSkillsPath: string;
  inventorySkillsPaths?: string[];
  detectionEvidence?: DetectionEvidence[];
  supportsSkills: boolean;
  notes: string[];
}

export interface DetectionEvidence {
  kind: DetectionEvidenceKind;
  path: string;
  version?: string;
  message: string;
}

export type SkillSource =
  | { kind: "localDirectory"; path: string }
  | { kind: "localArchive"; path: string }
  | { kind: "github"; url: string };

export interface SkillSourceDetails {
  owner?: string;
  repository?: string;
  reference?: string;
  subpath?: string;
  commitSha?: string;
  localPath?: string;
  archivePath?: string;
}

export interface SkillMetadata {
  skillId: string;
  relativePath: string;
  name: string;
  description: string;
  license?: string;
  compatibility?: string;
  metadata?: unknown;
  allowedTools?: string;
  source: SkillSource;
  sourceDetails: SkillSourceDetails;
  preparedPath: string;
  contentHash: string;
  fileCount: number;
  totalBytes: number;
  hasScripts: boolean;
  warnings: string[];
}

export interface RejectedSkill {
  relativePath: string;
  reason: string;
}

export interface SourceInspection {
  inspectionId: string;
  source: SkillSource;
  skills: SkillMetadata[];
  rejected: RejectedSkill[];
  warnings: string[];
}

export interface SkillAssignment {
  skillId: string;
  clientIds: string[];
}

export interface InstallPlanEntry {
  entryId: string;
  skillId: string;
  skillName: string;
  resolvedPath: string;
  consumers: string[];
  passiveConsumers: string[];
  conflict: ConflictState;
  existingHash?: string;
  warnings: string[];
}

export interface InstallPlan {
  planId: string;
  createdAt: string;
  expiresAt: string;
  skills: SkillMetadata[];
  entries: InstallPlanEntry[];
}

export interface OperationResult {
  entryId?: string;
  skillName?: string;
  path: string;
  success: boolean;
  status: string;
  message: string;
}

export interface PhysicalInstallation {
  id: string;
  skillName: string;
  resolvedPath: string;
  source?: SkillSource;
  sourceDetails: SkillSourceDetails;
  contentHash: string;
  scope: InstallScope;
  consumers: string[];
  passiveConsumers: string[];
  adapterVersion: number;
  installedAt: string;
  provenance: "tool" | "adopted";
  legacyProject: boolean;
}

export interface InventorySkill {
  inventoryId: string;
  name: string;
  directoryName: string;
  description?: string;
  resolvedPath: string;
  contentHash?: string;
  validity: SkillValidity;
  managementStatus: SkillManagementStatus;
  installationId?: string;
  issues: string[];
  consumers: string[];
  passiveFromClientId?: string;
}

export interface ClientSkillInventory {
  clientId: string;
  rootPath: string;
  directSkills: InventorySkill[];
  passiveSkills: InventorySkill[];
  scanError?: string;
}

export interface EnvironmentScan {
  clients: DetectedClient[];
  inventories: ClientSkillInventory[];
}

export interface BackupRecord {
  id: string;
  originalPath: string;
  backupPath: string;
  createdAt: string;
}

export interface UpdateStatus {
  installationId: string;
  status:
    | "current"
    | "sourceChanged"
    | "targetModified"
    | "sourceUnavailable"
    | "pinned";
  message: string;
  currentHash?: string;
  sourceHash?: string;
  sourceRevision?: string;
  changes?: FileChangeSummary;
}

export interface UpdatePlanEntry extends UpdateStatus {
  entryId: string;
  skillName: string;
  resolvedPath: string;
  requiresConfirmation: boolean;
}

export interface UpdatePlan {
  planId: string;
  createdAt: string;
  expiresAt: string;
  entries: UpdatePlanEntry[];
}

export interface FileChangeSummary {
  added: string[];
  modified: string[];
  removed: string[];
}

export type OperationJournalStatus =
  | "preparing"
  | "applying"
  | "partial"
  | "completed"
  | "recoveryRequired"
  | "rolledBack";

export interface OperationJournalTarget {
  path: string;
  existedBefore: boolean;
  backupId?: string;
  completed: boolean;
  resultingHash?: string;
}

export interface OperationJournal {
  id: string;
  operationType: string;
  createdAt: string;
  finishedAt?: string;
  status: OperationJournalStatus;
  targets: OperationJournalTarget[];
  message?: string;
}

export interface BackupPolicy {
  maxBackupsPerSkill: number;
  maxTotalBytes: number;
  retentionDays: number;
}

export interface AppOverview {
  backupPolicy: BackupPolicy;
  operationJournals: OperationJournal[];
  pinnedInstallationIds: string[];
}

export interface PortableBundleManifest {
  schemaVersion: number;
  exportedAt: string;
  appVersion: string;
  skills: Array<{
    skillName: string;
    contentHash: string;
    consumers: string[];
    archivePath: string;
  }>;
}

export interface SkillLockEntry {
  skillName: string;
  source?: SkillSource;
  sourceDetails: SkillSourceDetails;
  contentHash: string;
  consumers: string[];
  pinned: boolean;
}

export interface SkillLockfile {
  schemaVersion: number;
  generatedAt: string;
  appVersion: string;
  skills: SkillLockEntry[];
}

export interface LockfileImportPlan {
  installPlan: InstallPlan;
  missingClientIds: string[];
  unavailableSkills: Array<{ skillName: string; reason: string }>;
  extraInstallationIds: string[];
}
