import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "../src/App";
import type {
  DetectedClient,
  EnvironmentScan,
  SourceInspection,
} from "../src/types";

const clients: DetectedClient[] = [
  {
    id: "kiro",
    name: "Kiro",
    edition: "standard",
    version: "1.0.228",
    status: "installed",
    applicationPath: "/Applications/Kiro.app",
    globalSkillsPath: "/Users/test/.kiro/skills",
    inventorySkillsPaths: ["/Users/test/.kiro/skills"],
    detectionEvidence: [
      {
        kind: "application",
        path: "/Applications/Kiro.app",
        version: "1.0.228",
        message: "检测到 macOS 应用",
      },
    ],
    supportsSkills: true,
    notes: [],
  },
  {
    id: "trae-international",
    name: "TRAE International",
    edition: "traeInternational",
    version: "3.5.78",
    status: "installed",
    applicationPath: "/Applications/Trae.app",
    globalSkillsPath: "/Users/test/.trae/skills",
    supportsSkills: true,
    notes: [],
  },
  {
    id: "trae-china",
    name: "TRAE China",
    edition: "traeChina",
    version: "3.3.24",
    status: "unsupportedVersion",
    applicationPath: "/Applications/Trae CN.app",
    globalSkillsPath: "/Users/test/.trae-cn/skills",
    supportsSkills: false,
    notes: ["原生 Skills 最低版本 3.3.25"],
  },
];

const environment: EnvironmentScan = {
  clients,
  inventories: clients.map((client) => ({
    clientId: client.id,
    rootPath: client.globalSkillsPath,
    directSkills:
      client.id === "kiro"
        ? [
            {
              inventoryId: "external-id",
              name: "external",
              directoryName: "external",
              resolvedPath: "/Users/test/.kiro/skills/external",
              contentHash: "external-hash",
              validity: "valid",
              managementStatus: "external",
              issues: [],
              consumers: ["kiro"],
            },
          ]
        : [],
    passiveSkills: [],
  })),
};

const inspection: SourceInspection = {
  inspectionId: "inspection-id",
  source: { kind: "github", url: "https://github.com/acme/skills" },
  skills: [
    {
      skillId: "one-id",
      relativePath: "skills/one",
      name: "one",
      description: "First skill",
      source: { kind: "github", url: "https://github.com/acme/skills" },
      sourceDetails: { subpath: "skills/one" },
      preparedPath: "/tmp/skills/one",
      contentHash: "111111111111",
      fileCount: 2,
      totalBytes: 1200,
      hasScripts: false,
      warnings: [],
    },
    {
      skillId: "two-id",
      relativePath: "skills/two",
      name: "two",
      description: "Second skill",
      source: { kind: "github", url: "https://github.com/acme/skills" },
      sourceDetails: { subpath: "skills/two" },
      preparedPath: "/tmp/skills/two",
      contentHash: "222222222222",
      fileCount: 3,
      totalBytes: 2400,
      hasScripts: true,
      warnings: ["包含脚本"],
    },
  ],
  rejected: [{ relativePath: "broken", reason: "缺少 description" }],
  warnings: [],
};

const mocks = vi.hoisted(() => ({
  scanEnvironment: vi.fn(),
  listInstallations: vi.fn(),
  listBackups: vi.fn(),
  inspectSource: vi.fn(),
  planInstall: vi.fn(),
  adoptExternalSkill: vi.fn(),
  getAppOverview: vi.fn(),
  scanClientInventory: vi.fn(),
  recoverOperation: vi.fn(),
  rollbackOperation: vi.fn(),
  exportSkillBundle: vi.fn(),
  checkUpdates: vi.fn(),
  planUpdates: vi.fn(),
  applyUpdatePlan: vi.fn(),
  setInstallationPinned: vi.fn(),
  exportLockfile: vi.fn(),
  planLockfileImport: vi.fn(),
  applyInstallPlan: vi.fn(),
  openDialog: vi.fn(),
  saveDialog: vi.fn(),
  setBackupPolicy: vi.fn(),
  deleteBackup: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: mocks.openDialog,
  save: mocks.saveDialog,
}));

