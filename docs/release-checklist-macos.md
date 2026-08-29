# macOS 发布检查清单

适用于 macOS 预览版 DMG。当前不做 Apple Developer ID、公证、Stapling、Mac App Store 或自动更新。

## 版本与构建

- [ ] `macos/package.json`、`macos/src-tauri/tauri.conf.json`、`macos/src-tauri/Cargo.toml` 和 UI 标题版本一致。
- [ ] `pnpm install --frozen-lockfile`、TypeScript 检查和 React 测试通过。
- [ ] `cargo fmt --check`、`cargo test` 和 `cargo clippy -- -D warnings` 通过。
- [ ] `pnpm run build` 与 Tauri macOS 构建通过。

## DMG 完整性

- [ ] 使用 `hdiutil verify <file>.dmg` 验证镜像。
- [ ] 使用 `shasum -a 256 <file>.dmg` 记录校验和。
- [ ] 使用 `file` 或 `lipo -info` 确认目标架构（当前 CI 产物为 arm64 或 universal，按构建矩阵记录）。
- [ ] 使用 `codesign --verify --deep --strict --verbose=2` 检查 ad-hoc 签名；不要把 ad-hoc 签名描述为发行证书。
- [ ] 在干净用户目录安装并启动，验证首次扫描、导入、安装和卸载。

## 交付

- [ ] 更新 `CHANGELOG.md`、macOS smoke 模板和 GitHub Actions artifact 链接。
- [ ] 记录提交 SHA、DMG 文件名、SHA-256、测试环境和已知限制。
- [ ] 推送 `main` 后确认 CI 成功；失败需单独修复并重新验证。
