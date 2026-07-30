import { invoke } from "@tauri-apps/api/core";
import type {
  BackupRecord,
  DetectedClient,
  InstallPlan,
  InstallScope,
  OperationResult,
  PhysicalInstallation,
  SkillMetadata,
  SkillSource,
  UpdateStatus,
} from "./types";

export const api = {
  scanClients: () => invoke<DetectedClient[]>("scan_clients"),
  inspectSkill: (source: SkillSource) =>
    invoke<SkillMetadata>("inspect_skill", { source }),
  planInstall: (
    source: SkillSource,
    clientIds: string[],
    scope: InstallScope,
    projectPath?: string,
  ) =>
    invoke<InstallPlan>("plan_install", {
      source,
      clientIds,
      scope,
      projectPath,
    }),
  applyInstallPlan: (planId: string, overwritePaths: string[]) =>
    invoke<OperationResult[]>("apply_install_plan", {
      planId,
      overwritePaths,
    }),
  listInstallations: () =>
    invoke<PhysicalInstallation[]>("list_installations"),
  listBackups: () => invoke<BackupRecord[]>("list_backups"),
  checkUpdates: () => invoke<UpdateStatus[]>("check_updates"),
  uninstall: (installationId: string, force: boolean) =>
    invoke<OperationResult>("uninstall_installation", {
      installationId,
      force,
    }),
  restoreBackup: (backupId: string) =>
    invoke<OperationResult>("restore_backup", { backupId }),
  exportDiagnostics: () => invoke<string>("export_diagnostics"),
};
