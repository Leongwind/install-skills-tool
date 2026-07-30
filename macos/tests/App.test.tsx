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
}));

vi.mock("../src/api", () => ({
  api: {
    scanEnvironment: mocks.scanEnvironment,
    listInstallations: mocks.listInstallations,
    listBackups: mocks.listBackups,
    inspectSource: mocks.inspectSource,
    planInstall: mocks.planInstall,
    adoptExternalSkill: mocks.adoptExternalSkill,
    applyInstallPlan: vi.fn(),
    checkUpdates: vi.fn().mockResolvedValue([]),
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
    mocks.inspectSource.mockResolvedValue(inspection);
    mocks.adoptExternalSkill.mockResolvedValue({});
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  it("offers directory, ZIP and GitHub sources without project install controls", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.scanEnvironment).toHaveBeenCalled());

    expect(screen.getByRole("button", { name: "本地目录" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ZIP" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "GitHub" })).toBeInTheDocument();
    expect(screen.queryByText("项目仅指定项目目录")).not.toBeInTheDocument();
    expect(screen.queryByText("选择项目根目录")).not.toBeInTheDocument();
  });

  it("discovers multiple skills, keeps rejected rows and leaves IDEs unassigned", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.scanEnvironment).toHaveBeenCalled());
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
});
