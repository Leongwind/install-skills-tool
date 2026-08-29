import {
  ArrowClockwise,
  ArrowRight,
  Archive,
  Check,
  ClockCounterClockwise,
  CloudArrowDown,
  Code,
  Copy,
  Database,
  FolderOpen,
  FolderSimple,
  Gear,
  GithubLogo,
  Info,
  MagnifyingGlass,
  Package,
  ShieldWarning,
  Trash,
  WarningCircle,
} from "@phosphor-icons/react";
import {
  Badge,
  Button,
  Callout,
  Checkbox,
  Flex,
  Separator,
  Spinner,
  Text,
  TextField,
  Theme,
} from "@radix-ui/themes";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./api";
import type {
  BackupRecord,
  AppOverview,
  ClientSkillInventory,
  DetectedClient,
  EnvironmentScan,
  InstallPlan,
  InventorySkill,
  LockfileImportPlan,
  OperationResult,
  PhysicalInstallation,
  SkillManagementStatus,
  SkillSource,
  SourceInspection,
  UpdateStatus,
  UpdatePlan,
} from "./types";
import { conflictLabel, detectionLabel, formatBytes, shortPath } from "./ui";

type Page = "dashboard" | "install" | "manage" | "operations" | "diagnostics";
type SourceMode = "localDirectory" | "localArchive" | "github";
type InventoryFilter = "all" | "managed" | "external" | "issues";

const inventoryStatus: Record<
  SkillManagementStatus,
  { label: string; color: "green" | "blue" | "gray" | "orange" | "red" }
> = {
  toolManaged: { label: "本工具安装", color: "green" },
  adopted: { label: "已纳管", color: "blue" },
  external: { label: "外部安装", color: "gray" },
  modified: { label: "内容已修改", color: "orange" },
  unsafe: { label: "不安全", color: "red" },
  passive: { label: "被动发现", color: "blue" },
};

const operationLabels: Record<string, string> = {
  install: "安装",
  update: "更新",
  adopt: "纳管",
  uninstall: "卸载",
  restore: "恢复",
};

function friendlyError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function StatusBadge({ client }: { client: DetectedClient }) {
  const color =
    client.status === "installed"
      ? "green"
      : client.status === "unsupportedVersion"
        ? "orange"
        : client.status === "notInstalled"
          ? "gray"
          : "blue";
  return (
    <Badge color={color} variant="soft">
      {detectionLabel[client.status]}
    </Badge>
  );
}

function SearchBox({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
}) {
  return (
    <TextField.Root
      value={value}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
    >
      <TextField.Slot>
        <MagnifyingGlass />
      </TextField.Slot>
    </TextField.Root>
  );
}

function LoadingRows() {
  return (
    <div aria-label="正在扫描" className="skeleton-list">
      {[0, 1, 2].map((index) => (
        <div className="skeleton-row" key={index}>
          <span />
          <span />
        </div>
      ))}
    </div>
  );
}