vi.mock("../src/api", () => ({
  api: {
    scanEnvironment: mocks.scanEnvironment,
    listInstallations: mocks.listInstallations,
    listBackups: mocks.listBackups,
    getAppOverview: mocks.getAppOverview,
    scanClientInventory: mocks.scanClientInventory,
    recoverOperation: mocks.recoverOperation,
    rollbackOperation: mocks.rollbackOperation,
    exportSkillBundle: mocks.exportSkillBundle,
    exportLockfile: mocks.exportLockfile,
    planLockfileImport: mocks.planLockfileImport,
    inspectSource: mocks.inspectSource,
    planInstall: mocks.planInstall,
    adoptExternalSkill: mocks.adoptExternalSkill,
    applyInstallPlan: mocks.applyInstallPlan,
    checkUpdates: mocks.checkUpdates,
    planUpdates: mocks.planUpdates,
    applyUpdatePlan: mocks.applyUpdatePlan,
    setInstallationPinned: mocks.setInstallationPinned,
    setBackupPolicy: mocks.setBackupPolicy,
    deleteBackup: mocks.deleteBackup,
    uninstall: vi.fn(),
    restoreBackup: vi.fn(),
    exportDiagnostics: vi.fn(),
    revealInFinder: vi.fn(),
  },
}));

