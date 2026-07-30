import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "../src/App";
import type { DetectedClient } from "../src/types";

const clients: DetectedClient[] = [
  {
    id: "trae-international",
    name: "TRAE International",
    edition: "traeInternational",
    version: "3.5.78",
    status: "installed",
    applicationPath: "/Applications/Trae.app",
    globalSkillsPath: "/Users/test/.trae/skills",
    projectSkillsPath: ".trae/skills",
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
    projectSkillsPath: ".trae/skills",
    supportsSkills: false,
    notes: ["原生 Skills 最低版本 3.3.25"],
  },
];

const mocks = vi.hoisted(() => ({
  scanClients: vi.fn(),
  listInstallations: vi.fn(),
  listBackups: vi.fn(),
}));

vi.mock("../src/api", () => ({
  api: {
    scanClients: mocks.scanClients,
    listInstallations: mocks.listInstallations,
    listBackups: mocks.listBackups,
    inspectSkill: vi.fn(),
    planInstall: vi.fn(),
    applyInstallPlan: vi.fn(),
    checkUpdates: vi.fn(),
    uninstall: vi.fn(),
    restoreBackup: vi.fn(),
    exportDiagnostics: vi.fn(),
  },
}));

describe("Skill Installer desktop UI", () => {
  beforeEach(() => {
    mocks.scanClients.mockResolvedValue(clients);
    mocks.listInstallations.mockResolvedValue([]);
    mocks.listBackups.mockResolvedValue([]);
  });

  it("shows TRAE editions as separate targets with version status", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.scanClients).toHaveBeenCalled());

    expect(await screen.findByText("TRAE International")).toBeInTheDocument();
    expect(screen.getByText("TRAE China")).toBeInTheDocument();
    expect(screen.getByText("3.5.78")).toBeInTheDocument();
    expect(screen.getByText("版本过低")).toBeInTheDocument();
    expect(screen.getByText("~/.trae/skills")).toBeInTheDocument();
    expect(screen.getByText("~/.trae-cn/skills")).toBeInTheDocument();
  });
});
