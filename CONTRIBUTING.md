# Contributing to Skill Installer

感谢你为 Skill Installer 提交改进。当前维护范围是 macOS 版本，源码和测试位于 `macos/`；Windows 版本有独立的目录和工作流，请不要在本项目的 macOS 变更中修改 `windows/`。

## 开发环境

- macOS 13 或更高版本
- Node.js 22、pnpm 11
- Rust stable、Xcode Command Line Tools
- Tauri 2 CLI（可通过项目依赖调用）

```sh
cd macos
pnpm install
pnpm run dev
```

## 提交前检查

在 `macos/` 下运行：

```sh
pnpm run typecheck
pnpm test
pnpm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

涉及安装、备份、路径校验或来源下载的变更必须添加回归测试。测试不得执行 Skill 中的脚本，也不得把真实用户目录或 Skill 内容提交到仓库。

## Pull request 约定

- 使用清晰、单一目的的提交，例如 `fix: guard stale install plans`。
- 描述行为变化、测试命令和已知限制。
- UI 变更请附上窄窗口（约 900×620）截图或说明。
- 不要加入 Developer ID、公证、Stapling、自动更新、私有 GitHub/OAuth 或 Skill 市场功能。
- 许可证由项目维护者另行选择；在此之前不要新增 `LICENSE` 文件。

安全问题请先阅读 [SECURITY.md](SECURITY.md)，不要在公开 issue 中粘贴凭据、私人路径或 Skill 文件内容。
