import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  Backup,
  EnvironmentScan,
  InstallPlan,
  Installation,
  OperationResult,
  SkillAssignment,
  SkillSource,
  SourceInspection,
} from "./types";

export const backend = {
  scanEnvironment: () => invoke<EnvironmentScan>("scan_environment"),
  inspectSource: (source: SkillSource) => invoke<SourceInspection>("inspect_source", { source }),
  planInstall: (inspectionId: string, assignments: SkillAssignment[]) =>
    invoke<InstallPlan>("plan_install", { inspectionId, assignments }),
  applyInstallPlan: (planId: string, overwriteEntryIds: string[]) =>
    invoke<OperationResult[]>("apply_install_plan", { planId, overwriteEntryIds }),
  adoptExternalSkill: (clientId: string, resolvedPath: string) =>
    invoke<Installation>("adopt_external_skill", { clientId, resolvedPath }),
  listInstallations: () => invoke<Installation[]>("list_installations"),
  listBackups: () => invoke<Backup[]>("list_backups"),
  uninstallInstallation: (installationId: string, force = false) =>
    invoke<OperationResult>("uninstall_installation", { installationId, force }),
  restoreBackup: (backupId: string) => invoke<OperationResult>("restore_backup", { backupId }),
  exportDiagnostics: () => invoke<string>("export_diagnostics"),
  chooseDirectory: () => open({ directory: true, multiple: false }),
  chooseArchive: () => open({ directory: false, multiple: false, filters: [{ name: "ZIP", extensions: ["zip"] }] }),
};
