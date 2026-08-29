# Skill Installer macOS 0.4.1 plan

## Scope

This milestone hardens operation safety and local state durability. Windows source and CI
remain independent and are not modified here.

## Delivered slices

1. Schema v5 and durable state
   - Add monotonic `revision` to `state.json`.
   - Migrate v1-v4 state after saving a byte-for-byte copy under
     `backups/state-migrations/`.
   - Refuse future schema versions instead of rewriting unknown fields.
   - Write through a temporary file with `fsync`, atomic replacement and a serialized
     repository mutation path.
2. Stale-plan protection
   - Include `createdAt` and `expiresAt` in install and update plans.
   - Capture source snapshot hashes and target existence/content hashes at preview time.
   - Revalidate every source and target immediately before apply. A stale plan returns
     per-entry `stale` results and performs no backup, journal or filesystem write.
   - Local directory sources are copied to an inspection snapshot, so an apply never reads
     a directory that the user may be changing.
3. Recovery truthfulness
   - Journal targets record the final `resultingHash` after replacement.
   - The operation center only exposes completed-operation rollback when required backups
     still exist.
4. Verification
   - Synthetic temporary-directory tests cover migration, future-version rejection,
     concurrent repository mutations, snapshot tampering, target tampering and rollback.
   - React and TypeScript regression tests remain green.

## Claim boundaries

This release continues to use ad-hoc macOS packages for local testing. Apple Developer ID
signing, notarization, Stapling, Homebrew distribution and automatic updates are explicitly
outside the 0.4.1 scope.
