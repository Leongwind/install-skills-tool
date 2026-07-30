import {
  ArrowClockwise,
  ArrowRight,
  Check,
  CloudArrowDown,
  Code,
  Copy,
  Database,
  Desktop,
  FolderOpen,
  Gear,
  GithubLogo,
  HardDrives,
  Info,
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
  RadioGroup,
  Separator,
  Spinner,
  Text,
  TextField,
  Theme,
} from "@radix-ui/themes";
import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./api";
import type {
  BackupRecord,
  DetectedClient,
  InstallPlan,
  InstallScope,
  OperationResult,
  PhysicalInstallation,
  SkillMetadata,
  SkillSource,
  UpdateStatus,
} from "./types";
import { conflictLabel, detectionLabel, formatBytes, shortPath } from "./ui";

type Page = "install" | "manage" | "diagnostics";
type SourceMode = "local" | "github";

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

export default function App() {
  const [page, setPage] = useState<Page>("install");
  const [clients, setClients] = useState<DetectedClient[]>([]);
  const [installations, setInstallations] = useState<PhysicalInstallation[]>([]);
  const [backups, setBackups] = useState<BackupRecord[]>([]);
  const [sourceMode, setSourceMode] = useState<SourceMode>("local");
  const [sourceValue, setSourceValue] = useState("");
  const [skill, setSkill] = useState<SkillMetadata>();
  const [scope, setScope] = useState<InstallScope>("global");
  const [projectPath, setProjectPath] = useState("");
  const [selected, setSelected] = useState<string[]>([]);
  const [plan, setPlan] = useState<InstallPlan>();
  const [overwrites, setOverwrites] = useState<string[]>([]);
  const [results, setResults] = useState<OperationResult[]>([]);
  const [updates, setUpdates] = useState<UpdateStatus[]>([]);
  const [diagnostics, setDiagnostics] = useState("");
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    setBusy("scan");
    setError("");
    try {
      const [nextClients, nextInstallations, nextBackups] = await Promise.all([
        api.scanClients(),
        api.listInstallations(),
        api.listBackups(),
      ]);
      setClients(nextClients);
      setInstallations(nextInstallations);
      setBackups(nextBackups);
      setSelected((current) =>
        current.filter((id) =>
          nextClients.some((client) => client.id === id && client.supportsSkills),
        ),
      );
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const source: SkillSource | undefined = sourceValue
    ? sourceMode === "local"
      ? { kind: "local", path: sourceValue }
      : { kind: "github", url: sourceValue }
    : undefined;

  const activeClients = useMemo(
    () => clients.filter((client) => client.status !== "notInstalled"),
    [clients],
  );

  async function chooseFolder(kind: "skill" | "project") {
    const selectedPath = await open({ directory: true, multiple: false });
    if (typeof selectedPath !== "string") return;
    if (kind === "skill") {
      setSourceMode("local");
      setSourceValue(selectedPath);
      setSkill(undefined);
      setPlan(undefined);
    } else {
      setProjectPath(selectedPath);
      setPlan(undefined);
    }
  }

  async function inspect() {
    if (!source) return;
    setBusy("inspect");
    setError("");
    setPlan(undefined);
    try {
      setSkill(await api.inspectSkill(source));
    } catch (reason) {
      setSkill(undefined);
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function createPlan() {
    if (!source) return;
    setBusy("plan");
    setError("");
    setResults([]);
    try {
      const next = await api.planInstall(
        source,
        selected,
        scope,
        scope === "project" ? projectPath : undefined,
      );
      setSkill(next.skill);
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
      const next = await api.applyInstallPlan(plan.planId, overwrites);
      setResults(next);
      setPlan(undefined);
      await refresh();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function checkUpdates() {
    setBusy("updates");
    try {
      setUpdates(await api.checkUpdates());
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy("");
    }
  }

  async function uninstall(item: PhysicalInstallation, force = false) {
    setBusy(item.id);
    try {
      const result = await api.uninstall(item.id, force);
      if (!result.success && result.status === "confirmationRequired") {
        if (window.confirm(`${result.message}\n\n${shortPath(result.path)}`)) {
          await uninstall(item, true);
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

  return (
    <Theme accentColor="blue" grayColor="slate" radius="medium" scaling="95%">
      <div className="app-shell">
        <header className="titlebar" data-tauri-drag-region>
          <div className="traffic-space" />
          <Package size={17} weight="fill" />
          <strong>Skill Installer</strong>
          <Badge variant="outline">macOS Preview</Badge>
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
                className={page === "install" ? "nav-item active" : "nav-item"}
                onClick={() => setPage("install")}
              >
                <CloudArrowDown />安装 Skill
              </button>
              <button
                className={page === "manage" ? "nav-item active" : "nav-item"}
                onClick={() => setPage("manage")}
              >
                <Database />已安装与备份
                <span className="nav-count">{installations.length}</span>
              </button>
              <button
                className={page === "diagnostics" ? "nav-item active" : "nav-item"}
                onClick={() => setPage("diagnostics")}
              >
                <Gear />诊断
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
            {page === "install" && (
              <div className="page">
                <div className="page-heading">
                  <div>
                    <Text as="div" size="5" weight="bold">
                      安装一个 Skill
                    </Text>
                    <Text as="div" size="2" color="gray">
                      校验一次，安全地复制到多个 Agent。
                    </Text>
                  </div>
                  <div className="step-indicator">
                    <span className={skill ? "done" : "current"}>1 来源</span>
                    <ArrowRight />
                    <span className={plan ? "done" : skill ? "current" : ""}>2 目标</span>
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
                      <strong>选择 Skill 来源</strong>
                      <Text as="div" size="1" color="gray">
                        本地目录或公开 GitHub 目录
                      </Text>
                    </div>
                  </div>
                  <div className="segmented">
                    <button
                      className={sourceMode === "local" ? "selected" : ""}
                      onClick={() => {
                        setSourceMode("local");
                        setSourceValue("");
                        setSkill(undefined);
                      }}
                    >
                      <FolderOpen />本地目录
                    </button>
                    <button
                      className={sourceMode === "github" ? "selected" : ""}
                      onClick={() => {
                        setSourceMode("github");
                        setSourceValue("");
                        setSkill(undefined);
                      }}
                    >
                      <GithubLogo />GitHub
                    </button>
                  </div>
                  <Flex gap="2" className="source-input-row">
                    <TextField.Root
                      className="grow"
                      value={sourceValue}
                      placeholder={
                        sourceMode === "local"
                          ? "选择包含 SKILL.md 的目录"
                          : "https://github.com/owner/repo/tree/main/skill"
                      }
                      onChange={(event) => {
                        setSourceValue(event.target.value);
                        setSkill(undefined);
                        setPlan(undefined);
                      }}
                    />
                    {sourceMode === "local" && (
                      <Button variant="soft" onClick={() => void chooseFolder("skill")}>
                        浏览…
                      </Button>
                    )}
                    <Button disabled={!source || Boolean(busy)} onClick={() => void inspect()}>
                      {busy === "inspect" && <Spinner size="1" />}
                      校验
                    </Button>
                  </Flex>
                  {skill && (
                    <div className="skill-card">
                      <div className="skill-icon">
                        <Code size={20} />
                      </div>
                      <div className="grow min-width-zero">
                        <Flex align="center" gap="2">
                          <strong>{skill.name}</strong>
                          <Badge color="green">
                            <Check />规范有效
                          </Badge>
                        </Flex>
                        <Text as="div" size="2" color="gray" className="truncate">
                          {skill.description}
                        </Text>
                        <Text as="div" size="1" color="gray">
                          {skill.fileCount} 个文件 · {formatBytes(skill.totalBytes)} · SHA-256{" "}
                          {skill.contentHash.slice(0, 10)}
                        </Text>
                      </div>
                    </div>
                  )}
                  {skill?.warnings.map((warning) => (
                    <Callout.Root color="amber" size="1" key={warning}>
                      <Callout.Icon>
                        <ShieldWarning />
                      </Callout.Icon>
                      <Callout.Text>{warning}</Callout.Text>
                    </Callout.Root>
                  ))}
                </section>

                <section className={`panel ${skill ? "" : "disabled-panel"}`}>
                  <div className="panel-title">
                    <div className="number">2</div>
                    <div>
                      <strong>范围与目标</strong>
                      <Text as="div" size="1" color="gray">
                        目标路径由原生适配器生成
                      </Text>
                    </div>
                  </div>
                  <RadioGroup.Root
                    value={scope}
                    onValueChange={(value) => {
                      setScope(value as InstallScope);
                      setPlan(undefined);
                    }}
                    className="scope-row"
                    disabled={!skill}
                  >
                    <RadioGroup.Item value="global">
                      <Text size="2" weight="medium">
                        全局
                      </Text>
                      <Text size="1" color="gray">
                        当前用户的所有项目
                      </Text>
                    </RadioGroup.Item>
                    <RadioGroup.Item value="project">
                      <Text size="2" weight="medium">
                        项目
                      </Text>
                      <Text size="1" color="gray">
                        仅指定项目目录
                      </Text>
                    </RadioGroup.Item>
                  </RadioGroup.Root>
                  {scope === "project" && (
                    <Flex gap="2" className="project-input-row">
                      <TextField.Root
                        className="grow"
                        value={projectPath}
                        placeholder="选择项目根目录"
                        onChange={(event) => setProjectPath(event.target.value)}
                      />
                      <Button variant="soft" onClick={() => void chooseFolder("project")}>
                        浏览…
                      </Button>
                    </Flex>
                  )}
                  <div className="client-list">
                    {activeClients.length === 0 && busy !== "scan" && (
                      <div className="empty-inline">
                        <Desktop size={24} />
                        暂未检测到支持的 Agent
                      </div>
                    )}
                    {activeClients.map((client) => {
                      const checked = selected.includes(client.id);
                      return (
                        <label
                          className={`client-row ${client.supportsSkills ? "" : "unavailable"}`}
                          key={client.id}
                        >
                          <Checkbox
                            checked={checked}
                            disabled={!client.supportsSkills || !skill}
                            onCheckedChange={(value) => {
                              setSelected((current) =>
                                value
                                  ? [...current, client.id]
                                  : current.filter((id) => id !== client.id),
                              );
                              setPlan(undefined);
                            }}
                          />
                          <div className="client-mark">{client.name.slice(0, 1)}</div>
                          <div className="grow min-width-zero">
                            <Flex align="center" gap="2">
                              <Text size="2" weight="medium">
                                {client.name}
                              </Text>
                              <StatusBadge client={client} />
                            </Flex>
                            <Text as="div" size="1" color="gray" className="truncate mono">
                              {scope === "global"
                                ? shortPath(client.globalSkillsPath)
                                : client.projectSkillsPath}
                            </Text>
                          </div>
                          <Text size="1" color="gray">
                            {client.version ?? ""}
                          </Text>
                        </label>
                      );
                    })}
                  </div>
                  <Flex justify="end">
                    <Button
                      disabled={
                        !skill ||
                        selected.length === 0 ||
                        Boolean(busy) ||
                        (scope === "project" && !projectPath)
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
                          {plan.entries.length} 个唯一目录；共享路径已合并
                        </Text>
                      </div>
                    </div>
                    {plan.entries.map((entry) => {
                      const needsOverwrite =
                        entry.conflict === "conflict" ||
                        entry.conflict === "updateAvailable";
                      return (
                        <div className="plan-entry" key={entry.resolvedPath}>
                          <HardDrives size={19} />
                          <div className="grow min-width-zero">
                            <code>{shortPath(entry.resolvedPath)}</code>
                            <div className="consumer-line">
                              {entry.consumers.map((id) => (
                                <Badge key={id} variant="soft">
                                  {clients.find((client) => client.id === id)?.name ?? id}
                                </Badge>
                              ))}
                              {entry.passiveConsumers.length > 0 && (
                                <Text size="1" color="amber">
                                  可能被 {entry.passiveConsumers.join("、")} 被动发现
                                </Text>
                              )}
                            </div>
                          </div>
                          <Badge
                            color={
                              entry.conflict === "conflict"
                                ? "red"
                                : entry.conflict === "notWritable"
                                  ? "red"
                                  : entry.conflict === "identical"
                                    ? "gray"
                                    : entry.conflict === "updateAvailable"
                                      ? "orange"
                                      : "green"
                            }
                          >
                            {conflictLabel[entry.conflict]}
                          </Badge>
                          {needsOverwrite && (
                            <label className="overwrite">
                              <Checkbox
                                checked={overwrites.includes(entry.resolvedPath)}
                                onCheckedChange={(value) =>
                                  setOverwrites((current) =>
                                    value
                                      ? [...current, entry.resolvedPath]
                                      : current.filter((path) => path !== entry.resolvedPath),
                                  )
                                }
                              />
                              覆盖并备份
                            </label>
                          )}
                        </div>
                      );
                    })}
                    <Flex justify="between" align="center">
                      <Text size="1" color="gray">
                        写入前不会执行任何 Skill 脚本。
                      </Text>
                      <Button
                        disabled={
                          Boolean(busy) ||
                          plan.entries.some(
                            (entry) =>
                              (entry.conflict === "conflict" ||
                                entry.conflict === "updateAvailable") &&
                              !overwrites.includes(entry.resolvedPath),
                          )
                        }
                        onClick={() => void applyPlan()}
                      >
                        {busy === "apply" && <Spinner size="1" />}
                        执行安装
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
                    {results.map((result) => (
                      <div className="result-row" key={result.path}>
                        <span className={result.success ? "result-dot ok" : "result-dot failed"} />
                        <code>{shortPath(result.path)}</code>
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
              <div className="page">
                <div className="page-heading">
                  <div>
                    <Text as="div" size="5" weight="bold">
                      已安装与备份
                    </Text>
                    <Text as="div" size="2" color="gray">
                      仅管理由本工具追踪的物理安装。
                    </Text>
                  </div>
                  <Button variant="soft" onClick={() => void checkUpdates()}>
                    {busy === "updates" ? <Spinner size="1" /> : <ArrowClockwise />}
                    检查更新
                  </Button>
                </div>
                <section className="panel">
                  <div className="section-caption">
                    <strong>物理安装</strong>
                    <Badge variant="soft">{installations.length}</Badge>
                  </div>
                  {installations.length === 0 ? (
                    <div className="empty-state">
                      <Package size={34} />
                      <strong>还没有受管理的 Skill</strong>
                      <Text size="2" color="gray">
                        完成一次安装后会在这里显示。
                      </Text>
                    </div>
                  ) : (
                    installations.map((item) => {
                      const update = updates.find(
                        (status) => status.installationId === item.id,
                      );
                      return (
                        <div className="installation-row" key={item.id}>
                          <div className="skill-icon">
                            <Code size={18} />
                          </div>
                          <div className="grow min-width-zero">
                            <Flex align="center" gap="2">
                              <strong>{item.skillName}</strong>
                              {item.consumers.map((id) => (
                                <Badge key={id} variant="soft">
                                  {clients.find((client) => client.id === id)?.name ?? id}
                                </Badge>
                              ))}
                            </Flex>
                            <Text as="div" size="1" color="gray" className="mono truncate">
                              {shortPath(item.resolvedPath)}
                            </Text>
                            {update && (
                              <Text
                                as="div"
                                size="1"
                                color={update.status === "current" ? "green" : "orange"}
                              >
                                {update.message}
                              </Text>
                            )}
                          </div>
                          <Button
                            size="1"
                            variant="ghost"
                            color="red"
                            disabled={busy === item.id}
                            onClick={() => void uninstall(item)}
                          >
                            {busy === item.id ? <Spinner size="1" /> : <Trash />}
                            卸载
                          </Button>
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
                      覆盖或卸载前生成的备份会出现在这里。
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
                            variant="soft"
                            disabled={busy === backup.id}
                            onClick={() => void restore(backup)}
                          >
                            恢复
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
                      导出前先查看内容；用户目录会替换为 ~。
                    </Text>
                  </div>
                </div>
                <Callout.Root color="blue" size="1">
                  <Callout.Icon>
                    <Info />
                  </Callout.Icon>
                  <Callout.Text>
                    包含客户端检测结果、应用版本和安装记录；不包含 Skill 内容，不上传任何数据。
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
                      <Copy />复制 JSON
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
