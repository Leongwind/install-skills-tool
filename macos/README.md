# Skill Installer for macOS

Skill Installer 0.2.0 是独立的 macOS Agent Skill 批量安装与库存管理工具，使用
Tauri 2、Rust、React、TypeScript、Vite、Radix Themes 和 Phosphor Icons。
应用数据只保存在本机，不采集遥测，不上传 Skill 内容。

## 0.2.0 功能

- 从本地目录、本地 ZIP 或公开 GitHub URL 递归发现多个 Skill。
- 有效 Skill 默认全选；用户用 Skill-IDE 矩阵为每个 Skill 单独分配目标。
- 新安装只支持全局范围，不再创建项目级安装。
- 按 IDE 扫描全局 Skill 库存，区分本工具安装、已纳管、外部安装、手工修改、
  异常和被动发现。
- 外部 Skill 需用户主动纳入管理，之后才能由本工具备份和卸载。
- v1 历史项目安装保留在独立区域，可检查、备份和卸载，但不会自动迁移。

## 支持矩阵

| Agent | 全局 Skill 目录 |
|---|---|
| Codex | `~/.agents/skills` |
| Claude Code | `~/.claude/skills` |
| Kiro | `~/.kiro/skills` |
| Cursor | `~/.cursor/skills` |
| Windsurf | `~/.codeium/windsurf/skills` |
| TRAE 国际版 | `~/.trae/skills` |
| TRAE 国内版 | `~/.trae-cn/skills` |

TRAE 国际版通过 `com.trae.app`、应用内 `product.json` 和 `.trae` 数据目录联合
识别，原生 Skills 最低版本为 3.5.25；3.5.44 起还可能被动读取
`.agents/skills`。国内版结合 bundle、`product.json` 和 `.trae-cn` 识别，
最低版本为 3.3.25；3.3.44 起也可能读取 `.agents/skills`。

版本依据：

- [TRAE 国际版更新日志](https://www.trae.ai/ja/changelog)
- [TRAE 国内版更新日志](https://www.trae.cn/changelog)

## 来源与批量规则

- 本地目录可直接指向一个 Skill，也可指向包含多个 Skill 的父目录。
- ZIP 支持单根目录和多根目录结构，自动忽略 `__MACOSX` 与 `.DS_Store`。
- GitHub 支持仓库根 URL、子目录 URL 和直接 `SKILL.md` URL。仓库根 URL 通过
  `HEAD` 解析默认分支，并尽力记录 commit SHA。
- 发现一个 Skill 根目录后不再向下搜索，避免将示例文件识别为嵌套 Skill。
- 相同名称和相同哈希的重复项合并；相同名称但内容不同的项目不能同时安装。
- 检查结果通过 `inspectionId` 暂存在 Rust 内存中，生成计划时不重复下载来源。

## 库存与纳管

库存只扫描每个适配器的全局目录直接子项，不跟随软链接：

- `SKILL.md` 可读但目录名或 frontmatter 不规范时仍显示警告。
- 软链接、无法安全哈希或越出已知全局目录的内容仅供查看，不能纳管。
- Codex `.agents/skills` 可能被其他 Agent 被动发现；这些引用不会重复纳管。
- 外部 Skill 纳管时会重新校验物理路径并记录基线哈希。
- 无来源的纳管项显示“来源未绑定”，不提供在线更新。
- 卸载共享物理目录前会列出主动消费者，且总是先创建备份。

## 安全模型

- 必须存在符合 [Agent Skills 规范](https://agentskills.io/specification)的
  `SKILL.md`，且 `name` 与目录名一致。
- 安装期间绝不执行 Skill 脚本；存在脚本或可执行文件时显示警告。
- ZIP 或 GitHub 下载内容不超过 50 MB；展开后不超过 200 MB 和 5,000 个文件。
- 拒绝绝对路径、路径穿越和控制字符文件名。本地 ZIP 中的软链接会被拒绝；
  GitHub 归档中的软链接不会展开或复制。
- 下载与解压先进入缓存，检查缓存超过 24 小时自动清理。
- 先计算稳定 SHA-256 文件树哈希并生成只读计划，再允许写入。
- 冲突按计划项确认；每个覆盖目标独立备份，并使用同级临时目录原子替换。
- 批量操作允许部分成功，不会删除同目录中的其他 Skill。

状态位于：

```text
~/Library/Application Support/Skill Installer/
├── state.json
├── cache/
├── backups/
└── logs/
```

`state.json` 当前为 schema v2。诊断预览会将用户目录替换为 `~`，包含库存数量、
规范和管理状态，但不包含 Skill 文件内容。

## 开发

需要 Node.js 22+、pnpm 11、Rust stable 和完整 Xcode。

```bash
cd macos
pnpm install
pnpm run build
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer pnpm tauri build
```

未签名内测 DMG 输出到：

```text
macos/src-tauri/target/release/bundle/dmg/
```

首次打开未签名版本时，可在 Finder 中右键应用并选择“打开”。0.2.0 不包含
Developer ID 签名、公证、自动更新、私有 GitHub、OAuth 或 Skill 市场。

## Windows 边界

未来 Windows 版本创建顶层 `windows/`，拥有独立源码、依赖、测试、CI 和安装包。
它可以参考 macOS 版本稳定的数据模型与流程，但不会导入或依赖 `macos/` 源码。
