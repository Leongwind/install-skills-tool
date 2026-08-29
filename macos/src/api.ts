import { invoke } from "@tauri-apps/api/core";
import type {
  BackupRecord,
  AppOverview,
  ClientSkillInventory,
  EnvironmentScan,
  InstallPlan,
  LockfileImportPlan,
  OperationProgress,
  OperationResult,
  PhysicalInstallation,
  PortableBundleManifest,
  SkillAssignment,
  SkillLockfile,
  SkillSource,
  SourceInspection,
  UpdateStatus,
  UpdatePlan,
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
  getAppOverview: () => invoke<AppOverview>("get_app_overview"),
  scanClientInventory: (clientId: string) =>
    invoke<ClientSkillInventory>("scan_client_inventory", { clientId }),
  recoverOperation: (journalId: string) =>
    invoke<OperationResult[]>("recover_operation", { journalId }),
  rollbackOperation: (journalId: string) =>
    invoke<OperationResult[]>("rollback_operation", { journalId }),
  exportSkillBundle: (installationIds: string[], destination: string) =>
    invoke<PortableBundleManifest>("export_skill_bundle", {
      installationIds,
      destination,
    }),
  exportLockfile: (installationIds: string[], destination: string) =>
    invoke<SkillLockfile>("export_lockfile", {
      installationIds,
      destination,
    }),
  planLockfileImport: (path: string) =>
    invoke<LockfileImportPlan>("plan_lockfile_import", { path }),
  adoptExternalSkill: (clientId: string, resolvedPath: string) =>
    invoke<PhysicalInstallation>("adopt_external_skill", {
      clientId,
      resolvedPath,
    }),
  checkUpdates: () => invoke<UpdateStatus[]>("check_updates"),
  planUpdates: (installationIds: string[]) =>
    invoke<UpdatePlan>("plan_updates", { installationIds }),
  applyUpdatePlan: (planId: string, approvedEntryIds: string[]) =>
    invoke<OperationResult[]>("apply_update_plan", {
      planId,
      approvedEntryIds,
    }),
  getOperationProgress: () =>
    invoke<OperationProgress | null>("get_operation_progress"),
  cancelOperation: () => invoke<boolean>("cancel_operation"),
  setInstallationPinned: (installationId: string, pinned: boolean) =>
    invoke<void>("set_installation_pinned", { installationId, pinned }),
  uninstall: (installationId: string, force: boolean) =>
    invoke<OperationResult>("uninstall_installation", {
      installationId,
      force,
    }),
  restoreBackup: (backupId: string) =>
    invoke<OperationResult>("restore_backup", { backupId }),
  setBackupPolicy: (policy: {
    maxBackupsPerSkill: number;
    maxTotalBytes: number;
    retentionDays: number;
  }) => invoke<void>("set_backup_policy", { policy }),
  deleteBackup: (backupId: string) =>
    invoke<void>("delete_backup", { backupId }),
  exportDiagnostics: () => invoke<string>("export_diagnostics"),
  revealInFinder: (path: string) => invoke<void>("reveal_in_finder", { path }),
};
