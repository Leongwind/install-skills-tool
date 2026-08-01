# macOS 0.2.0 plan snapshot

macOS 0.2.0 changed the application from a single-Skill installer into a multi-source
batch installer and IDE Skill inventory manager.

Delivered behavior:

- Local directory, local ZIP, and public GitHub sources.
- Recursive discovery of multiple Skills with invalid entries retained for review.
- Per-Skill, per-IDE assignment matrix with no IDE selected by default.
- Global-only new installation.
- IDE inventory with managed, adopted, external, modified, unsafe, and passive states.
- External Skill adoption, backup, restore, and safe uninstall.
- Schema v2 migration preserving historical project records.
- Responsive compact-window layout and explicit partial-success states.

Implementation commits: `7d7d2d9`, `86aa8a0`, `c27577e`, and follow-up fix
`b9303be`.

