# Changelog

## 0.6.0

- 新增发现 Skills 页面、Anthropic Skills GitHub 公开目录来源、搜索筛选和 Skill 详情预览；不直连需要 Vercel OIDC 的 skills.sh API。
- 新增目录 ETag/Last-Modified 离线缓存；同步失败时继续使用最近有效快照。
- 新增收藏和可复用 Skill 集合，集合安装继续复用既有安全检查与 Skill-IDE 分配流程。
- 集合安装预览改为可编辑的 Skill × IDE 矩阵；跨来源成员按稳定目录条目 ID 映射到实际检查结果，所有成员均需显式目标。
- GitHub URL 解析统一复用安装器 parser，规范化 tree/blob/commit 与 `ref`/`path` 查询参数；Contents provider 单次读取、流式限制 20 MB 并支持取消。
- 目录条目按来源仓库和路径与库存关联，区分未安装、部分安装、已安装和有更新，避免同名跨仓库误合并。
- 状态 schema 升级到 v6；保持本地优先、无遥测、不执行 Skill 脚本和 ad-hoc DMG 内测边界。

## 0.5.1

- 支持公开 GitHub 简写、分支/ref、commit 和精确 `SKILL.md` 路径。
- 增加来源快照、stale 计划保护和真实回滚可用性判断。
- 完善 macOS 社区文档与诊断边界。

## 0.5.0

- 引入 Adapter V2、递归库存扫描和原生/兼容目录标识。
- 支持共享物理目录、被动发现和库存实时刷新。
- 版本和 frontmatter 兼容性检查更加明确。

## 0.4.1

- 状态 schema 升级到 v5，增加 revision、resultingHash 和安全迁移备份。
- 增加统一状态写入锁、耐久保存、安装/更新计划过期和来源/目标校验。
- 改进操作日志、备份恢复及 stale 计划测试。

当前版本仍是 macOS 预览版，不包含 Apple Developer ID、公证、Stapling、Mac App Store、自动更新或 Windows 实现。
