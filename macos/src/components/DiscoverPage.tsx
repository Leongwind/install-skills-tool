import {
  ArrowClockwise,
  ArrowRight,
  Check,
  Code,
  GithubLogo,
  Heart,
  MagnifyingGlass,
  ShieldWarning,
  Star,
  WarningCircle,
} from "@phosphor-icons/react";
import { Badge, Button, Callout, Checkbox, Flex, Spinner, Text } from "@radix-ui/themes";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { CatalogEntry, CatalogSource, DetectedClient, InstallPlan, SkillCollection } from "../types";
import { shortPath } from "../ui";

type Props = {
  clients: DetectedClient[];
  onOpenInstall: (entry: CatalogEntry) => void;
};

const stateLabels: Record<CatalogEntry["installedState"], string> = {
  notInstalled: "未安装",
  partial: "部分安装",
  installed: "已安装",
  updateAvailable: "有更新",
};

/** Public catalog browser.  Network failures are intentionally non-fatal:
 * the last valid snapshot remains searchable and is labelled as offline. */
export function DiscoverPage({ clients, onOpenInstall }: Props) {
  const [sources, setSources] = useState<CatalogSource[]>([]);
  const [entries, setEntries] = useState<CatalogEntry[]>([]);
  const [query, setQuery] = useState("");
  const [sourceId, setSourceId] = useState("");
  const [scriptsOnly, setScriptsOnly] = useState(false);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [favorites, setFavorites] = useState<string[]>([]);
  const [selected, setSelected] = useState<CatalogEntry>();
  const [collections, setCollections] = useState<SkillCollection[]>([]);
  const [selectedForCollection, setSelectedForCollection] = useState<string[]>([]);
  const [collectionName, setCollectionName] = useState("");
  const [collectionPlan, setCollectionPlan] = useState<InstallPlan>();
  const [collectionOverwriteIds, setCollectionOverwriteIds] = useState<string[]>([]);
  const [newSourceName, setNewSourceName] = useState("");
  const [newSourceUrl, setNewSourceUrl] = useState("");
  const searchGeneration = useRef(0);
  const detailRef = useRef<HTMLElement>(null);

  const available = typeof (api as Partial<typeof api>).listCatalogSources === "function";

  const loadSources = useCallback(async () => {
    if (!available) return;
    setBusy("sources");
    setError("");
    try {
      const nextSources = await api.listCatalogSources();
      setSources(nextSources);
      if (!sourceId && nextSources[0]) setSourceId(nextSources[0].id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy("");
    }
  }, [available, sourceId]);

  const loadFavorites = useCallback(async () => {
    if (!available || typeof api.listCatalogFavorites !== "function") return;
    try {
      setFavorites(await api.listCatalogFavorites());
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [available]);

  const loadCollections = useCallback(async () => {
    if (!available || typeof api.listCollections !== "function") return;
    try {
      setCollections(await api.listCollections());
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [available]);

  const search = useCallback(async () => {
    if (!available || typeof api.searchCatalog !== "function") return;
    const generation = ++searchGeneration.current;
    setBusy("search");
    setError("");
    try {
      const nextEntries = await api.searchCatalog({
          query: query || undefined,
          sourceId: sourceId || undefined,
          scriptsOnly,
        });
      if (generation !== searchGeneration.current) return;
      setEntries(nextEntries);
    } catch (reason) {
      if (generation !== searchGeneration.current) return;
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (generation === searchGeneration.current) setBusy("");
    }
  }, [available, query, scriptsOnly, sourceId]);

  useEffect(() => {
    void loadSources();
    void loadCollections();
    void loadFavorites();
  }, [loadCollections, loadFavorites, loadSources]);

  useEffect(() => {
    void search();
  }, [search]);

  useEffect(() => {
    if (!selected) return;
    detailRef.current?.focus();
  }, [selected]);

  async function sync(source: CatalogSource) {
    if (typeof api.syncCatalog !== "function") return;
    setBusy(`sync:${source.id}`);
    setError("");
    setNotice("");
    try {
      const result = await api.syncCatalog(source.id);
      setEntries(result.entries);
      setNotice(result.warning ?? `已同步 ${result.entries.length} 个目录条目。`);
      await loadSources();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy("");
    }
  }

  async function toggleFavorite(entry: CatalogEntry) {
    if (typeof api.setCatalogFavorite !== "function") return;
    const next = !favorites.includes(entry.id);
    try {
      setFavorites(await api.setCatalogFavorite(entry.id, next));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function addCatalogSource() {
    if (typeof api.saveCatalogSource !== "function") return;
    const name = newSourceName.trim();
    const url = newSourceUrl.trim();
    if (!name || !url) {
      setError("请填写目录名称和公开 HTTPS URL");
      return;
    }
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== "https:" || !["github.com", "api.github.com", "raw.githubusercontent.com"].includes(parsed.hostname)) {
        throw new Error("目录来源目前只支持公开 GitHub HTTPS 地址");
      }
      const provider = parsed.hostname === "api.github.com" && parsed.pathname.includes("/contents") ? "github-contents" : "github-json";
      setSources(await api.saveCatalogSource({
        id: `github-${crypto.randomUUID()}`,
        name,
        url: parsed.toString(),
        provider,
        enabled: true,
      }));
      setNewSourceName("");
      setNewSourceUrl("");
      setNotice("目录来源已添加，请同步后查看条目。");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function updateCatalogSource(source: CatalogSource, patch: Partial<CatalogSource>) {
    if (typeof api.saveCatalogSource !== "function") return;
    try {
      setSources(await api.saveCatalogSource({ ...source, ...patch }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function saveCurrentCollection() {
    if (!collectionName.trim() || selectedForCollection.length === 0 || typeof api.saveCollection !== "function") return;
    const now = new Date().toISOString();
    const sourceRefs = entries
      .filter((entry) => selectedForCollection.includes(entry.id) && entry.skillUrl)
      .map((entry) => ({
        catalogEntryId: entry.id,
        source: { kind: "github" as const, url: entry.skillUrl as string },
        sourceDetails: {
          owner: entry.owner,
          repository: entry.repository,
          reference: entry.reference,
          subpath: entry.path,
          commitSha: entry.commitSha,
        },
        skillName: entry.name,
        path: entry.path,
        commitSha: entry.commitSha,
      }));
    if (sourceRefs.length !== selectedForCollection.length) {
      setError("有目录条目缺少已确认的公开 Skill URL，无法纳入集合");
      return;
    }
    try {
      const next = await api.saveCollection({
        id: crypto.randomUUID(),
        name: collectionName.trim(),
        skillRefs: selectedForCollection,
        sourceRefs,
        defaultClientIds: clients.filter((client) => client.supportsSkills).map((client) => client.id),
        createdAt: now,
        updatedAt: now,
      });
      setCollections(next);
      setCollectionName("");
      setSelectedForCollection([]);
      setNotice("集合已保存，可在下次安装时复用。");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function removeCollection(collection: SkillCollection) {
    if (typeof api.deleteCollection !== "function") return;
    try {
      setCollections(await api.deleteCollection(collection.id));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function planCollection(collection: SkillCollection) {
    if (typeof api.planCollectionInstall !== "function") return;
    setBusy(`collection:${collection.id}`);
    setError("");
    try {
      const plan = await api.planCollectionInstall({ collectionId: collection.id });
      setCollectionPlan(plan);
      setCollectionOverwriteIds([]);
      setNotice(`已生成 ${plan.skills.length} 个 Skill 的安装预览，请确认目标 IDE。`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy("");
    }
  }

  async function applyCollectionPlan() {
    if (!collectionPlan || typeof api.applyInstallPlan !== "function") return;
    setBusy("collection-apply");
    setError("");
    try {
      const results = await api.applyInstallPlan(collectionPlan.planId, collectionOverwriteIds);
      const failed = results.filter((result) => !result.success).length;
      setNotice(failed ? `集合安装完成，${failed} 个目标失败。` : "集合安装完成，库存将自动刷新。");
      setCollectionPlan(undefined);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy("");
    }
  }

  const categories = useMemo(
    () => [...new Set(entries.map((entry) => entry.path?.split("/")[0]).filter(Boolean))] as string[],
    [entries],
  );

  if (!available) {
    return (
      <div className="page discover-page">
        <div className="page-heading">
          <div>
            <Text as="div" size="5" weight="bold">发现 Skills</Text>
            <Text as="div" size="2" color="gray">当前运行在兼容模式，目录服务将在下次启动后可用。</Text>
          </div>
        </div>
        <Callout.Root color="blue"><Callout.Icon><WarningCircle /></Callout.Icon><Callout.Text>目录 API 尚未接入当前运行时。</Callout.Text></Callout.Root>
      </div>
    );
  }

  return (
    <div className="page discover-page">
      <div className="page-heading">
        <div>
          <Text as="div" size="5" weight="bold">发现 Skills</Text>
          <Text as="div" size="2" color="gray">浏览可信公开目录，查看详情后分配到全局 IDE。</Text>
        </div>
        <Flex gap="2" wrap="wrap" justify="end">
          {sources.map((source) => (
            <Button key={source.id} size="2" variant="soft" disabled={busy === `sync:${source.id}`} onClick={() => void sync(source)}>
              {busy === `sync:${source.id}` ? <Spinner size="1" /> : <ArrowClockwise />} 同步 {source.name}
            </Button>
          ))}
        </Flex>
      </div>
      {error && <Callout.Root color="red" size="1"><Callout.Icon><WarningCircle /></Callout.Icon><Callout.Text>{error}</Callout.Text></Callout.Root>}
      {notice && !error && <Callout.Root color="green" size="1"><Callout.Icon><Check /></Callout.Icon><Callout.Text>{notice}</Callout.Text></Callout.Root>}

      <section className="panel discover-toolbar" aria-label="目录筛选">
        <div className="discover-search">
          <MagnifyingGlass />
          <input value={query} placeholder="搜索名称、描述或仓库" onChange={(event) => setQuery(event.target.value)} />
        </div>
        <select aria-label="来源" value={sourceId} onChange={(event) => setSourceId(event.target.value)}>
          <option value="">全部来源</option>
          {sources.map((source) => <option key={source.id} value={source.id}>{source.name}</option>)}
        </select>
        <label className="discover-check"><Checkbox checked={scriptsOnly} onCheckedChange={(value) => setScriptsOnly(Boolean(value))} /> 含脚本</label>
        <Text size="1" color="gray">{busy === "search" ? "正在搜索…" : `${entries.length} 个结果`}</Text>
      </section>

      <section className="panel catalog-source-panel" aria-label="目录来源管理">
        <div className="section-caption"><div><strong>公开目录来源</strong><Text as="div" size="1" color="gray">仅支持无需登录的 GitHub HTTPS 目录；skills.sh 仅作为浏览入口，不在桌面端伪装同步。</Text></div></div>
        <Flex gap="2" wrap="wrap" align="center">
          <input className="source-name-input" value={newSourceName} placeholder="来源名称" onChange={(event) => setNewSourceName(event.target.value)} />
          <input className="source-url-input" value={newSourceUrl} placeholder="https://api.github.com/repos/.../contents" onChange={(event) => setNewSourceUrl(event.target.value)} />
          <Button size="1" onClick={() => void addCatalogSource()}>添加来源</Button>
        </Flex>
        <div className="source-list">{sources.map((source) => <div className="source-row" key={source.id}><div className="grow min-width-zero"><strong>{source.name}</strong><Text as="div" size="1" color="gray" className="truncate">{source.url}</Text></div><Badge color={source.enabled ? "green" : "gray"} variant="soft">{source.enabled ? "启用" : "停用"}</Badge><Button size="1" variant="soft" disabled={busy === `sync:${source.id}`} onClick={() => void sync(source)}>同步</Button><Button size="1" variant="ghost" onClick={() => void updateCatalogSource(source, { enabled: !source.enabled })}>{source.enabled ? "停用" : "启用"}</Button>{source.id !== "anthropics-skills" && <Button size="1" variant="ghost" color="red" onClick={() => { if (api.removeCatalogSource) void api.removeCatalogSource(source.id).then(setSources).catch((reason) => setError(reason instanceof Error ? reason.message : String(reason))); }}>删除</Button>}</div>)}</div>
      </section>

      {categories.length > 0 && <div className="discover-categories">{categories.slice(0, 8).map((category) => <Badge key={category} variant="soft">{category}</Badge>)}</div>}
      <section className="discover-grid" aria-label="Skill 目录">
        {entries.length === 0 && busy !== "search" ? <div className="panel quiet-row">暂无缓存结果，请先同步目录。</div> : entries.map((entry) => (
          <article className={`panel discover-card ${selectedForCollection.includes(entry.id) ? "collection-selected" : ""}`} key={entry.id}>
            <div className="discover-card-head">
              <div className="skill-icon"><Code size={18} /></div>
              <div className="grow min-width-zero">
                <Flex align="center" gap="2" wrap="wrap"><strong>{entry.name}</strong><Badge color={entry.installedState === "notInstalled" ? "gray" : entry.installedState === "updateAvailable" ? "orange" : "green"} variant="soft">{stateLabels[entry.installedState]}</Badge></Flex>
                <Text as="div" size="1" color="gray" className="truncate">{entry.owner && entry.repository ? `${entry.owner}/${entry.repository}` : entry.sourceId}{entry.stars !== undefined ? ` · ${entry.stars} stars` : ""}</Text>
              </div>
              <Flex gap="1"><Checkbox aria-label={`加入集合 ${entry.name}`} checked={selectedForCollection.includes(entry.id)} onCheckedChange={(value) => setSelectedForCollection((current) => value ? [...new Set([...current, entry.id])] : current.filter((id) => id !== entry.id))} /><Button size="1" variant="ghost" aria-label={`${favorites.includes(entry.id) ? "取消收藏" : "收藏"} ${entry.name}`} onClick={() => void toggleFavorite(entry)}><Heart weight={favorites.includes(entry.id) ? "fill" : "regular"} color={favorites.includes(entry.id) ? "red" : undefined} /></Button></Flex>
            </div>
            <Text as="div" size="2" className="discover-description">{entry.description || "暂无描述"}</Text>
            <Flex gap="2" wrap="wrap" align="center"><Text size="1" color="gray">{entry.path ?? "目录根"}</Text>{entry.hasScripts && <Badge color="amber" variant="soft"><ShieldWarning /> 含脚本</Badge>}{entry.license && <Badge variant="soft">{entry.license}</Badge>}</Flex>
            <Flex justify="end" gap="2" mt="3"><Button size="1" variant="ghost" onClick={() => setSelected(entry)}>查看详情 <ArrowRight /></Button><Button size="1" disabled={!entry.skillUrl} onClick={() => onOpenInstall(entry)}>{entry.installedState === "notInstalled" ? "安装" : "查看安装"}</Button></Flex>
          </article>
        ))}
      </section>

      <section className="panel collection-panel" aria-label="Skill 集合">
        <div className="section-caption"><div><strong>我的集合</strong><Text as="div" size="1" color="gray">保存常用 Skill 组合，安装前仍会逐项预览和分配 IDE。</Text></div><Badge variant="soft">{collections.length}</Badge></div>
        <Flex gap="2" wrap="wrap" align="center">
          <input className="collection-name" value={collectionName} placeholder="集合名称" onChange={(event) => setCollectionName(event.target.value)} />
          <Text size="1" color="gray">已选 {selectedForCollection.length} 项</Text>
          <Button size="1" disabled={!collectionName.trim() || selectedForCollection.length === 0} onClick={() => void saveCurrentCollection()}>保存集合</Button>
        </Flex>
        {collections.length > 0 && <div className="collection-list">{collections.map((collection) => <div className="collection-row" key={collection.id}><div className="grow min-width-zero"><strong>{collection.name}</strong><Text as="div" size="1" color="gray">{(collection.sourceRefs?.length ?? collection.skillRefs.length)} 个 Skill · 默认 {collection.defaultClientIds.length} 个 IDE</Text></div><Button size="1" variant="soft" disabled={busy === `collection:${collection.id}`} onClick={() => void planCollection(collection)}>使用集合安装</Button><Button size="1" variant="ghost" color="red" onClick={() => void removeCollection(collection)}>删除</Button></div>)}</div>}
      </section>

      {collectionPlan && <section className="panel collection-plan-panel" aria-label="集合安装预览"><div className="section-caption"><div><strong>集合安装预览</strong><Text as="div" size="1" color="gray">所有成员均已重新检查来源，确认后才会进入现有安装流程。</Text></div><Badge variant="soft">{collectionPlan.entries.length} 个目标</Badge></div><div className="collection-plan-list">{collectionPlan.entries.map((entry) => <div className="collection-plan-row" key={entry.entryId}><Checkbox checked={collectionOverwriteIds.includes(entry.entryId)} onCheckedChange={(value) => setCollectionOverwriteIds((current) => value ? [...new Set([...current, entry.entryId])] : current.filter((id) => id !== entry.entryId))} /><div className="grow min-width-zero"><strong>{entry.skillName}</strong><Text as="div" size="1" color="gray" className="mono">{shortPath(entry.resolvedPath)} · {entry.consumers.join("、")}</Text></div><Badge color={entry.conflict === "notInstalled" || entry.conflict === "identical" ? "green" : "orange"} variant="soft">{entry.conflict}</Badge></div>)}</div><Flex justify="end" gap="2"><Button variant="soft" onClick={() => setCollectionPlan(undefined)}>取消</Button><Button disabled={busy === "collection-apply"} onClick={() => void applyCollectionPlan()}>确认并安装</Button></Flex></section>}

      {selected && <section ref={detailRef} tabIndex={-1} className="panel discover-detail" aria-label="Skill 详情">
        <div className="section-caption"><div><strong>{selected.name}</strong><Text as="div" size="1" color="gray">{selected.owner && selected.repository ? `${selected.owner}/${selected.repository}` : selected.sourceId}</Text></div><Button size="1" variant="ghost" onClick={() => setSelected(undefined)}>关闭</Button></div>
        <Text as="div" size="2">{selected.description || "暂无描述"}</Text>
        <div className="detail-grid"><div><Text size="1" color="gray">固定版本</Text><Text as="div" className="mono">{selected.commitSha ?? selected.reference ?? "未提供"}</Text></div><div><Text size="1" color="gray">目录路径</Text><Text as="div" className="mono">{shortPath(selected.path ?? "/")}</Text></div><div><Text size="1" color="gray">目标 IDE</Text><Text as="div">{clients.filter((client) => client.supportsSkills).map((client) => client.name).join("、") || "暂无可用 IDE"}</Text></div></div>
        {selected.hasScripts && <Callout.Root color="amber" size="1"><Callout.Icon><ShieldWarning /></Callout.Icon><Callout.Text>该 Skill 含脚本。安装器只复制文件，永不自动执行。</Callout.Text></Callout.Root>}
        {!selected.skillUrl && <Callout.Root color="red" size="1"><Callout.Icon><WarningCircle /></Callout.Icon><Callout.Text>该目录条目没有通过 URL 一致性校验，只可浏览，不能安装。</Callout.Text></Callout.Root>}
      </section>}
    </div>
  );
}
