# Windows 0.1.0 implementation plan

## Product

- Independent Tauri 2 application in `windows/`.
- Windows 10/11 x64 and NSIS installer.
- Feature parity with macOS 0.2.1 for directory, ZIP, public GitHub, multi-Skill
  assignment, global installation, inventory, adoption, backup, restore, and uninstall.
- No project installation, Windows ARM release, MSIX, signing, private GitHub, or
  automatic update.

## Adapters

Detect Codex, Claude Code, Kiro, Cursor, Windsurf, TRAE International, and TRAE China
through PATH, known user directories, product metadata, executable versions, and
Windows uninstall registry entries.

## Verification boundary

- Unit fixtures verify assignment rules and path generation.
- Windows CI creates synthetic client installations and performs actual copies inside
  temporary Windows directories.
- Neither fixture layer proves that a real IDE displays a Skill.
- Real visibility and multi-IDE behavior are accepted only after Windows 11 x64 manual
  testing with Cursor and Kiro.

## Delivery

Each completed milestone is tested, committed, pushed to `origin/main`, and recorded
in `docs/development-history.md`. The final CI run must upload the
`skill-installer-windows-x64` artifact.

