# macOS initial plan snapshot

The first macOS release established an independent Tauri 2 application under
`macos/`, producing a DMG and supporting native Agent Skill paths for Codex, Claude
Code, Kiro, Cursor, Windsurf, TRAE International, and TRAE China.

Core decisions:

- Rust owns detection, network, validation, planning, installation, backup, and state.
- React only calls typed Tauri IPC commands.
- Skills are copied, never linked, and scripts are never executed during installation.
- Every write is preceded by validation, conflict classification, and a read-only plan.
- Overwrites and uninstall create backups.
- State and diagnostics remain local with home paths redacted.
- macOS and future Windows code remain independent.

The implementation landed in commits `70c384f` through `ddd2c91`.

