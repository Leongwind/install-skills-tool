# macOS 真实 IDE Smoke 测试模板

此模板用于发布前在真实 macOS 机器上记录“检测到 IDE”和“IDE 原生页面可见”两件不同的结论。固定夹具只能证明安装器的分配计划与文件写入逻辑。

## 当前待测构建

- 提交：`5afe99c20ed63efe82c89da2932ccc997e222ec9`
- CI：[macOS run 33254849319](https://github.com/Leongwind/install-skills-tool/actions/runs/33254849319)
- Apple Silicon：`skill-installer-macos-arm64-dmg` / `Skill Installer_0.5.1_aarch64.dmg`
  / `ca3f83d5da71eaf7fb6d0afb21c999fa4beef5cc20c49cd22f77910091f8c6ad`
- Intel：`skill-installer-macos-x64-dmg` / `Skill Installer_0.5.1_x64.dmg`
  / `e519c95f27113d51ee5fdbf11e66b4e11b8b4463920be742c8e02dfeb75e1431`
- 自动验证：构建、checksum、DMG、bundle metadata、架构和 ad-hoc 签名均通过。
- 尚未验证：真实 IDE 原生 Skills 页面可见性和下面的人工操作流程。

## 测试信息

- 提交 SHA：
- Skill Installer 版本：
- macOS 版本/架构：
- 测试日期：
- DMG 文件名和 SHA-256：

## IDE 矩阵

| IDE | 版本 | 架构 | 安装路径 | 检测结果 | 原生 Skills 页面可见 |
| --- | --- | --- | --- | --- | --- |
| Codex | | | | ☐ | ☐ |
| Claude Code | | | | ☐ | ☐ |
| Kiro | | | | ☐ | ☐ |
| Cursor | | | | ☐ | ☐ |
| Windsurf | | | | ☐ | ☐ |
| TRAE International | | | | ☐ | ☐ |
| TRAE China | | | | ☐ | ☐ |

## 流程

1. 启动应用并执行“重新扫描”，记录检测状态、版本和库存根目录。
2. 从本地目录或 ZIP 导入至少两个测试 Skill；确认脚本警告和文件哈希显示正确。
3. 将 Skill A 分配给两个 IDE，将 Skill B 只分配给一个 IDE，确认预览中的物理路径和消费者去重。
4. 执行安装，在每个 IDE 的原生 Skills 页面确认可见、可启用和可禁用。
5. 在库存页检查受管理、外部、异常、共享和被动发现标识。
6. 手工修改一个 Skill，验证更新/卸载要求确认并生成备份。
7. 恢复备份，再卸载一个 Skill，确认同目录的其他 Skill 不受影响。

## 证据位置

记录截图、脱敏诊断和日志路径即可，不要提交 Skill 内容、完整用户路径或凭据。
