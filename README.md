# Skill Installer

Skill Installer 是一个本地优先的 Agent Skill 批量安装和 IDE Skill 库存管理
工具。macOS 与 Windows 使用独立源码、测试、CI 和安装包，互不依赖。

## 平台状态

| 平台 | 目录 | 状态 | 安装包 |
|---|---|---|---|
| macOS | [`macos/`](macos/) | 0.2.1 | `.dmg` |
| Windows | [`windows/`](windows/) | 0.1.0，等待 Win11 实机验收 | NSIS `.exe` |

macOS 版支持 Codex、Claude Code、Kiro、Cursor、Windsurf、TRAE 国际版和
TRAE 国内版。它可从目录、ZIP 或公开 GitHub 仓库批量发现 Skills，通过
Skill-IDE 矩阵分配全局目标，并按 IDE 展示受管理、外部、异常和被动发现库存。
写入前会校验和预览，冲突覆盖与卸载前自动备份，且不会执行 Skill 脚本。

开发、路径和安全模型参见 [`macos/README.md`](macos/README.md) 与
[`windows/README.md`](windows/README.md)。跨机器继续开发时先阅读
[`PROJECT_CONTEXT.md`](PROJECT_CONTEXT.md)。
