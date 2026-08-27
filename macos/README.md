# Skill Installer for macOS

Skill Installer 0.4.0 是独立的 macOS Agent Skill 批量安装与库存管理工具，使用
Tauri 2、Rust、React、TypeScript、Vite、Radix Themes 和 Phosphor Icons。
应用数据只保存在本机，不采集遥测，不上传 Skill 内容。

## 0.4.0 功能

- 更新不再停留在“发现变化”：可生成只读更新计划，查看新增、修改和删除摘要，
  逐项确认后批量更新。手工修改的目标会明确警告，写入前始终备份。
- 每个受管理 Skill 可固定/取消固定版本。固定项仍显示在库存中，但不会进入更新
  检查或更新计划。
- 已完成的安装、更新、纳管、卸载和恢复统一写入操作日志。操作中心区分待恢复、
  已完成和已回滚状态，并支持从备份回滚文件与管理记录。
- 新增 JSON 锁文件：记录来源、commit/子路径、文件树哈希、目标 IDE 和固定状态。
  导入时重新解析来源并校验锁定哈希；缺失 IDE、来源不可用或内容漂移只进入问题
  列表，不会静默安装其他版本。
- 便携 ZIP 与锁文件用途分离：ZIP 携带实际 Skill 内容，适合离线和无来源的纳管项；
  锁文件体积小、可审阅，适合从可访问来源重建相同配置。
- 操作中心可修改每个 Skill 的备份数量、总空间和保留天数，也可手工恢复或删除
  备份。仍被未完成操作引用的故障恢复备份禁止删除。
- 状态升级到 schema v4，原有安装、备份和 v3 操作日志原子迁移，不删除内容。

## 0.3.0 功能

- 新增本机概览，集中显示可用 IDE、受管理/外部 Skill 和需要处理的问题。
- IDE 检测结果会列出应用、CLI、配置目录和 Skills 目录等实际依据；Codex 明确
  区分默认写入的 `~/.agents/skills` 与兼容读取的 `~/.codex/skills`。
- 单个 IDE 可独立重新扫描；受管理 Skill 可进入现有分配矩阵同步到其他 IDE。
- 更新检查显示来源 commit、当前/来源哈希，并汇总新增、修改和删除的文件。
- 安装使用 schema v3 操作日志。应用异常退出后，下次启动会将未完成操作标为
  待恢复，用户可从概览回滚到操作前状态。
- 自动备份默认每个 Skill 保留 5 份、最多占用 1 GiB、保留 90 天；执行备份后
  自动清理超出策略的旧记录。
- 可将全部受管理 Skill 导出为带清单的 ZIP 便携包，在另一台机器通过 ZIP 来源
  直接检查、选择和安装。
- GitHub Actions 分别在 Apple Silicon 与 Intel runner 生成 DMG 和 SHA-256 文件。

## 0.2.1 修复

- 修复批量来源列表中搜索框被统计卡片样式影响而错位的问题。
- Codex 库存同时扫描当前的 `~/.agents/skills` 和早期版本使用的
  `~/.codex/skills`，旧目录中的外部 Skill 也可主动纳入管理。

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
| Codex | `~/.agents/skills`（库存兼容读取 `~/.codex/skills`） |
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

库存只扫描每个适配器的已知全局目录直接子项，不跟随软链接：

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

`state.json` 当前为 schema v4。v1/v2/v3 会原子迁移，不会删除 Skill 或备份。诊断
预览会将用户目录替换为 `~`，包含库存数量、检测依据、操作日志、备份策略、
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

Ad-hoc 签名的内测 DMG 输出到：

```text
macos/src-tauri/target/release/bundle/dmg/
```

CI 产物分别为 `skill-installer-macos-arm64-dmg` 与
`skill-installer-macos-x64-dmg`，每个产物同时包含 `SHA256SUMS.txt`。Ad-hoc 签名
不代表开发者身份或 Apple 公证；首次打开时仍可能需要在 Finder 中右键应用并
选择“打开”。0.4.0 不包含
Developer ID 签名、公证、自动更新、私有 GitHub、OAuth 或 Skill 市场。

## Windows 边界

未来 Windows 版本创建顶层 `windows/`，拥有独立源码、依赖、测试、CI 和安装包。
它可以参考 macOS 版本稳定的数据模型与流程，但不会导入或依赖 `macos/` 源码。
