# Skill Installer project context

This is the entry point for continuing the project on another machine.

## Current state

- Repository: `Leongwind/install-skills-tool`
- Default branch: `main`
- macOS version: `0.4.0`, local and dual-architecture CI verification complete
- Windows version: `0.1.0`, Windows CI passed, waiting for Win11 real-IDE validation
- Latest Windows code milestone: `89cb9ed`, GitHub source revision tracking
- Latest macOS milestones: `47f9672` safe update lifecycle, `8303874` reproducible
  lockfiles, `e9b4347` unified operation center, `0e2b68b` 0.4.0 delivery

## Platform boundary

- macOS code belongs only in `macos/`.
- Windows code belongs only in `windows/`.
- The two applications may copy proven behavior, but neither imports source from the other.
- New installations are global only. Historical macOS project records remain managed by macOS.

## Read in this order on Windows 11

1. `PROJECT_CONTEXT.md`
2. `docs/plans/windows-0.1.0-plan.md`
3. `docs/handoffs/windows-win11-testing.md`
4. `docs/handoffs/windows-test-results-template.md`

Then run `git status -sb` and `git log -5 --oneline` before changing anything.

## Delivery rules

- Test each completed Windows milestone.
- Commit only the intended files.
- Push every completed milestone to `origin/main`.
- Wait for the Windows workflow and fix failures in a separate commit.
- Never claim real IDE compatibility from fixtures or CI alone.
- Do not modify `macos/` while handling Windows-only test feedback.

## Test claim boundary

- Rust unit tests validate source inspection, assignment planning, paths, conflicts, and lifecycle rules.
- Windows CI validates real Windows APIs, filesystem behavior, the synthetic end-to-end environment, and NSIS packaging.
- Neither layer proves that a real IDE displays the installed Skill.
- Only the Win11 checklist with real Cursor and Kiro can mark real Skill-IDE assignment as verified.

## Current next steps

### macOS

Workflow run
[`33089545359`](https://github.com/Leongwind/install-skills-tool/actions/runs/33089545359)
passed for commit `0e2b68b5d842730ec7ad0d6b25c28e7143c1fb8d`. Both
`skill-installer-macos-arm64-dmg` and `skill-installer-macos-x64-dmg` were downloaded and
verified against their `SHA256SUMS.txt`; their hashes are recorded in
`docs/development-history.md`.

The next macOS gate is a real user-flow smoke test on this Mac: install the arm64 artifact,
scan the installed Codex/Kiro/TRAE clients and legacy inventory, exercise a multi-Skill
GitHub or ZIP install, then verify update diff/backup/rollback, pinning, lockfile round-trip,
and the operation center. Public distribution additionally requires Apple Developer ID
credentials and notarization. Current builds use Tauri ad-hoc signing
(`signingIdentity: "-"`) and must never be described as Developer ID signed or notarized.

### Windows

Download the NSIS artifact recorded in `docs/handoffs/windows-win11-testing.md`, then
complete real Windows 11 x64 validation with Cursor and Kiro. Record results in a dated
copy of `docs/handoffs/windows-test-results-template.md`.
