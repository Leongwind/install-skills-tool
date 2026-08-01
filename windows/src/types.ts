export type DetectionStatus = "installed" | "cliOnly" | "configOnly" | "unsupportedVersion" | "notInstalled";
export type ConflictState = "notInstalled" | "identical" | "updateAvailable" | "conflict" | "notWritable";
export type ManagementStatus = "toolManaged" | "adopted" | "external" | "modified" | "unsafe" | "passive";

export interface DetectedClient {
  id: string;
  name: string;
  edition: "standard" | "traeInternational" | "traeChina";
  version?: string;
  status: DetectionStatus;
  applicationPath?: string;
  cliPath?: string;
  globalSkillsPath: string;
  supportsSkills: boolean;
  notes: string[];
}

export type SkillSource =
  | { kind: "localDirectory"; path: string }
  | { kind: "localArchive"; path: string }
  | { kind: "github"; url: string };

export interface SkillMetadata {
  skillId: string;
  relativePath: string;
  name: string;
  description: string;
  source: SkillSource;
  preparedPath: string;
  contentHash: string;
  fileCount: number;
  totalBytes: number;
  hasScripts: boolean;
  warnings: string[];
}

export interface SourceInspection {
  inspectionId: string;
  source: SkillSource;
  skills: SkillMetadata[];
  rejected: { relativePath: string; reason: string }[];
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

export interface InventorySkill {
  inventoryId: string;
  name: string;
  directoryName: string;
  description?: string;
  resolvedPath: string;
  contentHash?: string;
  validity: "valid" | "nonConforming" | "unsafe";
  managementStatus: ManagementStatus;
  installationId?: string;
  issues: string[];
  consumers: string[];
  passiveFromClientId?: string;
}

export interface ClientInventory {
  clientId: string;
  rootPath: string;
  directSkills: InventorySkill[];
  passiveSkills: InventorySkill[];
  scanError?: string;
}

export interface EnvironmentScan {
  clients: DetectedClient[];
  inventories: ClientInventory[];
}

export interface Installation {
  id: string;
  skillName: string;
  resolvedPath: string;
  contentHash: string;
  consumers: string[];
  passiveConsumers: string[];
  provenance: "tool" | "adopted";
}

export interface Backup {
  id: string;
  originalPath: string;
  backupPath: string;
  createdAt: string;
}
