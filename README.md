# Skill Installer

Skill Installer 是一个本地优先的 Agent Skill 批量安装和 IDE Skill 库存管理
工具。macOS 与 Windows 使用独立源码、测试、CI 和安装包，互不依赖。

## 平台状态

| 平台 | 目录 | 状态 | 安装包 |
|---|---|---|---|
| macOS | [`macos/`](macos/) | 0.6.0 | `.dmg`（Apple Silicon / Intel） |
| Windows | [`windows/`](windows/) | 0.1.0 | `.exe` |

macOS 版支持 Codex、Claude Code、Kiro、Cursor、Windsurf、TRAE 国际版和
TRAE 国内版。它可从目录、ZIP 或公开 GitHub 仓库批量发现 Skills，通过
Skill-IDE 矩阵分配全局目标，并按 IDE 展示受管理、外部、异常和被动发现库存。
0.6.0 新增 skills.sh 与公开目录浏览、离线目录缓存、搜索筛选、收藏和可复用 Skill 集合。
写入前会校验和预览，冲突覆盖与卸载前自动备份，且不会执行 Skill 脚本。
0.6.0 在 0.5.1 的基础上增加可信目录发现与集合管理；0.5.1 增加 GitHub 简写、显式 ref/commit 和精确 Skill 路径解析；
同时保留 Adapter V2 原生目录描述、递归库存扫描、Codex 共享目录的被动发现提示、
Agent Skills 可选规范字段展示和库存页本地刷新。
跨机器迁移可按场景选择携带实际内容的 ZIP，或可审阅且会校验来源哈希的锁文件。

## Windows 与 macOS  版能力

Windows 版与 macOS 版使用独立源码和构建链路，当前支持：

- 按本机实际检测到的 IDE 或 CLI 展示 Skill 库存，不展示未安装客户端的虚拟库存。
- 管理已有 Skill：纳入管理、卸载、重新扫描，并支持在资源管理器中定位。
- 支持嵌套 Skill、路径归一化和跨扫描关联；诊断页可查看备份并执行恢复。

## 本地开发与验证示例

```powershell
cd windows
pnpm install
pnpm test
pnpm build
pnpm tauri build
```

开发、路径和安全模型参见 [`macos/README.md`](macos/README.md) 与
[`windows/README.md`](windows/README.md)。

社区贡献、漏洞报告和版本变化参见 [`CONTRIBUTING.md`](CONTRIBUTING.md)、
[`SECURITY.md`](SECURITY.md) 与 [`CHANGELOG.md`](CHANGELOG.md)。跨机器交接和
macOS 真实 IDE 验收记录位于 [`docs/`](docs/)。
