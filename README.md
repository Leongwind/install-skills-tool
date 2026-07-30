# Skill Installer

Skill Installer 是一个本地优先的 Agent Skill 批量安装和 IDE Skill 库存管理
工具。当前阶段只开发 macOS 独立版本；Windows 将在未来使用顶层 `windows/`
目录和独立安装包，不依赖 `macos/` 源码。

## 平台状态

| 平台 | 目录 | 状态 | 安装包 |
|---|---|---|---|
| macOS | [`macos/`](macos/) | 0.2.1 | `.dmg` |
| Windows | `windows/` | 尚未开始 | 未来提供独立安装包 |

macOS 版支持 Codex、Claude Code、Kiro、Cursor、Windsurf、TRAE 国际版和
TRAE 国内版。它可从目录、ZIP 或公开 GitHub 仓库批量发现 Skills，通过
Skill-IDE 矩阵分配全局目标，并按 IDE 展示受管理、外部、异常和被动发现库存。
写入前会校验和预览，冲突覆盖与卸载前自动备份，且不会执行 Skill 脚本。

开发、路径和安全模型参见 [`macos/README.md`](macos/README.md)。
