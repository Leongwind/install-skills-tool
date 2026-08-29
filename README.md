# Skill Installer

Skill Installer 是一个本地优先的 Agent Skill 批量安装和 IDE Skill 库存管理
工具。macOS 与 Windows 使用独立源码、测试、CI 和安装包，互不依赖。

## 平台状态

| 平台 | 目录 | 状态 | 安装包 |
|---|---|---|---|
| macOS | [`macos/`](macos/) | 0.4.1 | `.dmg`（Apple Silicon / Intel） |
| Windows | [`windows/`](windows/) | 0.1.0 | `.exe` |

macOS 版支持 Codex、Claude Code、Kiro、Cursor、Windsurf、TRAE 国际版和
TRAE 国内版。它可从目录、ZIP 或公开 GitHub 仓库批量发现 Skills，通过
Skill-IDE 矩阵分配全局目标，并按 IDE 展示受管理、外部、异常和被动发现库存。
写入前会校验和预览，冲突覆盖与卸载前自动备份，且不会执行 Skill 脚本。
0.4.1 增加可确认/可回滚的批量更新、版本固定、可复现 JSON 锁文件，以及统一的
操作与备份中心。跨机器迁移可按场景选择携带实际内容的 ZIP，或可审阅且会校验
来源哈希的锁文件。

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
