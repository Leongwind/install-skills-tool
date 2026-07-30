export type ClientEdition = "standard" | "traeInternational" | "traeChina";
export type DetectionStatus =
  | "installed"
  | "cliOnly"
  | "configOnly"
  | "unsupportedVersion"
  | "notInstalled";
export type InstallScope = "global" | "project";
export type ConflictState =
  | "notInstalled"
  | "identical"
  | "updateAvailable"
  | "conflict"
  | "notWritable";

export interface DetectedClient {
  id: string;
  name: string;
  edition: ClientEdition;
  version?: string;
  status: DetectionStatus;
  applicationPath?: string;
  cliPath?: string;
  globalSkillsPath: string;
  projectSkillsPath: string;
  supportsSkills: boolean;
  notes: string[];
}

export type SkillSource =
  | { kind: "local"; path: string }
  | { kind: "github"; url: string };

export interface SkillSourceDetails {
  owner?: string;
  repository?: string;
  reference?: string;
  subpath?: string;
  commitSha?: string;
  localPath?: string;
}

export interface SkillMetadata {
  name: string;
  description: string;
  source: SkillSource;
  sourceDetails: SkillSourceDetails;
  preparedPath: string;
  contentHash: string;
  fileCount: number;
  totalBytes: number;
  hasScripts: boolean;
  warnings: string[];
}

export interface InstallPlanEntry {
  resolvedPath: string;
  consumers: string[];
  passiveConsumers: string[];
  conflict: ConflictState;
  existingHash?: string;
  warnings: string[];
}

export interface InstallPlan {
  planId: string;
  skill: SkillMetadata;
  scope: InstallScope;
  entries: InstallPlanEntry[];
}

export interface OperationResult {
  path: string;
  success: boolean;
  status: string;
  message: string;
}

export interface PhysicalInstallation {
  id: string;
  skillName: string;
  resolvedPath: string;
  source: SkillSource;
  sourceDetails: SkillSourceDetails;
  contentHash: string;
  scope: InstallScope;
  consumers: string[];
  passiveConsumers: string[];
  adapterVersion: number;
  installedAt: string;
}

export interface BackupRecord {
  id: string;
  originalPath: string;
  backupPath: string;
  createdAt: string;
}

export interface UpdateStatus {
  installationId: string;
  status: "current" | "sourceChanged" | "targetModified" | "sourceUnavailable";
  message: string;
}
