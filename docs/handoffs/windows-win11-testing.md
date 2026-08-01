# Windows 11 x64 testing handoff

This file is completed when Windows 0.1.0 CI produces the NSIS installer.

## Build under test

- Commit SHA: `PENDING`
- GitHub Actions run: `PENDING`
- Artifact: `skill-installer-windows-x64`
- Installer filename: `PENDING`
- Installer SHA-256: `PENDING`

## Required environment

- Windows 11 x64 with current updates.
- Cursor x64 and Kiro x64.
- Access to a local multi-Skill directory or ZIP.
- Network access for one public GitHub source test.

## Acceptance flow

1. Record Windows, Cursor, and Kiro versions.
2. Install the NSIS package and launch Skill Installer.
3. Confirm Cursor and Kiro are both detected with their real versions and paths.
4. Import at least two valid Skills.
5. Assign Skill A to Cursor and Kiro, then assign Skill B to only one IDE.
6. Apply the plan and verify the files under both real IDE Skill roots.
7. Confirm both IDEs display the expected Skills in their native interfaces.
8. Restart Skill Installer and verify per-IDE inventory.
9. Add one external Skill manually, rescan, and adopt it.
10. Exercise conflict overwrite, backup, restore, and uninstall.
11. Confirm uninstalling one Skill leaves neighboring Skills untouched.
12. Export diagnostics and record every failure in the result template.

## Claim boundary

CI proves Windows build and synthetic integration behavior. Only this manual flow can
mark real Cursor/Kiro visibility and real multi-IDE assignment as verified.

## Continue with Codex on Windows

Open the repository in Codex and provide this instruction:

> Read `PROJECT_CONTEXT.md`, `docs/plans/windows-0.1.0-plan.md`,
> `docs/handoffs/windows-win11-testing.md`, and
> `docs/handoffs/windows-test-results-template.md`. Inspect git status and the latest
> commits. Execute the Windows 11 test checklist, record results in a dated copy of the
> template, fix only Windows-scoped failures, run relevant tests, commit intentionally,
> and push each completed fix to `origin/main`. Do not modify `macos/`.

