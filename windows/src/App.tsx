import {
  Archive as ArchiveBox,
  ArrowClockwise,
  CheckCircle,
  ClipboardText,
  Cube,
  Database,
  FolderOpen,
  GithubLogo,
  MagnifyingGlass,
  ShieldCheck,
  Trash,
  WarningCircle,
} from "@phosphor-icons/react";
import { Badge, Theme } from "@radix-ui/themes";
import { useEffect, useState } from "react";
import type { CSSProperties, Dispatch, SetStateAction } from "react";
import { backend } from "./api";
import type {
  Backup,
  EnvironmentScan,
  InstallPlan,
  Installation,
  ManagementStatus,
  OperationResult,
  SkillSource,
  SourceInspection,
} from "./types";
import { conflictText, formatBytes, managementText, statusText } from "./ui";

type Page = "install" | "inventory" | "diagnostics";
type SourceKind = "localDirectory" | "localArchive" | "github";
type InventoryFilter = "all" | "managed" | "external" | "issues";

const sourceLabels: Record<SourceKind, string> = {
  localDirectory: "本地目录",
  localArchive: "ZIP",
  github: "GitHub",
};

function sourceFrom(kind: SourceKind, value: string): SkillSource {
  if (kind === "github") return { kind, url: value };
  return { kind, path: value };
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export default function App() {
  const [page, setPage] = useState<Page>("install");
  const [environment, setEnvironment] = useState<EnvironmentScan>();
  const [installations, setInstallations] = useState<Installation[]>([]);
  const [backups, setBackups] = useState<Backup[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [sourceKind, setSourceKind] = useState<SourceKind>("github");
  const [sourceValue, setSourceValue] = useState("");
  const [inspection, setInspection] = useState<SourceInspection>();
  const [selectedSkills, setSelectedSkills] = useState<Set<string>>(new Set());
  const [assignments, setAssignments] = useState<Record<string, Set<string>>>({});
  const [skillSearch, setSkillSearch] = useState("");
  const [plan, setPlan] = useState<InstallPlan>();
  const [overwrite, setOverwrite] = useState<Set<string>>(new Set());
  const [results, setResults] = useState<OperationResult[]>([]);
  const [inventorySearch, setInventorySearch] = useState("");
  const [inventoryFilter, setInventoryFilter] = useState<InventoryFilter>("all");
  const [diagnostics, setDiagnostics] = useState("");

  const refresh = async () => {
    setLoading(true);
    setError("");
    try {
      const [scan, records, savedBackups] = await Promise.all([
        backend.scanEnvironment(),
        backend.listInstallations(),
        backend.listBackups(),
      ]);
      setEnvironment(scan);
      setInstallations(records);
      setBackups(savedBackups);
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const usableClients = environment?.clients.filter((client) => client.supportsSkills) ?? [];
  const visibleSkills = inspection?.skills.filter((skill) => {
    const query = skillSearch.trim().toLowerCase();
    return !query || `${skill.name} ${skill.description} ${skill.relativePath}`.toLowerCase().includes(query);
  }) ?? [];

  const inspect = async () => {
    if (!sourceValue.trim()) return;
    setBusy(true);
    setError("");
    setPlan(undefined);
    setResults([]);
    try {
      const next = await backend.inspectSource(sourceFrom(sourceKind, sourceValue.trim()));
      setInspection(next);
      setSelectedSkills(new Set(next.skills.map((skill) => skill.skillId)));
      setAssignments(Object.fromEntries(next.skills.map((skill) => [skill.skillId, new Set<string>()])));
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  const chooseSource = async () => {
    if (sourceKind === "github") return;
    const selected = sourceKind === "localArchive"
      ? await backend.chooseArchive()
      : await backend.chooseDirectory();
    if (typeof selected === "string") setSourceValue(selected);
  };

  const toggleSkill = (skillId: string) => {
    setSelectedSkills((current) => {
      const next = new Set(current);
      next.has(skillId) ? next.delete(skillId) : next.add(skillId);
      return next;
    });
  };

  const toggleAssignment = (skillId: string, clientId: string) => {
    setAssignments((current) => {
      const selected = new Set(current[skillId] ?? []);
      selected.has(clientId) ? selected.delete(clientId) : selected.add(clientId);
      return { ...current, [skillId]: selected };
    });
  };

  const toggleClientColumn = (clientId: string) => {
    const ids = [...selectedSkills];
    const everySelected = ids.every((id) => assignments[id]?.has(clientId));
    setAssignments((current) => Object.fromEntries(
      Object.entries(current).map(([skillId, clients]) => {
        const next = new Set(clients);
        if (ids.includes(skillId)) everySelected ? next.delete(clientId) : next.add(clientId);
        return [skillId, next];
      }),
    ));
  };

  const canPlan = selectedSkills.size > 0
    && [...selectedSkills].every((skillId) => (assignments[skillId]?.size ?? 0) > 0);

  const createPlan = async () => {
    if (!inspection || !canPlan) return;
    setBusy(true);
    setError("");
    try {
      const next = await backend.planInstall(
        inspection.inspectionId,
        [...selectedSkills].map((skillId) => ({
          skillId,
          clientIds: [...(assignments[skillId] ?? [])],
        })),
      );
      setPlan(next);
      setOverwrite(new Set());
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  const applyPlan = async () => {
    if (!plan) return;
    setBusy(true);
    setError("");
    try {
      setResults(await backend.applyInstallPlan(plan.planId, [...overwrite]));
      setPlan(undefined);
      await refresh();
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  const actAndRefresh = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError("");
    try {
      await action();
      await refresh();
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  const runAction = async (action: () => Promise<unknown>) => {
    setError("");
    try {
      await action();
    } catch (cause) {
      setError(errorText(cause));
    }
  };

  const inventoryCount = environment?.inventories.reduce(
    (total, item) => total + item.directSkills.length,
    0,
  ) ?? 0;

  return (
    <Theme accentColor="blue" grayColor="slate" radius="small" appearance="inherit">
      <div className="app-shell">
        <header className="titlebar">
          <Cube size={21} weight="fill" />
          <strong>Skill Installer</strong>
          <Badge variant="outline">Windows 0.1.0</Badge>
          <button className="link-button title-refresh" onClick={() => void refresh()} disabled={loading}>
            <ArrowClockwise size={17} />重新扫描
          </button>
        </header>
        <aside className="sidebar">
          <nav aria-label="主导航">
            <button className={page === "install" ? "active" : ""} onClick={() => setPage("install")}>
              <ArchiveBox size={20} />批量安装
            </button>
            <button className={page === "inventory" ? "active" : ""} onClick={() => setPage("inventory")}>
              <Database size={20} />IDE Skill 库存 <span>{inventoryCount}</span>
            </button>
            <button className={page === "diagnostics" ? "active" : ""} onClick={() => setPage("diagnostics")}>
              <ClipboardText size={20} />诊断与备份
            </button>
          </nav>
          <div className="privacy-note"><ShieldCheck size={18} />仅在本机处理文件<br />不执行 Skill 脚本</div>
        </aside>
        <main className="content">
          {error && <div className="alert error"><WarningCircle size={19} />{error}</div>}
          {loading ? <LoadingState /> : page === "install" ? (
            <InstallPage
              sourceKind={sourceKind}
              setSourceKind={(kind) => { setSourceKind(kind); setSourceValue(""); }}
              sourceValue={sourceValue}
              setSourceValue={setSourceValue}
              chooseSource={chooseSource}
              inspect={inspect}
              inspection={inspection}
              visibleSkills={visibleSkills}
              skillSearch={skillSearch}
              setSkillSearch={setSkillSearch}
              selectedSkills={selectedSkills}
              toggleSkill={toggleSkill}
              usableClients={usableClients}
              assignments={assignments}
              toggleAssignment={toggleAssignment}
              toggleClientColumn={toggleClientColumn}
              canPlan={canPlan}
              createPlan={createPlan}
              plan={plan}
              overwrite={overwrite}
              setOverwrite={setOverwrite}
              applyPlan={applyPlan}
              results={results}
              busy={busy}
            />
          ) : page === "inventory" ? (
            <InventoryPage
              environment={environment}
              query={inventorySearch}
              setQuery={setInventorySearch}
              filter={inventoryFilter}
              setFilter={setInventoryFilter}
              busy={busy}
              onReveal={(path) => runAction(() => backend.revealInExplorer(path))}
              onAdopt={(clientId, path) => actAndRefresh(() => backend.adoptExternalSkill(clientId, path))}
              onUninstall={(id) => actAndRefresh(async () => {
                const first = await backend.uninstallInstallation(id);
                if (first.status === "confirmationRequired" && window.confirm(first.message)) {
                  await backend.uninstallInstallation(id, true);
                }
              })}
            />
          ) : (
            <DiagnosticsPage
              installations={installations}
              backups={backups}
              diagnostics={diagnostics}
              busy={busy}
              onExport={() => actAndRefresh(async () => setDiagnostics(await backend.exportDiagnostics()))}
              onRestore={(id) => actAndRefresh(() => backend.restoreBackup(id))}
            />
          )}
        </main>
      </div>
    </Theme>
  );
}

function LoadingState() {
  return <div className="skeleton-stack" aria-label="正在扫描 IDE">
    <div className="skeleton heading" /><div className="skeleton card" /><div className="skeleton card" />
  </div>;
}

type InstallPageProps = {
  sourceKind: SourceKind; setSourceKind: (kind: SourceKind) => void;
  sourceValue: string; setSourceValue: (value: string) => void;
  chooseSource: () => Promise<void>; inspect: () => Promise<void>;
  inspection?: SourceInspection; visibleSkills: SourceInspection["skills"];
  skillSearch: string; setSkillSearch: (value: string) => void;
  selectedSkills: Set<string>; toggleSkill: (id: string) => void;
  usableClients: EnvironmentScan["clients"];
  assignments: Record<string, Set<string>>;
  toggleAssignment: (skillId: string, clientId: string) => void;
  toggleClientColumn: (clientId: string) => void;
  canPlan: boolean; createPlan: () => Promise<void>;
  plan?: InstallPlan; overwrite: Set<string>;
  setOverwrite: Dispatch<SetStateAction<Set<string>>>;
  applyPlan: () => Promise<void>; results: OperationResult[]; busy: boolean;
};

function InstallPage(props: InstallPageProps) {
  return <section>
    <PageHeader title="批量安装 Skills" subtitle="检查一次来源，再为每个 Skill 分配全局 IDE。" />
    <div className="panel source-panel">
      <StepTitle number="1" title="检查 Skill 来源" subtitle="本地目录、ZIP 或公开 GitHub 仓库" />
      <div className="segmented source-tabs">
        {(Object.keys(sourceLabels) as SourceKind[]).map((kind) => <button
          key={kind} className={props.sourceKind === kind ? "selected" : ""}
          onClick={() => props.setSourceKind(kind)}>
          {kind === "localDirectory" ? <FolderOpen /> : kind === "localArchive" ? <ArchiveBox /> : <GithubLogo />}
          {sourceLabels[kind]}
        </button>)}
      </div>
      <div className="source-input-row">
        <input aria-label="Skill 来源" value={props.sourceValue} onChange={(event) => props.setSourceValue(event.target.value)}
          placeholder={props.sourceKind === "github" ? "https://github.com/owner/repository" : "选择本机路径"} />
        {props.sourceKind !== "github" && <button className="secondary" onClick={() => void props.chooseSource()}>浏览</button>}
        <button className="primary" disabled={!props.sourceValue.trim() || props.busy} onClick={() => void props.inspect()}>检查来源</button>
      </div>
      {props.inspection && <>
        <div className="inspection-summary">
          <Stat value={props.inspection.skills.length} label="有效" />
          <Stat value={props.inspection.rejected.length} label="无效" />
          <Stat value={props.inspection.skills.filter((skill) => skill.hasScripts).length} label="含脚本" />
        </div>
        <label className="search-field skill-search"><MagnifyingGlass /><input aria-label="搜索 Skill" placeholder="搜索名称、描述或路径"
          value={props.skillSearch} onChange={(event) => props.setSkillSearch(event.target.value)} /></label>
        <div className="selection-meta">已选 {props.selectedSkills.size} / {props.inspection.skills.length}</div>
        <div className="skill-list">
          {props.visibleSkills.map((skill) => <label className="skill-row" key={skill.skillId}>
            <input type="checkbox" checked={props.selectedSkills.has(skill.skillId)} onChange={() => props.toggleSkill(skill.skillId)} />
            <span className="skill-icon">&lt;/&gt;</span>
            <span className="skill-copy"><strong>{skill.name}</strong>{skill.hasScripts && <Badge color="amber">含脚本</Badge>}
              <small>{skill.relativePath} · {skill.fileCount} 个文件 · {formatBytes(skill.totalBytes)} · {skill.contentHash.slice(0, 10)}</small>
              <span>{skill.description}</span></span>
          </label>)}
          {props.visibleSkills.length === 0 && <EmptyState text="没有匹配的 Skill" />}
          {props.inspection.rejected.map((skill) => <div className="skill-row rejected" key={skill.relativePath}>
            <span className="disabled-check" /><span className="skill-copy"><strong>{skill.relativePath}</strong><span>{skill.reason}</span></span>
          </div>)}
        </div>
      </>}
    </div>
    {props.inspection && <div className="panel assignment-panel">
      <StepTitle number="2" title="分配全局 IDE" subtitle="IDE 默认不选，每个 Skill 至少选择一个目标" />
      {props.usableClients.length === 0 ? <EmptyState text="未检测到可安装 Skills 的 IDE" /> : <div className="matrix" role="table" aria-label="Skill IDE 分配" style={{ "--columns": props.usableClients.length } as CSSProperties}>
        <div className="matrix-head"><span>Skill</span>{props.usableClients.map((client) => <button key={client.id} onClick={() => props.toggleClientColumn(client.id)}>{client.name}<small>整列选择</small></button>)}</div>
        {props.inspection.skills.filter((skill) => props.selectedSkills.has(skill.skillId)).map((skill) => <div className="matrix-row" key={skill.skillId}>
          <strong>{skill.name}</strong>{props.usableClients.map((client) => <label key={client.id} title={client.globalSkillsPath}>
            <input type="checkbox" checked={props.assignments[skill.skillId]?.has(client.id) ?? false}
              onChange={() => props.toggleAssignment(skill.skillId, client.id)} /><span>{client.name}</span></label>)}</div>)}
      </div>}
      <div className="panel-actions"><span>{props.canPlan ? "分配完整，可以生成预览" : "请完成每个 Skill 的 IDE 分配"}</span>
        <button className="primary" disabled={!props.canPlan || props.busy} onClick={() => void props.createPlan()}>生成安装预览</button></div>
    </div>}
    {props.plan && <div className="panel preview-panel">
      <StepTitle number="3" title="确认并执行" subtitle={`${props.plan.entries.length} 个物理安装目标`} />
      {props.plan.entries.map((entry) => { const needs = ["conflict", "updateAvailable"].includes(entry.conflict); return <div className="plan-row" key={entry.entryId}>
        <div><strong>{entry.skillName}</strong><Badge color={needs ? "amber" : "green"}>{conflictText[entry.conflict]}</Badge>
          <small>{entry.resolvedPath}</small><span>目标：{entry.consumers.join("、")}</span>
          {entry.passiveConsumers.length > 0 && <span className="warning">可能被动发现：{entry.passiveConsumers.join("、")}</span>}</div>
        {needs && <label><input type="checkbox" checked={props.overwrite.has(entry.entryId)} onChange={() => props.setOverwrite((current) => {
          const next = new Set(current); next.has(entry.entryId) ? next.delete(entry.entryId) : next.add(entry.entryId); return next;
        })} />确认覆盖</label>}
      </div>; })}
      <div className="panel-actions"><span>每个覆盖目标都会单独备份</span><button className="primary" disabled={props.busy || props.plan.entries.some((entry) => ["conflict", "updateAvailable"].includes(entry.conflict) && !props.overwrite.has(entry.entryId))} onClick={() => void props.applyPlan()}>执行安装</button></div>
    </div>}
    {props.results.length > 0 && <div className="panel results"><h2>操作结果</h2>{props.results.map((result, index) => <div key={`${result.path}-${index}`} className={result.success ? "result success" : "result failure"}>
      {result.success ? <CheckCircle /> : <WarningCircle />}<span><strong>{result.skillName}</strong>{result.message}<small>{result.path}</small></span></div>)}</div>}
  </section>;
}

function InventoryPage({ environment, query, setQuery, filter, setFilter, busy, onReveal, onAdopt, onUninstall }: {
  environment?: EnvironmentScan; query: string; setQuery: (value: string) => void;
  filter: InventoryFilter; setFilter: (value: InventoryFilter) => void; busy: boolean;
  onReveal: (path: string) => Promise<void>; onAdopt: (clientId: string, path: string) => Promise<void>; onUninstall: (id: string) => Promise<void>;
}) {
  const visible = (status: ManagementStatus) => filter === "all"
    || (filter === "managed" && ["toolManaged", "adopted", "modified"].includes(status))
    || (filter === "external" && status === "external")
    || (filter === "issues" && ["modified", "unsafe"].includes(status));
  return <section><PageHeader title="IDE Skill 库存" subtitle="按 IDE 查看直接安装、外部内容和被动发现项。" />
    <div className="panel inventory-tools"><label className="search-field"><MagnifyingGlass /><input aria-label="搜索库存" placeholder="搜索名称或路径" value={query} onChange={(event) => setQuery(event.target.value)} /></label>
      <div className="segmented">{(["all", "managed", "external", "issues"] as InventoryFilter[]).map((value) => <button key={value} className={filter === value ? "selected" : ""} onClick={() => setFilter(value)}>{{ all: "全部", managed: "受管理", external: "外部", issues: "异常" }[value]}</button>)}</div></div>
    {environment?.clients.filter((client) => client.status !== "notInstalled").map((client) => {
      const inventory = environment.inventories.find((item) => item.clientId === client.id);
      const skills = [...(inventory?.directSkills ?? []), ...(inventory?.passiveSkills ?? [])].filter((skill) => {
        const match = `${skill.name} ${skill.resolvedPath}`.toLowerCase().includes(query.toLowerCase());
        return match && visible(skill.managementStatus);
      });
      return <div className="panel client-card" key={client.id}><header><span className="client-icon">{client.name[0]}</span><div><h2>{client.name} <Badge color={client.supportsSkills ? "green" : "gray"}>{statusText[client.status]}</Badge></h2><small>{client.globalSkillsPath}{client.version ? ` · ${client.version}` : ""}</small></div><span className="client-count">{skills.length} Skills</span></header>
        {inventory?.scanError && <div className="inline-error">{inventory.scanError}</div>}
        {skills.length === 0 ? <EmptyState text={client.status === "notInstalled" ? "IDE 未安装" : "该筛选下没有 Skill"} /> : skills.map((skill) => <div className="inventory-row" key={skill.inventoryId}>
          <span className="skill-icon">&lt;/&gt;</span><div><strong>{skill.name}</strong><Badge color={skill.managementStatus === "external" ? "gray" : skill.managementStatus === "unsafe" || skill.managementStatus === "modified" ? "red" : "green"}>{managementText[skill.managementStatus]}</Badge>
            <small>{skill.resolvedPath}</small>{skill.passiveFromClientId && <span>来自 {skill.passiveFromClientId} 共享目录</span>}{skill.issues.map((issue) => <span className="warning" key={issue}>{issue}</span>)}</div>
          <div className="row-actions">{skill.managementStatus !== "passive" && <button aria-label={`在资源管理器中显示 ${skill.name}`} title="在资源管理器中显示" onClick={() => void onReveal(skill.resolvedPath)}><FolderOpen /></button>}{skill.managementStatus === "external" && <button disabled={busy} onClick={() => void onAdopt(client.id, skill.resolvedPath)}>纳入管理</button>}{skill.installationId && <button className="danger" disabled={busy} onClick={() => void onUninstall(skill.installationId!)}><Trash />卸载</button>}</div>
        </div>)}</div>;
    })}</section>;
}

function DiagnosticsPage({ installations, backups, diagnostics, busy, onExport, onRestore }: {
  installations: Installation[]; backups: Backup[]; diagnostics: string; busy: boolean;
  onExport: () => Promise<void>; onRestore: (id: string) => Promise<void>;
}) {
  return <section><PageHeader title="诊断与备份" subtitle="诊断不包含 Skill 文件内容，用户目录会被隐藏。" />
    <div className="diagnostic-grid"><div className="panel"><h2>管理状态</h2><Stat value={installations.length} label="受管理安装" /><button className="primary wide" disabled={busy} onClick={() => void onExport()}>生成诊断预览</button></div>
      <div className="panel backup-list"><h2>备份</h2>{backups.length === 0 ? <EmptyState text="暂无备份" /> : backups.map((backup) => <div key={backup.id}><span><strong>{backup.originalPath}</strong><small>{new Date(backup.createdAt).toLocaleString()}</small></span><button disabled={busy} onClick={() => void onRestore(backup.id)}>恢复</button></div>)}</div></div>
    {diagnostics && <div className="panel diagnostic-output"><h2>诊断预览</h2><pre>{diagnostics}</pre></div>}
  </section>;
}

function PageHeader({ title, subtitle }: { title: string; subtitle: string }) { return <header className="page-header"><h1>{title}</h1><p>{subtitle}</p></header>; }
function StepTitle({ number, title, subtitle }: { number: string; title: string; subtitle: string }) { return <div className="step-title"><b>{number}</b><div><h2>{title}</h2><p>{subtitle}</p></div></div>; }
function Stat({ value, label }: { value: number; label: string }) { return <div className="stat"><strong>{value}</strong><span>{label}</span></div>; }
function EmptyState({ text }: { text: string }) { return <div className="empty-state">{text}</div>; }
