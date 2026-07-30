# Skill Installer for macOS

这是 Skill Installer 的独立 macOS 实现，使用 Tauri 2、Rust、React、
TypeScript、Vite、Radix Themes 和 Phosphor Icons。应用数据只保存在本机，
不采集遥测，不上传 Skill 内容。

## 支持矩阵

| Agent | 全局目录 | 项目目录 |
|---|---|---|
| Codex | `~/.agents/skills` | `<project>/.agents/skills` |
| Claude Code | `~/.claude/skills` | `<project>/.claude/skills` |
| Kiro | `~/.kiro/skills` | `<project>/.kiro/skills` |
| Cursor | `~/.cursor/skills` | `<project>/.cursor/skills` |
| Windsurf | `~/.codeium/windsurf/skills` | `<project>/.windsurf/skills` |
| TRAE 国际版 | `~/.trae/skills` | `<project>/.trae/skills` |
| TRAE 国内版 | `~/.trae-cn/skills` | `<project>/.trae/skills` |

TRAE 国际版通过 `com.trae.app`、应用内 `product.json` 和 `.trae` 数据目录联合
识别，原生 Skills 最低版本为 3.5.25；3.5.44 起可能被动读取项目
`.agents/skills`。国内版结合 bundle、`product.json` 和 `.trae-cn` 识别，
最低版本为 3.3.25；3.3.44 起可能读取 `.agents/skills`。两版项目目录相同，
因此同时选择时安装计划会合并为一次物理写入。

版本依据：

- [TRAE 国际版更新日志](https://www.trae.ai/ja/changelog)
- [TRAE 国内版更新日志](https://www.trae.cn/changelog)

## 安全模型

- 来源支持本地 Skill 根目录、公开 GitHub Skill 目录和公开 `SKILL.md` URL。
- 必须存在符合 [Agent Skills 规范](https://agentskills.io/specification)的
  `SKILL.md`，且 `name` 与目录名一致。
- 安装期间绝不执行 Skill 脚本；存在 `scripts/` 时会显示警告。
- GitHub 下载不超过 50 MB，展开后不超过 200 MB 和 5,000 个文件。
- 拒绝路径穿越、绝对 ZIP 路径和软链接。
- 先计算稳定 SHA-256 文件树哈希并生成只读计划，再允许安装。
- 冲突覆盖和卸载前创建备份；写入使用同级临时目录替换。
- 项目安装不会修改 `.gitignore`。

状态位于：

```text
~/Library/Application Support/Skill Installer/
├── state.json
├── cache/
└── backups/
```

诊断预览会将用户目录替换为 `~`，不包含 Skill 文件内容。

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

首次打开未签名版本时，可在 Finder 中右键应用并选择“打开”。当前 MVP 不包含
Developer ID 签名、公证、自动更新、私有 GitHub 或 Skill 市场。

## Windows 边界

未来 Windows 版本创建顶层 `windows/`，拥有自己的源码、依赖、测试、CI 和安装
包。可参考此版本已经验证的数据模型和流程，但不会导入或依赖 `macos/` 中的代码。
