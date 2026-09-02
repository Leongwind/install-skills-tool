# macOS 0.6.0 计划与交付记录

## 范围

0.6.0 将安装器从单纯的来源导入升级为“发现、可信来源与集合管理”：

- 内置 skills.sh 和用户添加的公开 HTTPS 目录；目录只提供元数据，stars 仅作热度参考。
- 目录条目支持搜索、来源/分类/脚本筛选、详情预览、commit/ref 信息和安装状态。
- 使用 ETag、Last-Modified 与本地快照缓存；网络失败不阻断已有库存管理。
- 收藏条目并保存 SkillCollection 选择预设；实际写入仍复用 inspect_source → plan_install → apply_install_plan。
- 以 owner/repository/path/name 关联库存，禁止同名不同仓库误合并。
- schemaVersion 升级到 6，state.json 只保存来源配置、收藏、集合和缓存索引，目录内容保存在 cache/。

## 提交里程碑

| Commit | 里程碑 |
|---|---|
| `37cffa9` | catalog 来源模型、schema v6 迁移、离线缓存和 IPC |
| `050f5e0` | 发现 Skills 页面、搜索筛选、详情和安装跳转 |
| `f899f95` | 收藏与可复用集合 UI |
| `b8e3278` | 目录与受管理库存的来源关联和安装状态 |
| `fd8d76e` | 缓存过期、集合分配、跨仓库同名隔离测试 |
| 当前提交 | 0.6.0 文档、版本号和交接记录 |

## 安全与边界

目录同步仅访问公开 HTTPS 端点；来源内容安装前仍由现有 Agent Skills 规范校验、文件数/体积/path traversal/软链接检查和 SHA-256 计划保护。任何 Skill 脚本都不会执行。项目级安装、私有 GitHub/OAuth、云同步、社区评分、自动安装、Windows 代码、Apple Developer ID/公证/Stapling/Homebrew/自动更新均不在本版本范围内。

## 验证

```bash
cd macos
pnpm build
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

CI 继续构建 arm64/x64 ad-hoc DMG。真实目录服务内容、真实 IDE 可见性和网络受限环境需要在用户机器上进行验收。