export default function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [environment, setEnvironment] = useState<EnvironmentScan>({
    clients: [],
    inventories: [],
  });
  const [installations, setInstallations] = useState<PhysicalInstallation[]>([]);
  const [backups, setBackups] = useState<BackupRecord[]>([]);
  const [overview, setOverview] = useState<AppOverview>({
    backupPolicy: {
      maxBackupsPerSkill: 5,
      maxTotalBytes: 1024 * 1024 * 1024,
      retentionDays: 90,
    },
    operationJournals: [],
    pinnedInstallationIds: [],
  });
  const [sourceMode, setSourceMode] =
    useState<SourceMode>("localDirectory");
  const [sourceValue, setSourceValue] = useState("");
  const [inspection, setInspection] = useState<SourceInspection>();
  const [selectedSkillIds, setSelectedSkillIds] = useState<string[]>([]);
  const [assignments, setAssignments] = useState<Record<string, string[]>>({});
  const [sourceSearch, setSourceSearch] = useState("");
  const [plan, setPlan] = useState<InstallPlan>();
  const [overwrites, setOverwrites] = useState<string[]>([]);
  const [results, setResults] = useState<OperationResult[]>([]);
  const [updates, setUpdates] = useState<UpdateStatus[]>([]);
  const [updatePlan, setUpdatePlan] = useState<UpdatePlan>();
  const [approvedUpdateIds, setApprovedUpdateIds] = useState<string[]>([]);
  const [lockfilePlan, setLockfilePlan] = useState<LockfileImportPlan>();
  const [lockfileOverwrites, setLockfileOverwrites] = useState<string[]>([]);
  const [inventorySearch, setInventorySearch] = useState("");
  const [inventoryFilter, setInventoryFilter] =
    useState<InventoryFilter>("all");
  const [diagnostics, setDiagnostics] = useState("");
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [policyDraft, setPolicyDraft] = useState({
    maxBackupsPerSkill: 5,
    maxTotalMb: 1024,
    retentionDays: 90,
  });

  const clients = environment.clients;

  const refresh = useCallback(async () => {
    setBusy("scan");
    setError("");
    try {
      const [nextEnvironment, nextInstallations, nextBackups, nextOverview] =
        await Promise.all([
          api.scanEnvironment(),
          api.listInstallations(),
          api.listBackups(),
          api.getAppOverview(),
        ]);
      setEnvironment(nextEnvironment);
      setInstallations(nextInstallations);
      setBackups(nextBackups);
      setOverview(nextOverview);
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Keep the inventory view useful while an IDE or another manager changes a
  // known global directory. This is deliberately local-only; network update
  // checks remain explicit user actions.
  useEffect(() => {
    if (page !== "manage") return;
    let active = true;
    const timer = window.setInterval(() => {
      void api
        .scanEnvironment()
        .then((nextEnvironment) => {
          if (active) setEnvironment(nextEnvironment);
        })
        .catch(() => {
          // Keep the last good inventory visible; the next manual refresh will
          // surface a detailed error if the issue persists.
        });
    }, 5000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [page]);

  useEffect(() => {
    setPolicyDraft({
      maxBackupsPerSkill: overview.backupPolicy.maxBackupsPerSkill,
      maxTotalMb: Math.max(
        1,
        Math.round(overview.backupPolicy.maxTotalBytes / (1024 * 1024)),
      ),
      retentionDays: overview.backupPolicy.retentionDays,
    });
  }, [overview.backupPolicy]);

  const source: SkillSource | undefined = sourceValue
    ? sourceMode === "github"
      ? { kind: "github", url: sourceValue }
      : { kind: sourceMode, path: sourceValue }
    : undefined;

  const targetClients = useMemo(
    () => clients.filter((client) => client.status !== "notInstalled"),
    [clients],
  );
  const selectedSkills = useMemo(
    () =>
      inspection?.skills.filter((skill) =>
        selectedSkillIds.includes(skill.skillId),
      ) ?? [],
    [inspection, selectedSkillIds],
  );
  const filteredSkills = useMemo(() => {
    const query = sourceSearch.trim().toLowerCase();
    if (!inspection || !query) return inspection?.skills ?? [];
    return inspection.skills.filter(
      (skill) =>
        skill.name.toLowerCase().includes(query) ||
        skill.description.toLowerCase().includes(query) ||
        skill.relativePath.toLowerCase().includes(query),
    );
  }, [inspection, sourceSearch]);
  const duplicateNames = useMemo(() => {
    const hashes = new Map<string, Set<string>>();
    for (const skill of selectedSkills) {
      const values = hashes.get(skill.name) ?? new Set<string>();
      values.add(skill.contentHash);
      hashes.set(skill.name, values);
    }
    return [...hashes].filter(([, values]) => values.size > 1).map(([name]) => name);
  }, [selectedSkills]);
  const assignmentsComplete =
    selectedSkills.length > 0 &&
    selectedSkills.every((skill) => (assignments[skill.skillId]?.length ?? 0) > 0);

  function resetInspection() {
    setInspection(undefined);
    setSelectedSkillIds([]);
    setAssignments({});
    setPlan(undefined);
    setResults([]);
  }

  async function chooseSource() {
    const selectedPath =
      sourceMode === "localArchive"
        ? await open({
            directory: false,
            multiple: false,
            filters: [{ name: "ZIP 压缩包", extensions: ["zip"] }],
          })
        : await open({ directory: true, multiple: false });
    if (typeof selectedPath !== "string") return;
    setSourceValue(selectedPath);
    resetInspection();
  }

  async function inspect() {
    if (!source) return;
    setBusy("inspect");
    setError("");
    resetInspection();
    try {
      const next = await api.inspectSource(source);
      setInspection(next);
      setSelectedSkillIds(next.skills.map((skill) => skill.skillId));
      setAssignments(
        Object.fromEntries(next.skills.map((skill) => [skill.skillId, []])),
      );
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  function setSkillSelected(skillId: string, checked: boolean) {
    setSelectedSkillIds((current) =>
      checked
        ? current.includes(skillId)
          ? current
          : [...current, skillId]
        : current.filter((id) => id !== skillId),
    );
    setPlan(undefined);
  }

  function toggleAssignment(skillId: string, clientId: string, checked: boolean) {
    setAssignments((current) => {
      const ids = current[skillId] ?? [];
      return {
        ...current,
        [skillId]: checked
          ? ids.includes(clientId)
            ? ids
            : [...ids, clientId]
          : ids.filter((id) => id !== clientId),
      };
    });
    setPlan(undefined);
  }

  function toggleClientColumn(client: DetectedClient) {
    if (!client.supportsSkills) return;
    const allChecked = selectedSkills.every((skill) =>
      assignments[skill.skillId]?.includes(client.id),
    );
    setAssignments((current) =>
      Object.fromEntries(
        Object.entries(current).map(([skillId, ids]) => [
          skillId,
          selectedSkillIds.includes(skillId)
            ? allChecked
              ? ids.filter((id) => id !== client.id)
              : [...new Set([...ids, client.id])]
            : ids,
        ]),
      ),
    );
    setPlan(undefined);
  }

  async function createPlan() {
    if (!inspection) return;
    setBusy("plan");
    setError("");
    setResults([]);
    try {
      const next = await api.planInstall(
        inspection.inspectionId,
        selectedSkills.map((skill) => ({
          skillId: skill.skillId,
          clientIds: assignments[skill.skillId] ?? [],
        })),
      );
      setPlan(next);
      setOverwrites([]);
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function applyPlan() {
    if (!plan) return;
    setBusy("apply");
    setError("");
    try {
      setResults(await api.applyInstallPlan(plan.planId, overwrites));
      setPlan(undefined);
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function adopt(clientId: string, skill: InventorySkill) {
    if (!window.confirm(`将 ${skill.name} 纳入管理？\n\n${shortPath(skill.resolvedPath)}`)) {
      return;
    }
    setBusy(skill.inventoryId);
    setError("");
    try {
      await api.adoptExternalSkill(clientId, skill.resolvedPath);
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function uninstallById(
    installationId: string,
    displayPath: string,
    force = false,
  ) {
    setBusy(installationId);
    setError("");
    try {
      const result = await api.uninstall(installationId, force);
      if (!result.success && result.status === "confirmationRequired") {
        if (window.confirm(`${result.message}\n\n${shortPath(displayPath)}`)) {
          await uninstallById(installationId, displayPath, true);
        }
        return;
      }
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function restore(backup: BackupRecord) {
    if (!window.confirm(`恢复到 ${shortPath(backup.originalPath)}？当前内容会先备份。`)) {
      return;
    }
    setBusy(backup.id);
    try {
      await api.restoreBackup(backup.id);
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function checkUpdates() {
    setBusy("updates");
    setError("");
    try {
      setUpdates(await api.checkUpdates());
      setUpdatePlan(undefined);
      setApprovedUpdateIds([]);
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function createUpdatePlan() {
    const installationIds = updates
      .filter((update) =>
        ["sourceChanged", "targetModified"].includes(update.status),
      )
      .map((update) => update.installationId);
    if (installationIds.length === 0) return;
    setBusy("update-plan");
    setError("");
    try {
      setUpdatePlan(await api.planUpdates(installationIds));
      setApprovedUpdateIds([]);
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function applyUpdates() {
    if (!updatePlan) return;
    setBusy("update-apply");
    setError("");
    try {
      setResults(
        await api.applyUpdatePlan(updatePlan.planId, approvedUpdateIds),
      );
      setUpdatePlan(undefined);
      setApprovedUpdateIds([]);
      setUpdates([]);
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function setPinned(installationId: string, pinned: boolean) {
    setBusy(`pin:${installationId}`);
    setError("");
    try {
      await api.setInstallationPinned(installationId, pinned);
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function refreshClient(clientId: string) {
    setBusy(`scan:${clientId}`);
    setError("");
    try {
      const inventory = await api.scanClientInventory(clientId);
      setEnvironment((current) => ({
        ...current,
        inventories: current.inventories.map((item) =>
          item.clientId === clientId ? inventory : item,
        ),
      }));
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function recoverOperation(journalId: string) {
    if (!window.confirm("恢复会将已写入目标回滚到本次操作前的状态，是否继续？")) return;
    setBusy(`recover:${journalId}`);
    setError("");
    try {
      setResults(await api.recoverOperation(journalId));
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function rollbackOperation(journalId: string) {
    if (!window.confirm("回滚会恢复该操作涉及的文件与管理记录，是否继续？")) return;
    setBusy(`rollback:${journalId}`);
    setError("");
    try {
      setResults(await api.rollbackOperation(journalId));
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function saveBackupPolicy() {
    setBusy("backup-policy");
    setError("");
    try {
      await api.setBackupPolicy({
        maxBackupsPerSkill: policyDraft.maxBackupsPerSkill,
        maxTotalBytes: policyDraft.maxTotalMb * 1024 * 1024,
        retentionDays: policyDraft.retentionDays,
      });
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function deleteBackup(backup: BackupRecord) {
    if (!window.confirm(`永久删除此备份？\n\n${shortPath(backup.originalPath)}`)) return;
    setBusy(`delete-backup:${backup.id}`);
    setError("");
    try {
      await api.deleteBackup(backup.id);
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function exportPortableBundle() {
    const ids = installations
      .filter((installation) => !installation.legacyProject)
      .map((installation) => installation.id);
    if (ids.length === 0) return;
    const destination = await save({
      defaultPath: `skill-installer-bundle-${new Date().toISOString().slice(0, 10)}.zip`,
      filters: [{ name: "Skill 便携包", extensions: ["zip"] }],
    });
    if (!destination) return;
    setBusy("export");
    setError("");
    try {
      const manifest = await api.exportSkillBundle(ids, destination);
      window.alert(`已导出 ${manifest.skills.length} 个 Skill。此 ZIP 可在另一台机器直接导入。`);
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function exportLockfile() {
    const ids = installations
      .filter((installation) => !installation.legacyProject)
      .map((installation) => installation.id);
    if (ids.length === 0) return;
    const destination = await save({
      defaultPath: `skills-${new Date().toISOString().slice(0, 10)}.lock.json`,
      filters: [{ name: "Skill Installer 锁文件", extensions: ["json"] }],
    });
    if (!destination) return;
    setBusy("lock-export");
    setError("");
    try {
      const lockfile = await api.exportLockfile(ids, destination);
      window.alert(`已导出 ${lockfile.skills.length} 个可复现 Skill 配置。`);
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function importLockfile() {
    const path = await open({
      directory: false,
      multiple: false,
      filters: [{ name: "Skill Installer 锁文件", extensions: ["json"] }],
    });
    if (typeof path !== "string") return;
    setBusy("lock-import");
    setError("");
    try {
      setLockfilePlan(await api.planLockfileImport(path));
      setLockfileOverwrites([]);
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function applyLockfilePlan() {
    if (!lockfilePlan) return;
    setBusy("lock-apply");
    setError("");
    try {
      setResults(
        await api.applyInstallPlan(
          lockfilePlan.installPlan.planId,
          lockfileOverwrites,
        ),
      );
      setLockfilePlan(undefined);
      setLockfileOverwrites([]);
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function beginSync(skill: InventorySkill) {
    setBusy(`sync:${skill.inventoryId}`);
    setError("");
    try {
      const next = await api.inspectSource({
        kind: "localDirectory",
        path: skill.resolvedPath,
      });
      const selected = next.skills[0];
      if (!selected) throw new Error("当前目录中没有可同步的有效 Skill");
      setSourceMode("localDirectory");
      setSourceValue(skill.resolvedPath);
      setInspection(next);
      setSelectedSkillIds([selected.skillId]);
      setAssignments({ [selected.skillId]: [...skill.consumers] });
      setPlan(undefined);
      setResults([]);
      setPage("install");
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function loadDiagnostics() {
    setBusy("diagnostics");
    try {
      setDiagnostics(await api.exportDiagnostics());
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  const managedCount = environment.inventories.reduce(
    (count, inventory) =>
      count +
      inventory.directSkills.filter((skill) =>
        ["toolManaged", "adopted", "modified"].includes(skill.managementStatus),
      ).length,
    0,
  );
  const legacyInstallations = installations.filter((item) => item.legacyProject);
  const externalCount = environment.inventories.reduce(
    (count, inventory) =>
      count +
      inventory.directSkills.filter((skill) => skill.managementStatus === "external")
        .length,
    0,
  );
  const issueCount = environment.inventories.reduce(
    (count, inventory) =>
      count +
      inventory.directSkills.filter((skill) =>
        ["modified", "unsafe"].includes(skill.managementStatus),
      ).length,
    0,
  );
  const recoverableJournals = overview.operationJournals.filter((journal) =>
    ["partial", "recoveryRequired"].includes(journal.status),
  );
  const availableUpdateCount = updates.filter((update) =>
    ["sourceChanged", "targetModified"].includes(update.status),
  ).length;

  return (
    <Theme accentColor="blue" grayColor="slate" radius="medium" scaling="95%">
      <div className="app-shell">
        <header className="titlebar" data-tauri-drag-region>
          <div className="traffic-space" />
          <Package size={17} weight="fill" />
          <strong>Skill Installer</strong>
          <Badge variant="outline">macOS 0.5.0</Badge>
          <span className="titlebar-spacer" />
          <Button size="1" variant="ghost" onClick={() => void refresh()}>
            {busy === "scan" ? <Spinner size="1" /> : <ArrowClockwise />}
            重新扫描
          </Button>
        </header>

        <div className="workspace">
          <aside className="sidebar">
            <nav aria-label="主导航">
              <button
                className={page === "dashboard" ? "nav-item active" : "nav-item"}
                onClick={() => setPage("dashboard")}
              >
                <Package />
                概览
              </button>
              <button
                className={page === "install" ? "nav-item active" : "nav-item"}
                onClick={() => setPage("install")}
              >
                <CloudArrowDown />
                批量安装
              </button>
              <button
                className={page === "manage" ? "nav-item active" : "nav-item"}
                onClick={() => setPage("manage")}
              >
                <Database />
                IDE Skill 库存
                <span className="nav-count">{managedCount}</span>
              </button>
              <button
                className={page === "operations" ? "nav-item active" : "nav-item"}
                onClick={() => setPage("operations")}
              >
                <ClockCounterClockwise />
                操作中心
              </button>
              <button
                className={page === "diagnostics" ? "nav-item active" : "nav-item"}
                onClick={() => setPage("diagnostics")}
              >
                <Gear />
                诊断
              </button>
            </nav>
            <div className="sidebar-footer">
              <Text size="1" color="gray">
                仅在本机处理文件
                <br />
                不执行 Skill 脚本 · 无遥测
              </Text>
            </div>
          </aside>

          <main className="content">
            {error && (
              <Callout.Root color="red" size="1" className="global-error">
                <Callout.Icon>
                  <WarningCircle />
                </Callout.Icon>
                <Callout.Text>{error}</Callout.Text>
              </Callout.Root>
            )}
            {page !== "install" && results.length > 0 && (
              <section className="panel global-results" aria-label="最近操作结果">
                <div className="section-caption">
                  <strong>最近操作结果</strong>
                  <Button size="1" variant="ghost" onClick={() => setResults([])}>
                    关闭
                  </Button>
                </div>
                {results.map((result, index) => (
                  <div className="result-row" key={result.entryId ?? `${result.path}-${index}`}>
                    <span className={result.success ? "result-dot ok" : "result-dot failed"} />
                    <strong>{result.skillName ?? "Skill"}</strong>
                    <code className="truncate">{shortPath(result.path)}</code>
                    <span className="grow" />
                    <Text size="1" color={result.success ? "green" : "red"}>
                      {result.message}
                    </Text>
                  </div>
                ))}
              </section>
            )}

            {page === "dashboard" && (
              <div className="page dashboard-page">
                <div className="page-heading">
                  <div>
                    <Text as="div" size="5" weight="bold">
                      本机 Skill 概览
                    </Text>
                    <Text as="div" size="2" color="gray">
                      先看环境状态，再决定安装、同步或修复。
                    </Text>
                  </div>
                  <Button variant="soft" onClick={() => void refresh()}>
                    {busy === "scan" ? <Spinner size="1" /> : <ArrowClockwise />}
                    刷新全部
                  </Button>
                </div>

                <section className="overview-grid" aria-label="环境统计">
                  <div className="overview-card">
                    <span>可用 IDE</span>
                    <strong>{clients.filter((client) => client.supportsSkills).length}</strong>
                    <small>共检测 {clients.filter((client) => client.status !== "notInstalled").length} 个</small>
                  </div>
                  <div className="overview-card">
                    <span>受管理 Skills</span>
                    <strong>{managedCount}</strong>
                    <small>{installations.length} 条安装记录</small>
                  </div>
                  <div className="overview-card">
                    <span>外部 Skills</span>
                    <strong>{externalCount}</strong>
                    <small>可在库存页主动纳管</small>
                  </div>
                  <div className={issueCount || recoverableJournals.length ? "overview-card attention" : "overview-card"}>
                    <span>需要处理</span>
                    <strong>{issueCount + recoverableJournals.length}</strong>
                    <small>修改、风险或待恢复操作</small>
                  </div>
                </section>

                {recoverableJournals.map((journal) => (
                  <Callout.Root color="orange" size="1" key={journal.id}>
                    <Callout.Icon><WarningCircle /></Callout.Icon>
                    <Callout.Text>
                      上次{journal.operationType === "install" ? "安装" : "操作"}未完整结束，
                      涉及 {journal.targets.length} 个目录。
                    </Callout.Text>
                    <Button
                      size="1"
                      color="orange"
                      variant="soft"
                      disabled={busy === `recover:${journal.id}`}
                      onClick={() => void recoverOperation(journal.id)}
                    >
                      {busy === `recover:${journal.id}` && <Spinner size="1" />}
                      恢复到操作前
                    </Button>
                  </Callout.Root>
                ))}

                <section className="panel">
                  <div className="section-caption">
                    <strong>IDE 检测结果</strong>
                    <Badge variant="soft">{clients.length}</Badge>
                  </div>
                  <div className="client-overview-list">
                    {clients.map((client) => (
                      <div className="client-overview-row" key={client.id}>
                        <div className="client-mark">{client.name.slice(0, 1)}</div>
                        <div className="grow min-width-zero">
                          <Flex align="center" gap="2" wrap="wrap">
                            <strong>{client.name}</strong>
                            <StatusBadge client={client} />
                            {client.version && <Text size="1" color="gray">{client.version}</Text>}
                          </Flex>
                          <Text as="div" size="1" className="mono truncate" color="gray">
                            写入 {shortPath(client.globalSkillsPath)}
                          </Text>
                        </div>
                        <Text size="1" color="gray">
                          {client.detectionEvidence?.length ?? 0} 条依据
                        </Text>
                      </div>
                    ))}
                  </div>
                  <Flex justify="between" align="center" wrap="wrap" gap="2">
                    <Text size="1" color="gray">
                      备份策略：每个 Skill 最多 {overview.backupPolicy.maxBackupsPerSkill} 份，保留 {overview.backupPolicy.retentionDays} 天。
                    </Text>
                    <Button variant="soft" onClick={() => setPage("manage")}>打开库存管理</Button>
                  </Flex>
                </section>
              </div>
            )}

            {page === "install" && (
              <div className="page install-page">
                <div className="page-heading">
                  <div>
                    <Text as="div" size="5" weight="bold">
                      批量安装 Skills
                    </Text>
                    <Text as="div" size="2" color="gray">
                      一次检查来源，再为每个 Skill 分配全局 IDE。
                    </Text>
                  </div>
                  <div className="step-indicator" aria-label="安装步骤">
                    <span className={inspection ? "done" : "current"}>1 来源</span>
                    <ArrowRight />
                    <span className={plan ? "done" : inspection ? "current" : ""}>
                      2 分配
                    </span>
                    <ArrowRight />
                    <span className={results.length ? "done" : plan ? "current" : ""}>
                      3 执行
                    </span>
                  </div>
                </div>

                <section className="panel">
                  <div className="panel-title">
                    <div className="number">1</div>
                    <div>
                      <strong>检查 Skill 来源</strong>
                      <Text as="div" size="1" color="gray">
                        可从目录、ZIP 或公开 GitHub 递归发现多个 Skill
                      </Text>
                    </div>
                  </div>
                  <div className="segmented" aria-label="来源类型">
                    {(
                      [
                        ["localDirectory", "本地目录", <FolderOpen key="dir" />],
                        ["localArchive", "ZIP", <Archive key="zip" />],
                        ["github", "GitHub", <GithubLogo key="git" />],
                      ] as const
                    ).map(([mode, label, icon]) => (
                      <button
                        className={sourceMode === mode ? "selected" : ""}
                        key={mode}
                        onClick={() => {
                          setSourceMode(mode);
                          setSourceValue("");
                          resetInspection();
                        }}
                      >
                        {icon}
                        {label}
                      </button>
                    ))}
                  </div>
                  <Flex gap="2" className="source-input-row">
                    <TextField.Root
                      className="grow"
                      value={sourceValue}
                      placeholder={
                        sourceMode === "github"
                          ? "https://github.com/owner/repository"
                          : sourceMode === "localArchive"
                            ? "选择包含 Skills 的 .zip 文件"
                            : "选择单个 Skill 或包含多个 Skills 的目录"
                      }
                      onChange={(event) => {
                        setSourceValue(event.target.value);
                        resetInspection();
                      }}
                    />
                    {sourceMode !== "github" && (
                      <Button variant="soft" onClick={() => void chooseSource()}>
                        浏览…
                      </Button>
                    )}
                    <Button disabled={!source || Boolean(busy)} onClick={() => void inspect()}>
                      {busy === "inspect" && <Spinner size="1" />}
                      检查来源
                    </Button>
                  </Flex>

                  {busy === "inspect" && <LoadingRows />}
                  {inspection && (
                    <>
                      <div className="inspection-summary">
                        <div className="inspection-stat">
                          <strong>{inspection.skills.length}</strong>
                          <span>有效</span>
                        </div>
                        <div className="inspection-stat">
                          <strong>{inspection.rejected.length}</strong>
                          <span>无效</span>
                        </div>
                        <div className="inspection-stat">
                          <strong>
                            {inspection.skills.filter((skill) => skill.hasScripts).length}
                          </strong>
                          <span>含脚本</span>
                        </div>
                        <SearchBox
                          value={sourceSearch}
                          onChange={setSourceSearch}
                          placeholder="搜索 Skill"
                        />
                      </div>
                      <div className="list-toolbar">
                        <Checkbox
                          checked={
                            inspection.skills.length > 0 &&
                            selectedSkillIds.length === inspection.skills.length
                          }
                          onCheckedChange={(value) => {
                            setSelectedSkillIds(
                              value
                                ? inspection.skills.map((skill) => skill.skillId)
                                : [],
                            );
                            setPlan(undefined);
                          }}
                        />
                        <Text size="1" color="gray">
                          已选 {selectedSkillIds.length} / {inspection.skills.length}
                        </Text>
                      </div>
                      <div className="source-skill-list">
                        {filteredSkills.map((skill) => (
                          <label className="source-skill-row" key={skill.skillId}>
                            <Checkbox
                              checked={selectedSkillIds.includes(skill.skillId)}
                              onCheckedChange={(value) =>
                                setSkillSelected(skill.skillId, Boolean(value))
                              }
                            />
                            <div className="skill-icon">
                              <Code size={18} />
                            </div>
                            <div className="grow min-width-zero">
                              <Flex align="center" gap="2" wrap="wrap">
                                <strong>{skill.name}</strong>
                                <Badge color="green" variant="soft">
                                  <Check />
                                  有效
                                </Badge>
                                {skill.hasScripts && (
                                  <Badge color="amber" variant="soft">
                                    <ShieldWarning />
                                    含脚本
                                  </Badge>
                                )}
                                {skill.license && (
                                  <Badge color="gray" variant="soft">
                                    许可证 {skill.license}
                                  </Badge>
                                )}
                                {skill.compatibility && (
                                  <Badge color="gray" variant="soft">
                                    {skill.compatibility}
                                  </Badge>
                                )}
                              </Flex>
                              <Text as="div" size="1" color="gray" className="truncate">
                                {skill.relativePath} · {skill.fileCount} 个文件 ·{" "}
                                {formatBytes(skill.totalBytes)} ·{" "}
                                {skill.contentHash.slice(0, 10)}
                              </Text>
                              <Text as="div" size="1" color="gray" className="truncate">
                                {skill.description}
                              </Text>
                            </div>
                          </label>
                        ))}
                        {inspection.rejected.map((rejected) => (
                          <div className="source-skill-row rejected" key={rejected.relativePath}>
                            <Checkbox disabled />
                            <div className="skill-icon">
                              <WarningCircle size={18} />
                            </div>
                            <div className="grow min-width-zero">
                              <strong>{rejected.relativePath}</strong>
                              <Text as="div" size="1" color="red">
                                {rejected.reason}
                              </Text>
                            </div>
                          </div>
                        ))}
                      </div>
                      {inspection.warnings.map((warning) => (
                        <Callout.Root color="amber" size="1" key={warning}>
                          <Callout.Icon>
                            <ShieldWarning />
                          </Callout.Icon>
                          <Callout.Text>{warning}</Callout.Text>
                        </Callout.Root>
                      ))}
                    </>
                  )}
                </section>

                <section className={`panel ${inspection ? "" : "disabled-panel"}`}>
                  <div className="panel-title">
                    <div className="number">2</div>
                    <div>
                      <strong>分配到 IDE</strong>
                      <Text as="div" size="1" color="gray">
                        新安装仅写入全局目录，IDE 默认全部不选
                      </Text>
                    </div>
                  </div>
                  {busy === "scan" && clients.length === 0 ? (
                    <LoadingRows />
                  ) : selectedSkills.length === 0 ? (
                    <div className="empty-inline">请先选择至少一个有效 Skill</div>
                  ) : (
                    <div className="assignment-matrix">
                      <div className="matrix-head">
                        <span>Skill</span>
                        {targetClients.map((client) => (
                          <button
                            disabled={!client.supportsSkills}
                            key={client.id}
                            onClick={() => toggleClientColumn(client)}
                            title={client.notes.join("；")}
                          >
                            <span>{client.name}</span>
                            <small>
                              {client.supportsSkills
                                ? "整列选择"
                                : detectionLabel[client.status]}
                            </small>
                          </button>
                        ))}
                      </div>
                      {selectedSkills.map((skill) => (
                        <div className="matrix-row" key={skill.skillId}>
                          <div className="matrix-skill">
                            <strong>{skill.name}</strong>
                            <small>{skill.contentHash.slice(0, 8)}</small>
                          </div>
                          <div className="matrix-targets">
                            {targetClients.map((client) => (
                              <label
                                className={!client.supportsSkills ? "unavailable" : ""}
                                key={client.id}
                                title={client.notes.join("；")}
                              >
                                <Checkbox
                                  checked={assignments[skill.skillId]?.includes(client.id)}
                                  disabled={!client.supportsSkills}
                                  onCheckedChange={(value) =>
                                    toggleAssignment(
                                      skill.skillId,
                                      client.id,
                                      Boolean(value),
                                    )
                                  }
                                />
                                <span>{client.name}</span>
                                {!client.supportsSkills && (
                                  <small>{detectionLabel[client.status]}</small>
                                )}
                              </label>
                            ))}
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                  {duplicateNames.length > 0 && (
                    <Callout.Root color="red" size="1">
                      <Callout.Icon>
                        <WarningCircle />
                      </Callout.Icon>
                      <Callout.Text>
                        同名但内容不同，必须只保留一个：{duplicateNames.join("、")}
                      </Callout.Text>
                    </Callout.Root>
                  )}
                  <Flex justify="between" align="center" gap="3">
                    <Text size="1" color="gray">
                      每个已选 Skill 至少分配一个可用 IDE。
                    </Text>
                    <Button
                      disabled={
                        Boolean(busy) ||
                        !assignmentsComplete ||
                        duplicateNames.length > 0
                      }
                      onClick={() => void createPlan()}
                    >
                      {busy === "plan" && <Spinner size="1" />}
                      生成安装预览
                    </Button>
                  </Flex>
                </section>

                {plan && (
                  <section className="panel plan-panel">
                    <div className="panel-title">
                      <div className="number">3</div>
                      <div>
                        <strong>确认物理写入</strong>
                        <Text as="div" size="1" color="gray">
                          {plan.skills.length} 个 Skill，{plan.entries.length} 个目标路径
                        </Text>
                        <Text as="div" size="1" color="gray">
                          预览有效至 {new Date(plan.expiresAt).toLocaleTimeString()}
                        </Text>
                      </div>
                    </div>
                    <div className="plan-list">
                      {plan.entries.map((entry) => {
                        const needsOverwrite = ["conflict", "updateAvailable"].includes(
                          entry.conflict,
                        );
                        return (
                          <div className="plan-entry" key={entry.entryId}>
                            <div className="grow min-width-zero">
                              <Flex align="center" gap="2" wrap="wrap">
                                <strong>{entry.skillName}</strong>
                                <Badge
                                  color={
                                    ["conflict", "notWritable"].includes(entry.conflict)
                                      ? "red"
                                      : entry.conflict === "updateAvailable"
                                        ? "orange"
                                        : entry.conflict === "identical"
                                          ? "gray"
                                          : "green"
                                  }
                                >
                                  {conflictLabel[entry.conflict]}
                                </Badge>
                              </Flex>
                              <Text as="div" size="1" className="mono truncate">
                                {shortPath(entry.resolvedPath)}
                              </Text>
                              <div className="consumer-line">
                                {entry.consumers.map((id) => (
                                  <Badge key={id} variant="soft">
                                    {clients.find((client) => client.id === id)?.name ?? id}
                                  </Badge>
                                ))}
                                {entry.passiveConsumers.length > 0 && (
                                  <Text size="1" color="amber">
                                    可能被{" "}
                                    {entry.passiveConsumers
                                      .map(
                                        (id) =>
                                          clients.find((client) => client.id === id)
                                            ?.name ?? id,
                                      )
                                      .join("、")}
                                    被动发现
                                  </Text>
                                )}
                              </div>
                            </div>
                            {needsOverwrite && (
                              <label className="overwrite">
                                <Checkbox
                                  checked={overwrites.includes(entry.entryId)}
                                  onCheckedChange={(value) =>
                                    setOverwrites((current) =>
                                      value
                                        ? [...current, entry.entryId]
                                        : current.filter((id) => id !== entry.entryId),
                                    )
                                  }
                                />
                                覆盖并备份
                              </label>
                            )}
                          </div>
                        );
                      })}
                    </div>
                    <Flex justify="between" align="center">
                      <Text size="1" color="gray">
                        安装期间不会执行 Skill 中的脚本。
                      </Text>
                      <Button
                        disabled={
                          Boolean(busy) ||
                          plan.entries.some(
                            (entry) =>
                              ["conflict", "updateAvailable"].includes(entry.conflict) &&
                              !overwrites.includes(entry.entryId),
                          )
                        }
                        onClick={() => void applyPlan()}
                      >
                        {busy === "apply" && <Spinner size="1" />}
                        执行批量安装
                      </Button>
                    </Flex>
                  </section>
                )}

                {results.length > 0 && (
                  <section className="panel">
                    <div className="panel-title">
                      <div className="number success">
                        <Check />
                      </div>
                      <strong>操作结果</strong>
                    </div>
                    {results.map((result, index) => (
                      <div className="result-row" key={result.entryId ?? `${result.path}-${index}`}>
                        <span className={result.success ? "result-dot ok" : "result-dot failed"} />
                        <strong>{result.skillName ?? "Skill"}</strong>
                        <code className="truncate">{shortPath(result.path)}</code>
                        <span className="grow" />
                        <Text size="1" color={result.success ? "green" : "red"}>
                          {result.message}
                        </Text>
                      </div>
                    ))}
                  </section>
                )}
              </div>
            )}

            {page === "manage" && (
              <div className="page manage-page">
                <div className="page-heading">
                  <div>
                    <Text as="div" size="5" weight="bold">
                      IDE Skill 库存
                    </Text>
                    <Text as="div" size="2" color="gray">
                      查看每个 IDE 的直接安装、外部内容和被动发现项。
                    </Text>
                  </div>
                  <Flex gap="2" wrap="wrap" justify="end">
                    <Button
                      variant="soft"
                      disabled={busy === "lock-import"}
                      onClick={() => void importLockfile()}
                    >
                      {busy === "lock-import" ? <Spinner size="1" /> : <CloudArrowDown />}
                      从锁文件迁移
                    </Button>
                    <Button
                      variant="soft"
                      disabled={installations.every((item) => item.legacyProject) || busy === "lock-export"}
                      onClick={() => void exportLockfile()}
                    >
                      {busy === "lock-export" ? <Spinner size="1" /> : <Archive />}
                      导出锁文件
                    </Button>
                    <Button
                      variant="soft"
                      disabled={installations.every((item) => item.legacyProject) || busy === "export"}
                      onClick={() => void exportPortableBundle()}
                    >
                      {busy === "export" ? <Spinner size="1" /> : <Archive />}
                      导出便携包
                    </Button>
                    <Button variant="soft" onClick={() => void checkUpdates()}>
                      {busy === "updates" ? <Spinner size="1" /> : <ArrowClockwise />}
                      检查更新
                    </Button>
                    {availableUpdateCount > 0 && (
                      <Button
                        disabled={busy === "update-plan"}
                        onClick={() => void createUpdatePlan()}
                      >
                        {busy === "update-plan" && <Spinner size="1" />}
                        生成更新计划 ({availableUpdateCount})
                      </Button>
                    )}
                  </Flex>
                </div>
                {lockfilePlan && (
                  <section className="panel lockfile-plan-panel">
                    <div className="section-caption">
                      <div>
                        <strong>锁文件迁移预览</strong>
                        <Text as="div" size="1" color="gray">
                          仅安装哈希验证通过且本机 IDE 可用的条目。
                        </Text>
                      </div>
                      <Badge variant="soft">
                        {lockfilePlan.installPlan.entries.length} 个目标
                      </Badge>
                    </div>
                    {lockfilePlan.missingClientIds.length > 0 && (
                      <Callout.Root color="orange" size="1">
                        <Callout.Icon><WarningCircle /></Callout.Icon>
                        <Callout.Text>
                          缺少 IDE：{lockfilePlan.missingClientIds.join("、")}
                        </Callout.Text>
                      </Callout.Root>
                    )}
                    {lockfilePlan.unavailableSkills.map((issue) => (
                      <Callout.Root color="red" size="1" key={`${issue.skillName}-${issue.reason}`}>
                        <Callout.Icon><WarningCircle /></Callout.Icon>
                        <Callout.Text>{issue.skillName}：{issue.reason}</Callout.Text>
                      </Callout.Root>
                    ))}
                    {lockfilePlan.extraInstallationIds.length > 0 && (
                      <Callout.Root color="blue" size="1">
                        <Callout.Icon><Info /></Callout.Icon>
                        <Callout.Text>
                          当前机器额外 {lockfilePlan.extraInstallationIds.length} 条安装记录不会自动删除；请在库存中逐项确认。
                        </Callout.Text>
                      </Callout.Root>
                    )}
                    <div className="plan-list">
                      {lockfilePlan.installPlan.entries.map((entry) => {
                        const needsOverwrite = ["conflict", "updateAvailable"].includes(entry.conflict);
                        return (
                          <div className="plan-entry" key={entry.entryId}>
                            <div className="grow min-width-zero">
                              <strong>{entry.skillName}</strong>
                              <Text as="div" size="1" className="mono truncate" color="gray">
                                {shortPath(entry.resolvedPath)} · {conflictLabel[entry.conflict]}
                              </Text>
                            </div>
                            {needsOverwrite && (
                              <label className="overwrite">
                                <Checkbox
                                  checked={lockfileOverwrites.includes(entry.entryId)}
                                  onCheckedChange={(value) =>
                                    setLockfileOverwrites((current) =>
                                      value
                                        ? [...current, entry.entryId]
                                        : current.filter((id) => id !== entry.entryId),
                                    )
                                  }
                                />
                                覆盖并备份
                              </label>
                            )}
                          </div>
                        );
                      })}
                    </div>
                    <Flex justify="end">
                      <Button
                        disabled={
                          busy === "lock-apply" ||
                          lockfilePlan.installPlan.entries.length === 0 ||
                          lockfilePlan.installPlan.entries.some(
                            (entry) =>
                              ["conflict", "updateAvailable"].includes(entry.conflict) &&
                              !lockfileOverwrites.includes(entry.entryId),
                          )
                        }
                        onClick={() => void applyLockfilePlan()}
                      >
                        {busy === "lock-apply" && <Spinner size="1" />}
                        应用迁移计划
                      </Button>
                    </Flex>
                  </section>
                )}
                {updatePlan && (
                  <section className="panel update-plan-panel">
                    <div className="section-caption">
                      <div>
                        <strong>确认更新内容</strong>
                        <Text as="div" size="1" color="gray">
                          仅更新已勾选项；每个目标会先备份，完成后可回滚。
                        </Text>
                        <Text as="div" size="1" color="gray">
                          预览有效至 {new Date(updatePlan.expiresAt).toLocaleTimeString()}
                        </Text>
                      </div>
                      <Badge variant="soft">{updatePlan.entries.length}</Badge>
                    </div>
                    <div className="plan-list">
                      {updatePlan.entries.map((entry) => {
                        const changes = entry.changes;
                        return (
                          <div className="plan-entry" key={entry.entryId}>
                            <Checkbox
                              aria-label={`确认更新 ${entry.skillName}`}
                              checked={approvedUpdateIds.includes(entry.entryId)}
                              disabled={!entry.requiresConfirmation}
                              onCheckedChange={(value) =>
                                setApprovedUpdateIds((current) =>
                                  value
                                    ? [...current, entry.entryId]
                                    : current.filter((id) => id !== entry.entryId),
                                )
                              }
                            />
                            <div className="grow min-width-zero">
                              <Flex align="center" gap="2" wrap="wrap">
                                <strong>{entry.skillName}</strong>
                                <Badge
                                  color={
                                    entry.status === "targetModified"
                                      ? "orange"
                                      : entry.status === "sourceChanged"
                                        ? "blue"
                                        : "gray"
                                  }
                                  variant="soft"
                                >
                                  {entry.message}
                                </Badge>
                              </Flex>
                              <Text as="div" size="1" className="mono truncate" color="gray">
                                {shortPath(entry.resolvedPath)}
                              </Text>
                              {changes && (
                                <Text as="div" size="1" color="gray">
                                  新增 {changes.added.length} · 修改 {changes.modified.length} · 删除 {changes.removed.length}
                                </Text>
                              )}
                            </div>
                          </div>
                        );
                      })}
                    </div>
                    <Flex justify="end">
                      <Button
                        disabled={
                          busy === "update-apply" ||
                          updatePlan.entries.some(
                            (entry) =>
                              entry.requiresConfirmation &&
                              !approvedUpdateIds.includes(entry.entryId),
                          )
                        }
                        onClick={() => void applyUpdates()}
                      >
                        {busy === "update-apply" && <Spinner size="1" />}
                        应用已确认更新
                      </Button>
                    </Flex>
                  </section>
                )}
                <section className="panel inventory-controls">
                  <SearchBox
                    value={inventorySearch}
                    onChange={setInventorySearch}
                    placeholder="搜索名称或路径"
                  />
                  <div className="filter-tabs" aria-label="库存筛选">
                    {(
                      [
                        ["all", "全部"],
                        ["managed", "受管理"],
                        ["external", "外部"],
                        ["issues", "异常"],
                      ] as const
                    ).map(([value, label]) => (
                      <button
                        className={inventoryFilter === value ? "selected" : ""}
                        key={value}
                        onClick={() => setInventoryFilter(value)}
                      >
                        {label}
                      </button>
                    ))}
                  </div>
                </section>

                {busy === "scan" && environment.inventories.length === 0 ? (
                  <section className="panel">
                    <LoadingRows />
                  </section>
                ) : (
                  environment.inventories.map((inventory) => (
                    <InventoryGroup
                      client={clients.find((client) => client.id === inventory.clientId)}
                      inventory={inventory}
                      filter={inventoryFilter}
                      search={inventorySearch}
                      busy={busy}
                      updates={updates}
                      clients={clients}
                      onAdopt={adopt}
                      onUninstall={uninstallById}
                      onRefresh={refreshClient}
                      onSync={beginSync}
                      pinnedInstallationIds={overview.pinnedInstallationIds}
                      onSetPinned={setPinned}
                      key={inventory.clientId}
                    />
                  ))
                )}

                {legacyInstallations.length > 0 && (
                  <section className="panel legacy-panel">
                    <div className="section-caption">
                      <strong>历史项目安装</strong>
                      <Badge color="orange" variant="soft">
                        {legacyInstallations.length}
                      </Badge>
                    </div>
                    <Text size="1" color="gray">
                      0.2.0 不再创建项目安装；历史记录仍可检查、备份和卸载。
                    </Text>
                    {legacyInstallations.map((item) => (
                      <div className="inventory-row" key={item.id}>
                        <div className="skill-icon">
                          <Code size={18} />
                        </div>
                        <div className="grow min-width-zero">
                          <strong>{item.skillName}</strong>
                          <Text as="div" size="1" className="mono truncate" color="gray">
                            {shortPath(item.resolvedPath)}
                          </Text>
                        </div>
                        <Button
                          size="1"
                          variant="ghost"
                          color="red"
                          disabled={busy === item.id}
                          onClick={() =>
                            void uninstallById(item.id, item.resolvedPath)
                          }
                        >
                          <Trash />
                          卸载
                        </Button>
                      </div>
                    ))}
                  </section>
                )}

              </div>
            )}

            {page === "operations" && (
              <div className="page operations-page">
                <div className="page-heading">
                  <div>
                    <Text as="div" size="5" weight="bold">
                      操作与备份
                    </Text>
                    <Text as="div" size="2" color="gray">
                      集中查看安装生命周期、故障恢复、回滚与备份保留策略。
                    </Text>
                  </div>
                </div>

                <section className="panel">
                  <div className="section-caption">
                    <strong>备份策略</strong>
                    <Badge variant="soft">自动执行</Badge>
                  </div>
                  <div className="policy-grid">
                    <label>
                      <Text size="1" color="gray">每个 Skill 最多</Text>
                      <TextField.Root
                        type="number"
                        min="1"
                        max="100"
                        value={policyDraft.maxBackupsPerSkill}
                        onChange={(event) =>
                          setPolicyDraft((current) => ({
                            ...current,
                            maxBackupsPerSkill: Number(event.target.value),
                          }))
                        }
                      />
                    </label>
                    <label>
                      <Text size="1" color="gray">总空间（MB）</Text>
                      <TextField.Root
                        type="number"
                        min="1"
                        value={policyDraft.maxTotalMb}
                        onChange={(event) =>
                          setPolicyDraft((current) => ({
                            ...current,
                            maxTotalMb: Number(event.target.value),
                          }))
                        }
                      />
                    </label>
                    <label>
                      <Text size="1" color="gray">保留天数</Text>
                      <TextField.Root
                        type="number"
                        min="1"
                        max="3650"
                        value={policyDraft.retentionDays}
                        onChange={(event) =>
                          setPolicyDraft((current) => ({
                            ...current,
                            retentionDays: Number(event.target.value),
                          }))
                        }
                      />
                    </label>
                    <Button
                      disabled={busy === "backup-policy"}
                      onClick={() => void saveBackupPolicy()}
                    >
                      {busy === "backup-policy" && <Spinner size="1" />}
                      保存策略
                    </Button>
                  </div>
                </section>

                <section className="panel">
                  <div className="section-caption">
                    <strong>操作记录</strong>
                    <Badge variant="soft">{overview.operationJournals.length}</Badge>
                  </div>
                  {overview.operationJournals.length === 0 ? (
                    <Text as="div" size="2" color="gray" className="quiet-row">
                      尚无可显示的操作记录。
                    </Text>
                  ) : (
                    overview.operationJournals
                      .slice()
                      .reverse()
                      .map((journal) => {
                        const recoverable = ["partial", "recoveryRequired"].includes(journal.status);
                        const retainedBackupIds = new Set(backups.map((backup) => backup.id));
                        const rollbackable =
                          journal.status === "completed" &&
                          journal.targets.every(
                            (target) =>
                              !target.existedBefore ||
                              (target.backupId !== undefined && retainedBackupIds.has(target.backupId)),
                          );
                        return (
                          <div className="operation-row" key={journal.id}>
                            <ClockCounterClockwise />
                            <div className="grow min-width-zero">
                              <Flex align="center" gap="2" wrap="wrap">
                                <strong>{operationLabels[journal.operationType] ?? journal.operationType}</strong>
                                <Badge
                                  color={
                                    recoverable
                                      ? "orange"
                                      : journal.status === "completed"
                                        ? "green"
                                        : journal.status === "rolledBack"
                                          ? "gray"
                                          : "blue"
                                  }
                                  variant="soft"
                                >
                                  {journal.status === "completed"
                                    ? "已完成"
                                    : journal.status === "rolledBack"
                                      ? "已回滚"
                                      : recoverable
                                        ? "待恢复"
                                        : "进行中"}
                                </Badge>
                              </Flex>
                              <Text as="div" size="1" color="gray">
                                {new Date(journal.createdAt).toLocaleString()} · {journal.targets.length} 个目标
                              </Text>
                              {journal.message && (
                                <Text as="div" size="1" color="gray">{journal.message}</Text>
                              )}
                            </div>
                            {recoverable && (
                              <Button
                                size="1"
                                variant="soft"
                                color="orange"
                                disabled={busy === `recover:${journal.id}`}
                                onClick={() => void recoverOperation(journal.id)}
                              >
                                恢复到操作前
                              </Button>
                            )}
                            {rollbackable && (
                              <Button
                                size="1"
                                variant="soft"
                                disabled={busy === `rollback:${journal.id}`}
                                onClick={() => void rollbackOperation(journal.id)}
                              >
                                回滚此操作
                              </Button>
                            )}
                          </div>
                        );
                      })
                  )}
                </section>

                <section className="panel">
                  <div className="section-caption">
                    <strong>自动备份</strong>
                    <Badge variant="soft">{backups.length}</Badge>
                  </div>
                  {backups.length === 0 ? (
                    <Text as="div" size="2" color="gray" className="quiet-row">
                      覆盖、卸载或恢复前生成的备份会显示在这里。
                    </Text>
                  ) : (
                    backups
                      .slice()
                      .reverse()
                      .map((backup) => (
                        <div className="backup-row" key={backup.id}>
                          <Database />
                          <div className="grow min-width-zero">
                            <Text as="div" size="2" className="mono truncate">
                              {shortPath(backup.originalPath)}
                            </Text>
                            <Text as="div" size="1" color="gray">
                              {new Date(backup.createdAt).toLocaleString()}
                            </Text>
                          </div>
                          <Button
                            size="1"
                            variant="ghost"
                            aria-label="在 Finder 中显示备份"
                            onClick={() => void api.revealInFinder(backup.backupPath)}
                          >
                            <FolderSimple />
                          </Button>
                          <Button
                            size="1"
                            variant="soft"
                            disabled={busy === backup.id}
                            onClick={() => void restore(backup)}
                          >
                            恢复
                          </Button>
                          <Button
                            size="1"
                            variant="ghost"
                            color="red"
                            disabled={busy === `delete-backup:${backup.id}`}
                            onClick={() => void deleteBackup(backup)}
                          >
                            <Trash />
                            删除
                          </Button>
                        </div>
                      ))
                  )}
                </section>
              </div>
            )}

            {page === "diagnostics" && (
              <div className="page">
                <div className="page-heading">
                  <div>
                    <Text as="div" size="5" weight="bold">
                      诊断导出
                    </Text>
                    <Text as="div" size="2" color="gray">
                      导出前可预览；用户目录会替换为 ~。
                    </Text>
                  </div>
                </div>
                <Callout.Root color="blue" size="1">
                  <Callout.Icon>
                    <Info />
                  </Callout.Icon>
                  <Callout.Text>
                    包含 IDE 检测、库存数量、规范和管理状态；不包含 Skill 文件内容，也不会上传数据。
                  </Callout.Text>
                </Callout.Root>
                <section className="panel">
                  <Flex gap="2">
                    <Button onClick={() => void loadDiagnostics()}>
                      {busy === "diagnostics" ? <Spinner size="1" /> : <Gear />}
                      生成预览
                    </Button>
                    <Button
                      variant="soft"
                      disabled={!diagnostics}
                      onClick={() => void navigator.clipboard.writeText(diagnostics)}
                    >
                      <Copy />
                      复制 JSON
                    </Button>
                  </Flex>
                  <Separator size="4" />
                  <pre className="diagnostics">
                    {diagnostics || "诊断尚未生成。点击“生成预览”后可检查并复制。"}
                  </pre>
                </section>
              </div>
            )}
          </main>
        </div>
      </div>
    </Theme>
  );
}

function InventoryGroup({
  client,
  inventory,
  filter,
  search,
  busy,
  updates,
  clients,
  onAdopt,
  onUninstall,
  onRefresh,
  onSync,
  pinnedInstallationIds,
  onSetPinned,
}: {
  client?: DetectedClient;
  inventory: ClientSkillInventory;
  filter: InventoryFilter;
  search: string;
  busy: string;
  updates: UpdateStatus[];
  clients: DetectedClient[];
  onAdopt: (clientId: string, skill: InventorySkill) => Promise<void>;
  onUninstall: (
    installationId: string,
    displayPath: string,
    force?: boolean,
  ) => Promise<void>;
  onRefresh: (clientId: string) => Promise<void>;
  onSync: (skill: InventorySkill) => Promise<void>;
  pinnedInstallationIds: string[];
  onSetPinned: (installationId: string, pinned: boolean) => Promise<void>;
}) {
  if (!client) return null;
  const allSkills = [...inventory.directSkills, ...inventory.passiveSkills];
  const query = search.trim().toLowerCase();
  const visible = allSkills.filter((skill) => {
    const matchesSearch =
      !query ||
      skill.name.toLowerCase().includes(query) ||
      skill.resolvedPath.toLowerCase().includes(query);
    const matchesFilter =
      filter === "all" ||
      (filter === "managed" &&
        ["toolManaged", "adopted", "modified"].includes(skill.managementStatus)) ||
      (filter === "external" && skill.managementStatus === "external") ||
      (filter === "issues" &&
        ["modified", "unsafe"].includes(skill.managementStatus));
    return matchesSearch && matchesFilter;
  });
  const count = (status: SkillManagementStatus[]) =>
    allSkills.filter((skill) => status.includes(skill.managementStatus)).length;

  return (
    <section className="panel inventory-group">
      <div className="inventory-heading">
        <div className="client-mark">{client.name.slice(0, 1)}</div>
        <div className="grow min-width-zero">
          <Flex align="center" gap="2" wrap="wrap">
            <strong>{client.name}</strong>
            <StatusBadge client={client} />
            {client.version && <Text size="1" color="gray">{client.version}</Text>}
          </Flex>
          <Text as="div" size="1" className="mono truncate" color="gray">
            {shortPath(inventory.rootPath)}
          </Text>
        </div>
        <div className="inventory-counts">
          <Badge color="green" variant="soft">
            管理 {count(["toolManaged", "adopted", "modified"])}
          </Badge>
          <Badge color="gray" variant="soft">
            外部 {count(["external"])}
          </Badge>
          <Badge color="red" variant="soft">
            异常 {count(["modified", "unsafe"])}
          </Badge>
          <Badge color="blue" variant="soft">
            被动 {count(["passive"])}
          </Badge>
          <Button
            size="1"
            variant="ghost"
            aria-label={`重新扫描 ${client.name}`}
            disabled={busy === `scan:${client.id}`}
            onClick={() => void onRefresh(client.id)}
          >
            {busy === `scan:${client.id}` ? <Spinner size="1" /> : <ArrowClockwise />}
          </Button>
        </div>
      </div>
      {(client.detectionEvidence?.length ?? 0) > 0 && (
        <details className="detection-evidence">
          <summary>查看检测依据与库存目录</summary>
          <div>
            {client.detectionEvidence?.map((evidence) => (
              <Text as="div" size="1" color="gray" key={`${evidence.kind}-${evidence.path}`}>
                {evidence.message} · <span className="mono">{shortPath(evidence.path)}</span>
                {evidence.version ? ` · ${evidence.version}` : ""}
              </Text>
            ))}
          </div>
        </details>
      )}
      {inventory.scanError ? (
        <Callout.Root color="red" size="1">
          <Callout.Icon>
            <WarningCircle />
          </Callout.Icon>
          <Callout.Text>目录读取失败：{inventory.scanError}</Callout.Text>
        </Callout.Root>
      ) : visible.length === 0 ? (
        <div className="empty-inline">
          {allSkills.length === 0 ? "Skill 目录为空" : "没有符合筛选条件的 Skill"}
        </div>
      ) : (
        <div className="inventory-list">
          {visible.map((skill) => {
            const status = inventoryStatus[skill.managementStatus];
            const update = updates.find(
              (item) => item.installationId === skill.installationId,
            );
            const passiveClient = clients.find(
              (item) => item.id === skill.passiveFromClientId,
            );
            const pinned = Boolean(
              skill.installationId &&
                pinnedInstallationIds.includes(skill.installationId),
            );
            return (
              <div className="inventory-row" key={skill.inventoryId}>
                <div className="skill-icon">
                  <Code size={18} />
                </div>
                <div className="grow min-width-zero">
                  <Flex align="center" gap="2" wrap="wrap">
                    <strong>{skill.name}</strong>
                    <Badge color={status.color} variant="soft">
                      {status.label}
                    </Badge>
                    {skill.validity === "nonConforming" && (
                      <Badge color="orange" variant="soft">
                        非规范
                      </Badge>
                    )}
                  </Flex>
                  <Text as="div" size="1" className="mono truncate" color="gray">
                    {shortPath(skill.resolvedPath)}
                  </Text>
                  {passiveClient && (
                    <Text as="div" size="1" color="blue">
                      来自 {passiveClient.name} 共享目录，仅供查看
                    </Text>
                  )}
                  {skill.issues.map((issue) => (
                    <Text as="div" size="1" color="orange" key={issue}>
                      {issue}
                    </Text>
                  ))}
                  {update && (
                    <div className="update-summary">
                      <Text
                        as="div"
                        size="1"
                        color={update.status === "current" ? "green" : "orange"}
                      >
                        {update.message}
                        {update.sourceRevision ? ` · ${update.sourceRevision.slice(0, 8)}` : ""}
                      </Text>
                      {update.changes &&
                        update.changes.added.length + update.changes.modified.length + update.changes.removed.length > 0 && (
                          <Text as="div" size="1" color="gray">
                            新增 {update.changes.added.length} · 修改 {update.changes.modified.length} · 删除 {update.changes.removed.length}
                          </Text>
                        )}
                    </div>
                  )}
                </div>
                <div className="row-actions">
                  <Button
                    size="1"
                    variant="ghost"
                    aria-label={`复制 ${skill.name} 路径`}
                    onClick={() =>
                      void navigator.clipboard.writeText(skill.resolvedPath)
                    }
                  >
                    <Copy />
                  </Button>
                  {skill.managementStatus !== "passive" && (
                    <Button
                      size="1"
                      variant="ghost"
                      aria-label={`在 Finder 中显示 ${skill.name}`}
                      onClick={() => void api.revealInFinder(skill.resolvedPath)}
                    >
                      <FolderSimple />
                    </Button>
                  )}
                  {skill.managementStatus === "external" && (
                    <Button
                      size="1"
                      variant="soft"
                      disabled={busy === skill.inventoryId}
                      onClick={() => void onAdopt(client.id, skill)}
                    >
                      纳入管理
                    </Button>
                  )}
                  {skill.installationId &&
                    ["toolManaged", "adopted", "modified"].includes(
                      skill.managementStatus,
                    ) && (
                      <>
                        <Button
                          size="1"
                          variant="ghost"
                          aria-label={`${pinned ? "取消固定" : "固定版本"} ${skill.name}`}
                          disabled={busy === `pin:${skill.installationId}`}
                          onClick={() =>
                            void onSetPinned(skill.installationId!, !pinned)
                          }
                        >
                          {pinned ? "取消固定" : "固定版本"}
                        </Button>
                        <Button
                          size="1"
                          variant="ghost"
                          disabled={busy === `sync:${skill.inventoryId}`}
                          onClick={() => void onSync(skill)}
                        >
                          <ArrowRight />
                          同步
                        </Button>
                        <Button
                          size="1"
                          variant="ghost"
                          color="red"
                          disabled={busy === skill.installationId}
                          onClick={() => {
                          const consumers = skill.consumers
                            .map(
                              (id) =>
                                clients.find((item) => item.id === id)?.name ?? id,
                            )
                            .join("、");
                          if (
                            window.confirm(
                              `卸载 ${skill.name}？\n\n将影响：${consumers}\n卸载前会自动备份。`,
                            )
                          ) {
                            void onUninstall(
                              skill.installationId!,
                              skill.resolvedPath,
                            );
                          }
                          }}
                        >
                          <Trash />
                          卸载
                        </Button>
                      </>
                    )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
