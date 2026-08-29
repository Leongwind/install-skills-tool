# macOS 0.5.1 GitHub 来源计划

## 目标

让公开 GitHub Skill 来源在复制链接或从社区文档粘贴时更容易使用，同时保持来源
可复现、路径安全和无认证依赖。

## 交付范围

- 接受 `owner/repository` 简写，以及 `owner/repository@ref:path` 简写。
- 接受仓库根、`tree`、`blob`、`commit/<sha>` URL，并支持 `?ref=...&path=...`
  的精确选择。
- 直接指向 `SKILL.md` 时自动选择其父目录；未给出路径时递归发现仓库中的多个 Skill。
- ref、owner、repository 和子路径拒绝控制字符、绝对路径和 `..` 逃逸。
- 通过 GitHub API 尽力记录 commit SHA；API 不可用时保留明确的“SHA 暂不可用”状态，
  不改变下载内容或静默切换来源。
- 仅支持公开 GitHub，不加入私有仓库、OAuth 或 Skill 市场。

## 示例

```text
mattpocock/skills
mattpocock/skills@main:skills/engineering/tdd
https://github.com/mattpocock/skills/tree/main/skills/engineering/tdd
https://github.com/mattpocock/skills/blob/main/skills/engineering/tdd/SKILL.md
https://github.com/mattpocock/skills/commit/<sha>
```

## 验收

- Rust 单测覆盖简写、分支 ref、commit SHA、精确路径和不安全输入。
- 既有 GitHub 多 Skill、软链接和来源快照测试保持通过。
- 文档和版本号更新到 `0.5.1`；macOS CI 继续只构建和校验 ad-hoc DMG，不包含
  Developer ID、公证、Stapling、Homebrew 或自动更新。
