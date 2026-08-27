# Development history

## macOS foundation, 2026-07-30

| Commit | Change |
|---|---|
| `70c384f` | Created the macOS project directory. |
| `d7626c8` | Bootstrapped the Tauri application. |
| `0a2a475` | Added client detection and Skill validation. |
| `d9d21f9` | Added native Agent adapters. |
| `2024033` | Added multi-target installation and backups. |
| `bcb0605` | Added the macOS management interface. |
| `d096526` | Added conflict and security coverage. |
| `ddd2c91` | Documented macOS usage and the Windows boundary. |

## macOS hardening and 0.2.x, 2026-07-30 to 2026-07-31

| Commit | Change |
|---|---|
| `d5cbfc8` | Scoped GitHub archive validation to the selected Skill. |
| `c127465` | Fixed compact layout and Codex detection. |
| `7d7d2d9` | Added ZIP, multi-Skill sources, and IDE inventory. |
| `86aa8a0` | Added the Skill-IDE matrix and inventory UI. |
| `c27577e` | Released macOS 0.2.0 documentation. |
| `b9303be` | Added legacy Codex inventory scanning and repaired source search layout. |

macOS 0.2.1 passed React, TypeScript, Rust, Clippy, Tauri build, DMG verification,
and GitHub Actions. The verified local DMG SHA-256 was
`cb898336dc3ca08c93dacac779350126f9dc2051d231a252c0066575a0279abc`.

## macOS lifecycle and reproducibility, 2026-08-27

| Commit | Change |
|---|---|
| `67fe54c` | Added transparent client detection evidence and crash recovery state. |
| `8ef2608` | Added resilient inventory management, update diffs, backup policy, and portable ZIP export. |
| `aa66cb4` | Documented macOS 0.3.0 and enabled Apple Silicon plus Intel DMG artifacts. |
| `47f9672` | Added planned, confirmed, backed-up and reversible Skill updates with version pinning. |
| `8303874` | Added reproducible JSON lockfile export/import with source-hash and IDE reconciliation. |
| `e9b4347` | Added the unified operation journal, rollback controls and managed backup policy UI. |

macOS 0.4.0 uses state schema v4. Existing v1-v3 state migrates atomically. Portable ZIP
bundles carry Skill content; JSON lockfiles carry source provenance, expected hashes, IDE
assignments and pin state. Lockfile reconciliation does not automatically delete extra local
installations. CI artifacts use ad-hoc signing and are not Developer ID signed or notarized.
The locally verified Apple Silicon 0.4.0 DMG has SHA-256
`234a73c39a905678b4d2848d0b867ed5f73e966c7ac8f575f8b1a86d0033309c`.
`hdiutil verify`, bundle version/identifier inspection, arm64 Mach-O inspection and strict
ad-hoc `codesign` verification all passed.

## Windows 0.1.0, started 2026-08-02

| Commit | Change |
|---|---|
| `f5996ba` | Created the independent `windows/` directory. |
| `9e89a1c` | Added project history, plan snapshots, and Win11 handoff documents. |
| `69ced78` | Bootstrapped the independent Windows Tauri 2 application. |
| `951b0a1` | Added Windows client detection, registry inputs, and seven native adapters. |
| `c768c72` | Added directory, ZIP, GitHub source inspection and per-IDE inventory. |
| `bc08505` | Added assignment planning, global installation, adoption, backups, restore, and IPC. |
| `146354f` | Added the responsive Windows batch installation and inventory interface. |
| `50501c0` | Added the Windows-only synthetic environment integration test and NSIS CI workflow. |
| `a3c6b74` | Fixed a Windows-only Clippy warning in reparse-point detection. |
| `89cb9ed` | Recorded GitHub source commit SHAs with a clear API fallback warning. |

The synthetic environment test is deliberately not described as a real IDE compatibility
test. Real Cursor and Kiro visibility remains a Windows 11 manual acceptance item.

Windows workflow run
[`30713093329`](https://github.com/Leongwind/install-skills-tool/actions/runs/30713093329)
passed frontend build, React tests, Rust format, Windows Clippy, Windows unit and synthetic
integration tests, NSIS packaging, and artifact upload for commit `89cb9ed`. The verified
installer SHA-256 is
`a78cc1b366921d265bd406d3cd3d07024e5d0f91585c5bb0af800a6c868b3640`.