describe("Skill Installer desktop UI", () => {
  afterEach(() => cleanup());

  beforeEach(() => {
    mocks.scanEnvironment.mockResolvedValue(environment);
    mocks.listInstallations.mockResolvedValue([]);
    mocks.listBackups.mockResolvedValue([]);
    mocks.getAppOverview.mockResolvedValue({
      backupPolicy: {
        maxBackupsPerSkill: 5,
        maxTotalBytes: 1024 * 1024 * 1024,
        retentionDays: 90,
      },
      operationJournals: [],
      pinnedInstallationIds: [],
    });
    mocks.inspectSource.mockResolvedValue(inspection);
    mocks.adoptExternalSkill.mockResolvedValue({});
    mocks.checkUpdates.mockResolvedValue([]);
    mocks.setInstallationPinned.mockResolvedValue(undefined);
    mocks.openDialog.mockResolvedValue(null);
    mocks.saveDialog.mockResolvedValue(null);
    mocks.rollbackOperation.mockResolvedValue([]);
    mocks.setBackupPolicy.mockResolvedValue(undefined);
    mocks.deleteBackup.mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  it("offers directory, ZIP and GitHub sources without project install controls", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.scanEnvironment).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "批量安装" }));

    expect(screen.getByRole("button", { name: "本地目录" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ZIP" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "GitHub" })).toBeInTheDocument();
    expect(screen.queryByText("项目仅指定项目目录")).not.toBeInTheDocument();
    expect(screen.queryByText("选择项目根目录")).not.toBeInTheDocument();
  });

  it("discovers multiple skills, keeps rejected rows and leaves IDEs unassigned", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.scanEnvironment).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "批量安装" }));
    fireEvent.click(screen.getByRole("button", { name: "GitHub" }));
    fireEvent.change(
      screen.getByPlaceholderText("https://github.com/owner/repository"),
      { target: { value: "https://github.com/acme/skills" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "检查来源" }));

    expect(await screen.findByText("First skill")).toBeInTheDocument();
    expect(screen.getByText("Second skill")).toBeInTheDocument();
    expect(screen.getByText("缺少 description")).toBeInTheDocument();
    expect(screen.getByText("已选 2 / 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成安装预览" })).toBeDisabled();

    const search = screen.getByPlaceholderText("搜索 Skill");
    expect(search.closest(".inspection-summary")).not.toBeNull();
    expect(search.closest(".inspection-stat")).toBeNull();
    fireEvent.change(search, { target: { value: "two" } });
    expect(screen.queryByText("First skill")).not.toBeInTheDocument();
    expect(screen.getByText("Second skill")).toBeInTheDocument();
  });

  it("groups inventory by IDE and allows an external skill to be adopted", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.scanEnvironment).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: /IDE Skill 库存/ }));

    expect(await screen.findByText("TRAE International")).toBeInTheDocument();
    expect(screen.getByText("TRAE China")).toBeInTheDocument();
    expect(screen.getByText("版本过低")).toBeInTheDocument();
    expect(screen.getByText("外部安装")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "纳入管理" }));

    await waitFor(() =>
      expect(mocks.adoptExternalSkill).toHaveBeenCalledWith(
        "kiro",
        "/Users/test/.kiro/skills/external",
      ),
    );
  });

  it("opens on an environment overview with transparent detection evidence", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.scanEnvironment).toHaveBeenCalled());

    expect(screen.getByText("本机 Skill 概览")).toBeInTheDocument();
    expect(screen.getByText("可用 IDE")).toBeInTheDocument();
    expect(screen.getByText("1 条依据")).toBeInTheDocument();
    expect(screen.getByText("每个 Skill 最多 5 份", { exact: false })).toBeInTheDocument();
  });

  it("offers recovery for an interrupted operation", async () => {
    mocks.getAppOverview.mockResolvedValueOnce({
      backupPolicy: {
        maxBackupsPerSkill: 5,
        maxTotalBytes: 1024 * 1024 * 1024,
        retentionDays: 90,
      },
      operationJournals: [
        {
          id: "journal-id",
          operationType: "install",
          createdAt: "2026-08-27T00:00:00Z",
          status: "recoveryRequired",
          targets: [
            {
              path: "/Users/test/.agents/skills/demo",
              existedBefore: false,
              completed: true,
            },
          ],
        },
      ],
      pinnedInstallationIds: [],
    });
    mocks.recoverOperation.mockResolvedValue([]);
    render(<App />);

    const button = await screen.findByRole("button", { name: "恢复到操作前" });
    fireEvent.click(button);

    await waitFor(() =>
      expect(mocks.recoverOperation).toHaveBeenCalledWith("journal-id"),
    );
  });

  it("previews source changes before applying a managed Skill update", async () => {
    const managedEnvironment: EnvironmentScan = {
      ...environment,
      inventories: environment.inventories.map((inventory) =>
        inventory.clientId === "kiro"
          ? {
              ...inventory,
              directSkills: [
                ...inventory.directSkills,
                {
                  inventoryId: "managed-id",
                  name: "demo",
                  directoryName: "demo",
                  resolvedPath: "/Users/test/.kiro/skills/demo",
                  contentHash: "old-hash",
                  validity: "valid",
                  managementStatus: "toolManaged",
                  installationId: "installation-id",
                  issues: [],
                  consumers: ["kiro"],
                },
              ],
            }
          : inventory,
      ),
    };
    mocks.scanEnvironment.mockResolvedValueOnce(managedEnvironment);
    mocks.listInstallations.mockResolvedValueOnce([
      {
        id: "installation-id",
        skillName: "demo",
        resolvedPath: "/Users/test/.kiro/skills/demo",
        source: { kind: "github", url: "https://github.com/acme/skills" },
        sourceDetails: { subpath: "skills/demo" },
        contentHash: "old-hash",
        scope: "global",
        consumers: ["kiro"],
        passiveConsumers: [],
        adapterVersion: 1,
        installedAt: "2026-08-27T00:00:00Z",
        provenance: "tool",
        legacyProject: false,
      },
    ]);
    mocks.checkUpdates.mockResolvedValueOnce([
      {
        installationId: "installation-id",
        status: "sourceChanged",
        message: "来源有新内容",
        currentHash: "old-hash",
        sourceHash: "new-hash",
        sourceRevision: "abcdef123456",
        changes: { added: ["README.md"], modified: ["SKILL.md"], removed: [] },
      },
    ]);
    mocks.planUpdates.mockResolvedValueOnce({
      planId: "update-plan-id",
      entries: [
        {
          entryId: "update-entry-id",
          installationId: "installation-id",
          skillName: "demo",
          resolvedPath: "/Users/test/.kiro/skills/demo",
          status: "sourceChanged",
          message: "来源有新内容",
          currentHash: "old-hash",
          sourceHash: "new-hash",
          sourceRevision: "abcdef123456",
          changes: { added: ["README.md"], modified: ["SKILL.md"], removed: [] },
          requiresConfirmation: true,
        },
      ],
    });
    mocks.applyUpdatePlan.mockResolvedValueOnce([
      {
        entryId: "update-entry-id",
        skillName: "demo",
        path: "/Users/test/.kiro/skills/demo",
        success: true,
        status: "updated",
        message: "更新完成，可在操作中心回滚",
      },
    ]);

    render(<App />);
    await waitFor(() => expect(mocks.scanEnvironment).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: /IDE Skill 库存/ }));
    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));

    expect(await screen.findByText(/来源有新内容/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "生成更新计划 (1)" }));
    expect(await screen.findByText("确认更新内容")).toBeInTheDocument();
    expect(screen.getAllByText("新增 1 · 修改 1 · 删除 0")).toHaveLength(2);
    const applyButton = screen.getByRole("button", { name: "应用已确认更新" });
    expect(applyButton).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "确认更新 demo" }));
    fireEvent.click(applyButton);

    await waitFor(() =>
      expect(mocks.applyUpdatePlan).toHaveBeenCalledWith("update-plan-id", [
        "update-entry-id",
      ]),
    );
  });

  it("previews lockfile migration gaps without deleting extra Skills", async () => {
    mocks.openDialog.mockResolvedValueOnce("/tmp/skills.lock.json");
    mocks.planLockfileImport.mockResolvedValueOnce({
      installPlan: { planId: "lock-plan", skills: [], entries: [] },
      missingClientIds: ["cursor"],
      unavailableSkills: [
        { skillName: "private-skill", reason: "来源未绑定" },
      ],
      extraInstallationIds: ["extra-id"],
    });

    render(<App />);
    await waitFor(() => expect(mocks.scanEnvironment).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: /IDE Skill 库存/ }));
    fireEvent.click(screen.getByRole("button", { name: "从锁文件迁移" }));

    expect(await screen.findByText("锁文件迁移预览")).toBeInTheDocument();
    expect(screen.getByText(/缺少 IDE：cursor/)).toBeInTheDocument();
    expect(screen.getByText(/private-skill：来源未绑定/)).toBeInTheDocument();
    expect(
      screen.getByText(/额外 1 条安装记录不会自动删除/),
    ).toBeInTheDocument();
    expect(mocks.planLockfileImport).toHaveBeenCalledWith(
      "/tmp/skills.lock.json",
    );
  });

  it("centralizes completed operations, rollback and backup policy", async () => {
    mocks.listBackups.mockResolvedValueOnce([
      {
        id: "backup-id",
        originalPath: "/Users/test/.agents/skills/demo",
        backupPath: "/tmp/backups/backup-id",
        createdAt: "2026-08-27T00:00:00Z",
      },
    ]);
    mocks.getAppOverview.mockResolvedValueOnce({
      backupPolicy: {
        maxBackupsPerSkill: 5,
        maxTotalBytes: 1024 * 1024 * 1024,
        retentionDays: 90,
      },
      operationJournals: [
        {
          id: "completed-update",
          operationType: "update",
          createdAt: "2026-08-27T00:00:00Z",
          finishedAt: "2026-08-27T00:01:00Z",
          status: "completed",
          targets: [
            {
              path: "/Users/test/.agents/skills/demo",
              existedBefore: true,
              backupId: "backup-id",
              completed: true,
            },
          ],
        },
      ],
      pinnedInstallationIds: [],
    });

    render(<App />);
    await waitFor(() => expect(mocks.getAppOverview).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "操作中心" }));

    expect(screen.getByText("操作与备份")).toBeInTheDocument();
    expect(screen.getByText("更新")).toBeInTheDocument();
    expect(screen.getByText("自动备份")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "回滚此操作" }));

    await waitFor(() =>
      expect(mocks.rollbackOperation).toHaveBeenCalledWith("completed-update"),
    );
  });
});
