import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DiscoverPage } from "../src/components/DiscoverPage";

const mocks = vi.hoisted(() => ({
  listCatalogSources: vi.fn(),
  listCatalogFavorites: vi.fn(),
  listCollections: vi.fn(),
  searchCatalog: vi.fn(),
  saveCatalogSource: vi.fn(),
  planCollectionInstall: vi.fn(),
  applyInstallPlan: vi.fn(),
}));

vi.mock("../src/api", () => ({ api: mocks }));

const clients = [{
  id: "codex",
  name: "Codex",
  edition: "standard" as const,
  version: "1.0.0",
  status: "installed" as const,
  globalSkillsPath: "/Users/test/.agents/skills",
  supportsSkills: true,
  notes: [],
}];

const entry = {
  id: "anthropics-skills:anthropics/skills:demo",
  sourceId: "anthropics-skills",
  name: "demo",
  description: "Demo skill",
  owner: "anthropics",
  repository: "skills",
  reference: "main",
  path: "skills/demo",
  skillUrl: "https://github.com/anthropics/skills/tree/main/skills/demo",
  hasScripts: false,
  installedState: "notInstalled" as const,
  warnings: [],
};

describe("DiscoverPage catalog workflow", () => {
  beforeEach(() => {
    mocks.listCatalogSources.mockResolvedValue([{ id: "anthropics-skills", name: "Anthropic", url: "https://api.github.com/repos/anthropics/skills/contents/skills?ref=main", provider: "github-contents", enabled: true }]);
    mocks.listCatalogFavorites.mockResolvedValue([]);
    mocks.listCollections.mockResolvedValue([]);
    mocks.searchCatalog.mockResolvedValue([entry]);
    mocks.saveCatalogSource.mockResolvedValue([]);
    mocks.planCollectionInstall.mockResolvedValue({ planId: "plan", createdAt: "now", expiresAt: "later", skills: [], entries: [] });
    mocks.applyInstallPlan.mockResolvedValue([]);
  });

  it("validates custom catalog URLs and surfaces backend errors", async () => {
    mocks.saveCatalogSource.mockRejectedValue(new Error("目录来源目前只支持公开 GitHub HTTPS 地址"));
    render(<DiscoverPage clients={clients} onOpenInstall={vi.fn()} />);
    await screen.findByText("demo");
    fireEvent.change(screen.getByPlaceholderText("来源名称"), { target: { value: "Private" } });
    fireEvent.change(screen.getByPlaceholderText("https://api.github.com/repos/.../contents"), { target: { value: "https://example.com/catalog" } });
    fireEvent.click(screen.getByRole("button", { name: "添加来源" }));
    expect(await screen.findByText("目录来源目前只支持公开 GitHub HTTPS 地址")).toBeInTheDocument();
  });

  it("sends a saved collection through the collection preview seam", async () => {
    mocks.listCollections.mockResolvedValue([{
      id: "collection",
      name: "Coding",
      skillRefs: [entry.id],
      sourceRefs: [{ catalogEntryId: entry.id, source: { kind: "github", url: entry.skillUrl }, sourceDetails: { owner: entry.owner, repository: entry.repository, reference: entry.reference, subpath: entry.path }, skillName: entry.name, path: entry.path }],
      defaultClientIds: ["codex"],
      createdAt: "now",
      updatedAt: "now",
    }]);
    mocks.planCollectionInstall.mockResolvedValue({ planId: "plan", createdAt: "now", expiresAt: "later", skills: [entry], entries: [{ entryId: "target", skillId: "demo", skillName: "demo", resolvedPath: "/Users/test/.agents/skills/demo", consumers: ["codex"], passiveConsumers: [], conflict: "notInstalled", warnings: [] }] });
    render(<DiscoverPage clients={clients} onOpenInstall={vi.fn()} />);
    await screen.findByText("Coding");
    fireEvent.click(screen.getAllByRole("button", { name: "使用集合安装" })[0]);
    expect(await screen.findByText("分配集合 Skill 到 IDE")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "生成安装预览" }));
    await waitFor(() => expect(mocks.planCollectionInstall).toHaveBeenCalledWith({
      collectionId: "collection",
      assignments: [{ skillId: entry.id, clientIds: ["codex"] }],
    }));
    expect(await screen.findByText("集合安装预览")).toBeInTheDocument();
    expect(screen.getAllByText("demo").length).toBeGreaterThanOrEqual(2);
  });

  it("requires every collection member to keep at least one IDE target", async () => {
    mocks.planCollectionInstall.mockClear();
    mocks.listCollections.mockResolvedValue([{
      id: "collection",
      name: "Coding",
      skillRefs: [entry.id],
      sourceRefs: [{ catalogEntryId: entry.id, source: { kind: "github", url: entry.skillUrl }, sourceDetails: { owner: entry.owner, repository: entry.repository, reference: entry.reference, subpath: entry.path }, skillName: entry.name, path: entry.path }],
      defaultClientIds: ["codex"],
      createdAt: "now",
      updatedAt: "now",
    }]);
    render(<DiscoverPage clients={clients} onOpenInstall={vi.fn()} />);
    await screen.findByText("Coding");
    fireEvent.click(screen.getAllByRole("button", { name: "使用集合安装" })[0]);
    const target = await screen.findByRole("checkbox", { name: "Codex" });
    fireEvent.click(target);
    fireEvent.click(screen.getByRole("button", { name: "生成安装预览" }));
    expect(await screen.findByText("集合中每个 Skill 至少需要分配一个可用 IDE")).toBeInTheDocument();
    expect(mocks.planCollectionInstall).not.toHaveBeenCalled();
  });
});
