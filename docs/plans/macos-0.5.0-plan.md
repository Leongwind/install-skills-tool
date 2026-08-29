# macOS 0.5.0 Adapter V2 与库存计划

## 目标

在不改变全局安装边界和安全模型的前提下，让 macOS 版更准确地描述 Agent 的
原生 Skills 目录、递归发现规则和 Agent Skills 规范字段，并让库存页能够看到
外部变更。

## 交付范围

- Adapter V2：保留兼容写入目录，同时暴露原生全局目录和递归库存标记。
- 库存递归扫描已知全局根目录；忽略 `.git`、`node_modules`、`target`、
  `__MACOSX`，发现 Skill 根目录后停止向下搜索，不跟随软链接。
- Codex 的 `.agents/skills` 与 `.codex/skills` 均可读取；共享目录对其他 Agent
  只显示为被动发现，避免把同一物理目录误报为多个直接安装。
- 解析并展示 Agent Skills 规范中的 `license`、`compatibility`、`metadata` 和
  `allowed-tools` 可选 frontmatter 字段。
- 库存页在停留期间每 5 秒进行一次本地扫描；网络更新检查仍由用户显式触发。
- 版本号升级到 `0.5.0`，CI 继续构建 arm64/x86_64 ad-hoc DMG 并校验包元数据。

## 非目标

本计划不加入项目级安装、私有 GitHub/OAuth、Skill 市场、自动更新、Homebrew、
Developer ID 签名、公证或 Stapling。Windows 代码和 Windows CI 保持独立不变。

## 验收

- Rust：递归嵌套 Skill、重复根去重、Codex 共享目录被动发现、可选 frontmatter
  字段和既有安全测试全部通过。
- React/TypeScript：来源列表、库存筛选和元数据渲染回归通过。
- macOS CI：版本一致性、Tauri 构建、DMG 完整性、架构和 ad-hoc 签名检查通过。
