# Skill Installer macOS 0.4.0 plan

## Scope

This milestone closes the lifecycle gaps left after macOS 0.3.0. Windows source, CI and
handoff documents are outside this milestone.

## Delivered slices

1. Safe updates
   - Generate a read-only update plan from tracked source provenance.
   - Show file additions, modifications and removals.
   - Require per-target confirmation, back up before replacement and allow partial success.
   - Pin or unpin individual installations and roll back completed updates.
2. Reproducible migration
   - Export source, source revision, file-tree hash, consumers and pin state to schema v1 JSON.
   - Re-resolve GitHub entries at their recorded commit when available.
   - Reject hash drift and report missing IDEs or unavailable sources.
   - Never automatically delete installations that are not present in the imported lockfile.
3. Unified operation center
   - Journal install, update, adopt, uninstall and restore operations.
   - Separate crash recovery from deliberate rollback of completed work.
   - Configure backup count, total space and retention; protect active recovery backups.
4. Delivery
   - Upgrade the application and state schema to 0.4.0/schema v4.
   - Run React, TypeScript, Rust, rustfmt and Clippy verification.
   - Build ad-hoc signed local DMG and dual-architecture GitHub Actions artifacts.

## Claim boundaries

- A portable ZIP includes actual Skill files and is appropriate for offline migration or
  adopted Skills without a source.
- A JSON lockfile is reproducible only while its recorded source remains accessible; every
  resolved file tree must match the recorded SHA-256 hash.
- CI DMGs use the Tauri ad-hoc identity unless Apple Developer ID and notarization credentials
  are explicitly configured. Ad-hoc signing does not establish developer identity or notarize
  the app.
