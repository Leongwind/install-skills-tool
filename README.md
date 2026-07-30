# Skill Installer

Skill Installer 是一个本地优先的 Agent Skill 批量安装工具。当前阶段只开发
macOS 独立版本；Windows 将在未来使用顶层 `windows/` 目录和独立安装包，不依赖
`macos/` 源码。

## 平台状态

| 平台 | 目录 | 状态 | 安装包 |
|---|---|---|---|
| macOS | [`macos/`](macos/) | MVP 开发中 | `.dmg` |
| Windows | `windows/` | 尚未开始 | 未来提供独立安装包 |

macOS 版支持 Codex、Claude Code、Kiro、Cursor、Windsurf、TRAE 国际版和
TRAE 国内版。它会校验 `SKILL.md`，预览所有物理写入位置，在冲突覆盖前备份，
并且不会执行 Skill 中的脚本。

开发、路径和安全模型参见 [`macos/README.md`](macos/README.md)。
