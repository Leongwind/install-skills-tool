import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "../src/App";
import { backend } from "../src/api";
import type { EnvironmentScan, SourceInspection } from "../src/types";

vi.mock("../src/api", () => ({
  backend: {
    scanEnvironment: vi.fn(),
    listInstallations: vi.fn(),
    listBackups: vi.fn(),
    inspectSource: vi.fn(),
    planInstall: vi.fn(),
    applyInstallPlan: vi.fn(),
    adoptExternalSkill: vi.fn(),
    revealInExplorer: vi.fn(),
    uninstallInstallation: vi.fn(),
    restoreBackup: vi.fn(),
    exportDiagnostics: vi.fn(),
    chooseDirectory: vi.fn(),
    chooseArchive: vi.fn(),
  },
}));

const environment: EnvironmentScan = {
  clients: [
    {
      id: "cursor", name: "Cursor", edition: "standard", version: "1.7.0",
      status: "installed", globalSkillsPath: "C:\\Users\\tester\\.cursor\\skills",
      supportsSkills: true, notes: [],
    },
    {
      id: "kiro", name: "Kiro", edition: "standard", version: "0.6.0",
      status: "installed", globalSkillsPath: "C:\\Users\\tester\\.kiro\\skills",
      supportsSkills: true, notes: [],
    },
    {
      id: "windsurf", name: "Windsurf", edition: "standard",
      status: "notInstalled", globalSkillsPath: "C:\\Users\\tester\\.codeium\\windsurf\\skills",
      supportsSkills: false, notes: [],
    },
  ],
  inventories: [
    {
      clientId: "cursor", rootPath: "C:\\Users\\tester\\.cursor\\skills", passiveSkills: [],
      directSkills: [{
        inventoryId: "external", name: "review", directoryName: "review",
        resolvedPath: "C:\\Users\\tester\\.cursor\\skills\\review", contentHash: "abc",
        validity: "valid", managementStatus: "external", issues: [], consumers: ["cursor"],
      }],
    },
    { clientId: "kiro", rootPath: "C:\\Users\\tester\\.kiro\\skills", directSkills: [], passiveSkills: [] },
    { clientId: "windsurf", rootPath: "C:\\Users\\tester\\.codeium\\windsurf\\skills", directSkills: [], passiveSkills: [] },
  ],
};

const inspection: SourceInspection = {
  inspectionId: "inspection", source: { kind: "github", url: "https://github.com/acme/skills" },
  rejected: [{ relativePath: "bad", reason: "name 与目录名不一致" }], warnings: [],
  skills: [
    {
      skillId: "alpha", relativePath: "skills/alpha", name: "alpha", description: "Alpha fixture",
      source: { kind: "github", url: "https://github.com/acme/skills" }, preparedPath: "C:\\cache\\alpha",
      contentHash: "1234567890abcdef", fileCount: 3, totalBytes: 1024, hasScripts: false, warnings: [],
    },
    {
      skillId: "beta", relativePath: "skills/beta", name: "beta", description: "Beta fixture",
      source: { kind: "github", url: "https://github.com/acme/skills" }, preparedPath: "C:\\cache\\beta",
      contentHash: "abcdef1234567890", fileCount: 4, totalBytes: 2048, hasScripts: true, warnings: ["script"],
    },
  ],
};

beforeEach(() => {
  vi.mocked(backend.scanEnvironment).mockResolvedValue(environment);
  vi.mocked(backend.listInstallations).mockResolvedValue([]);
  vi.mocked(backend.listBackups).mockResolvedValue([]);
  vi.mocked(backend.inspectSource).mockResolvedValue(inspection);
  vi.mocked(backend.adoptExternalSkill).mockResolvedValue({
    id: "record", skillName: "review", resolvedPath: "path", contentHash: "abc",
    consumers: ["cursor"], passiveConsumers: [], provenance: "adopted",
  });
  vi.mocked(backend.revealInExplorer).mockResolvedValue();
});

describe("Windows desktop interface", () => {
  it("shows the independent Windows version, all source modes and no project install controls", async () => {
    render(<App />);
    expect(await screen.findByText("Windows 0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "本地目录" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ZIP" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "GitHub" })).toBeInTheDocument();
    expect(screen.queryByText(/项目目录|项目安装/)).not.toBeInTheDocument();
    expect(screen.getByText(/不执行 Skill 脚本/)).toBeInTheDocument();
  });

  it("defaults valid skills on but leaves every IDE assignment off", async () => {
    render(<App />);
    await screen.findByText("批量安装 Skills");
    fireEvent.change(screen.getByLabelText("Skill 来源"), { target: { value: "https://github.com/acme/skills" } });
    fireEvent.click(screen.getByRole("button", { name: "检查来源" }));
    expect(await screen.findByText("Alpha fixture")).toBeInTheDocument();
    expect(screen.getByLabelText("搜索 Skill")).toBeInTheDocument();
    const skillRows = screen.getAllByRole("checkbox");
    expect(skillRows.filter((input) => (input as HTMLInputElement).checked)).toHaveLength(2);
    expect(screen.getByRole("button", { name: "生成安装预览" })).toBeDisabled();
    const matrix = screen.getByRole("table", { name: "Skill IDE 分配" });
    const matrixChecks = within(matrix).getAllByRole("checkbox");
    expect(matrixChecks.every((input) => !(input as HTMLInputElement).checked)).toBe(true);
    expect(screen.getAllByText("含脚本")).toHaveLength(2);
    expect(screen.getByText("name 与目录名不一致")).toBeInTheDocument();
  });

  it("groups inventory by IDE and lets an external Skill be adopted", async () => {
    render(<App />);
    await screen.findByText("批量安装 Skills");
    fireEvent.click(screen.getByRole("button", { name: /IDE Skill 库存/ }));
    expect(await screen.findByRole("heading", { name: /Cursor/ })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Windsurf/ })).not.toBeInTheDocument();
    expect(screen.getByText("review")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "在资源管理器中显示 review" }));
    expect(backend.revealInExplorer).toHaveBeenCalledWith("C:\\Users\\tester\\.cursor\\skills\\review");
    const adopt = screen.getByRole("button", { name: "纳入管理" });
    expect(adopt).toBeEnabled();
    fireEvent.click(adopt);
    await waitFor(() => expect(backend.adoptExternalSkill).toHaveBeenCalledWith(
      "cursor", "C:\\Users\\tester\\.cursor\\skills\\review",
    ));
  });
});
