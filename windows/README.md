# Skill Installer for Windows

Windows 0.1.0 is an independent Tauri 2 desktop application for installing and managing
Agent Skills across Windows coding IDEs. It does not import source from `macos/`.

## Status

- Code target: Windows 10/11 x64
- Package: current-user NSIS `.exe`
- Current validation: Rust/React tests and successful GitHub `windows-latest` NSIS build
- Final validation: real Windows 11 x64 with Cursor and Kiro

CI fixtures prove assignment planning and the complete synthetic filesystem data flow.
They do not prove that a real IDE displays a Skill. The real claim is intentionally
reserved for the Win11 checklist in
[`docs/handoffs/windows-win11-testing.md`](../docs/handoffs/windows-win11-testing.md).

## Supported IDEs and global roots

| IDE | Global Skill root |
|---|---|
| Codex | `%USERPROFILE%\.agents\skills` |
| Codex legacy inventory | `%USERPROFILE%\.codex\skills` |
| Claude Code | `%USERPROFILE%\.claude\skills` |
| Kiro | `%USERPROFILE%\.kiro\skills` |
| Cursor | `%USERPROFILE%\.cursor\skills` |
| Windsurf | `%USERPROFILE%\.codeium\windsurf\skills` |
| TRAE International | `%USERPROFILE%\.trae\skills` |
| TRAE China | `%USERPROFILE%\.trae-cn\skills` |

New project-scoped installation is not supported.

## Features

- Discover multiple Skills from a local directory, local ZIP, GitHub repository,
  GitHub subdirectory, or direct `SKILL.md` URL.
- Search and select valid Skills, then assign each Skill to one or more IDEs.
- Scan each IDE inventory and distinguish tool-managed, adopted, external, modified,
  unsafe, and passively discovered Skills.
- Preview conflicts and target paths before writing.
- Copy directories without executing Skill scripts.
- Back up each overwritten or removed physical target.
- Adopt external Skills, detect manual changes, restore backups, and uninstall safely.
- Export redacted diagnostics without Skill file contents or telemetry.

## Security boundaries

- ZIP input is limited to 50 MB compressed, 200 MB expanded, and 5,000 files.
- Absolute paths, traversal, control-character names, symlinks, junctions, and reparse
  point escapes are rejected.
- A discovered Skill must contain readable `SKILL.md` YAML frontmatter with matching
  `name` and directory name.
- Installation uses a sibling temporary directory and replacement flow.
- Skill scripts and executables are reported but never run.

## Development

Requirements: Node.js 22, pnpm 11.17.0, Rust stable, and Windows x64 build tools.

```powershell
cd windows
pnpm install --frozen-lockfile
pnpm build
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm tauri build --target x86_64-pc-windows-msvc
```

The installer is produced under
`src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis\`.

## State location

```text
%APPDATA%\Skill Installer\
├── state.json
├── cache\
├── backups\
└── logs\
```

Continue cross-machine work from [`PROJECT_CONTEXT.md`](../PROJECT_CONTEXT.md).
