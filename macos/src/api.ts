import { invoke } from "@tauri-apps/api/core";
import type {
  BackupRecord,
  EnvironmentScan,
  InstallPlan,
  OperationResult,
  PhysicalInstallation,
  SkillAssignment,
  SkillSource,
  SourceInspection,
  UpdateStatus,
} from "./types";

export const api = {
  scanEnvironment: () => invoke<EnvironmentScan>("scan_environment"),
  inspectSource: (source: SkillSource) =>
    invoke<SourceInspection>("inspect_source", { source }),
  planInstall: (inspectionId: string, assignments: SkillAssignment[]) =>
    invoke<InstallPlan>("plan_install", { inspectionId, assignments }),
  applyInstallPlan: (planId: string, overwriteEntryIds: string[]) =>
    invoke<OperationResult[]>("apply_install_plan", {
      planId,
      overwriteEntryIds,
    }),
  listInstallations: () =>
    invoke<PhysicalInstallation[]>("list_installations"),
  listBackups: () => invoke<BackupRecord[]>("list_backups"),
  adoptExternalSkill: (clientId: string, resolvedPath: string) =>
    invoke<PhysicalInstallation>("adopt_external_skill", {
      clientId,
      resolvedPath,
    }),
  checkUpdates: () => invoke<UpdateStatus[]>("check_updates"),
  uninstall: (installationId: string, force: boolean) =>
    invoke<OperationResult>("uninstall_installation", {
      installationId,
      force,
    }),
  restoreBackup: (backupId: string) =>
    invoke<OperationResult>("restore_backup", { backupId }),
  exportDiagnostics: () => invoke<string>("export_diagnostics"),
  revealInFinder: (path: string) => invoke<void>("reveal_in_finder", { path }),
};
