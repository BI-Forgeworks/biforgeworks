# WP02 — Lossless Preservation and Safe Project Write Foundation

## Objective and baseline

Establish snapshot-based, explicit project transactions before semantic editing.
Accepted baseline: `dd5ad87b74836b1f454c2fd58f30f2e45b759cdd` (WP00/WP01 merged).
Branch: `feature/wp02-safe-project-writes`.

Read AGENTS, README, CONTRIBUTING, architecture overview and discovery document,
all ADRs, WP00, WP01 and its validation record, this specification, and the
existing Power BI crate before implementation.

## Requirements

- Extend `biforgeworks-powerbi`; React/Tauri receive no arbitrary write API.
- Snapshot relevant complete project content with relative paths, authoritative
  content hashes, size, timestamps, identity, and capture time.
- Distinguish Managed, Preserved, and Unknown files. Only explicitly managed
  files may change; unknown bytes/properties and opaque directories survive.
- Support exactly one controlled non-semantic operation. Prefer an existing
  low-risk PBIP metadata field; otherwise use a synthetic test artifact without
  pretending it is a real editing feature. No general editing UI is required.
- Explicit transactions stage changes, validate, detect conflicts, persist
  atomic file replacements, validate through WP01 rediscovery, and refresh the
  snapshot after successful commit.
- Validate encoding/JSON as appropriate, required paths, references, and unique
  destinations before replacing source files.
- Compare contents and identity immediately before commit. External edits,
  deletion, replacement, file-type changes, and changes to preserved files must
  be detected. Conservative refusal for every conflict is acceptable. No merges.
- Retain WP01 path/symlink protections. Destinations derive from trusted project
  metadata, never arbitrary frontend paths or absolute relative destinations.
- Stage recoverable originals and complete replacements, flush before rename,
  clean successful temporary artifacts, and provide rollback on failed saves.
  Document per-file atomicity versus multi-file/crash guarantees accurately.
- Detect recovery data after interrupted saves where practical. Never silently
  delete unrecognized recovery artifacts or overwrite subsequent external edits.
- Prevent concurrent saves of the same project within this process.
- Track Clean, Dirty, Saving, Conflict, and Error in Rust.
- Return typed stable diagnostics, including external change/deletion,
  conflict, unsafe target, write/validation/rollback failure, and recovery needed.
- Preserve LF/CRLF, BOM, encoding, trailing-newline behavior for untouched
  content. Document behavior for intentionally modified files.

## Tests and fixtures

Expand synthetic `fixtures/powerbi` cases for preservation, unknown files and
properties, mixed encodings/newlines, external changes, and multi-file rollback.
Never commit customer/client data. Build a reusable full-tree comparison helper
reporting additions, removals, byte modifications, and relevant metadata changes.

Cover snapshots, no-op preservation, a controlled edit, unknown-content
preservation, external modification/deletion/replacement/type changes, unsafe
targets, atomic replace, write failure, rollback, concurrent save prevention,
post-save discovery, and fresh snapshots. Inject deterministic failures after
staging, during writes, before replace, during multi-file commit and validation.
Test any newly displayed frontend states if the UI changes.

If an approved safe real Desktop-created PBIP is locally available, snapshot and
perform a no-op or controlled test on a copy, verify preservation, and reopen it
in Power BI Desktop where available. Report unavailable validation explicitly.

## Documentation and gates

Create `docs/architecture/powerbi-safe-writes.md`, update the overview, and record
ADR-0005 for explicit snapshot-validated transactions. Document snapshots,
preservation, conflicts, atomic replacements, rollback/recovery, and limitations.

Run `cargo check --workspace --locked`, `cargo fmt --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo test --workspace`, `pnpm install --frozen-lockfile`, `pnpm lint`,
`pnpm typecheck`, `pnpm test`, `pnpm build`, and `git diff --check`.
CI must pass; the native Linux application must still launch.

## Orchestration, acceptance, and stop condition

Codex owns integration, independent review/tests, Git/PR/CI and merge. Claude
Opus/Sonnet workers handle bounded implementation/review/testing assignments.
Workers do not own Git integration or merges. After review and all gates, commit
only WP02, push, create PR, verify CI, merge, and synchronize clean main.

Acceptance requires demonstrated unchanged unknown/preserved content and no-op
behavior, intended-only edits, safe paths, atomic file writes, conflict and
concurrency protection, recoverable failed saves, post-save rediscovery and fresh
snapshots, reusable Rust transactions, passing tests/CI, and native startup.

No TMDL semantic parser, DAX editing, PBIR visual/page editing, Fabric work, BFIR,
or generalized serialization/generation framework. After WP02 STOP; do not start
WP03 (TMDL Reader and Semantic Model Inspection).

## Handback

Report Complete/Partial/Blocked; branch, PR, merge commit and final main;
delivered snapshot/stage/validate/commit/rollback/refresh architecture;
preservation/conflict/failure evidence; path safety; every gate and test counts;
real-PBIP and Linux validation; files/APIs/dependencies; honest limitations;
explicit exclusions and readiness for WP03. Include successful controlled-save
and blocked-external-change demonstrations with unexpected modifications counted.
