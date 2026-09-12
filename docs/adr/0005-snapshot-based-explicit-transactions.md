# ADR-0005: Power BI writes use snapshot-based explicit transactions

## Status

Accepted in WP02.

## Context

WP01 opens untrusted PBIP projects without writes. Later semantic editors need
a shared preservation and persistence boundary before they can safely modify
project content. Whole-project serialization would discard unknown constructs;
timestamp-only conflict detection could silently overwrite another tool's work.

## Decision

Power BI project mutation belongs in `biforgeworks-powerbi` and occurs through
explicit transactions validated against a complete source snapshot. Content
hashes are authoritative, supplemented by file identity and metadata. Changes
to preserved or unknown content conservatively block a save.

Production staging accepts a specific operation on trusted project metadata,
not arbitrary paths and bytes. WP02's operation changes only an existing PBIP
`settings.enableAutoRecovery` boolean token. It neither adds a missing setting
nor serializes the surrounding JSON. No editing control or unrestricted write
command is exposed to React or Tauri.

Before replacement, validate staged content and source conflicts, and retain
recoverable originals. Replacement is atomic per file on supported Linux
filesystems. Multi-file operations use rollback and recovery evidence; they are
not claimed to be atomic as a group. A failed or interrupted rollback must
retain recovery data and report that intervention is required. Reopen through
WP01 discovery and refresh the snapshot after a successful save.

## Consequences

- Rust owns dirty/save/conflict state and the persistence rules for future UI,
  CLI, and agent operations. No generic BFIR writer is introduced.
- Unknown files and JSON properties are preserved byte-for-byte. Explicitly
  managed content has an operation-specific serialization contract.
- Same-process saves for one physical project root are serialized. External
  writers cannot be made cooperative by this process-local lock.
- Per-file rename atomicity does not imply multi-file or power-loss atomicity.
  Filesystem assumptions, rollback limits, and recovery instructions are part
  of the architecture contract.
- The desktop remains read-only until a later work package authorizes an
  end-user editing operation. WP02 does not parse TMDL, DAX, or PBIR semantics.

## References

- [Safe-write architecture](../architecture/powerbi-safe-writes.md)
- [Microsoft PBIP properties schema](https://github.com/microsoft/json-schemas/blob/main/fabric/pbip/pbipProperties/1.0.0/schema.json)
- [WP02](../work-packages/WP02-safe-project-writes.md)
