# WP02 validation

Codex independently reviewed the integrated implementation and ran the repository
gates on Linux on 2026-09-11. The starting main was
`dd5ad87b74836b1f454c2fd58f30f2e45b759cdd`; implementation used
`feature/wp02-safe-project-writes`.

## Repository gates

| Command | Result |
| --- | --- |
| `cargo check --workspace --locked` | PASS |
| `cargo fmt --check` | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --workspace` | PASS: 118 tests |
| `cargo test -p biforgeworks-powerbi --features safe_write_test_support` | PASS: 115 tests, overlapping the default suite with four additional tests |
| `pnpm install --frozen-lockfile` | PASS |
| `pnpm lint` | PASS |
| `pnpm typecheck` | PASS |
| `pnpm test` | PASS: 19 tests in three files |
| `pnpm build` | PASS |
| `git diff --check` | PASS |
| `./scripts/check.sh` | PASS: aggregate of the authoritative gates |

Rust tests passed on both tmpfs and the home directory's Btrfs filesystem.
The final Btrfs runs repeated the workspace and feature suites with zero failures,
ignored tests, or reported filesystem skips. An existing Unix-socket fixture was
made independent of temporary-directory path length using its open directory
descriptor; formatting and strict Clippy also passed after that test-only fix.

Installed versions: Rust 1.98.0, Cargo 1.98.0, Node 26.7.0, pnpm 12.4.1.
Selected versions: Tauri 2.11.5, React 19.3.0, TypeScript 6.0.3, Vite 8.3.0,
Vitest 5.0.0. SHA-256 uses sha2 0.10.9, already present transitively in the lockfile.

## Preservation and failure evidence

Coverage includes exact no-op preservation, unknown files and JSON properties,
BOM/CRLF preservation, token-only metadata changes, additions/deletions/replacement
conflicts, root replacement, unsafe paths and symlinks, bounded snapshots,
same-process locking through root aliases, post-save discovery, and fresh snapshots.
Deterministic injected failures cover staging, incomplete temporary writes,
replacement, multi-file rollback, validation, and retained recovery evidence.
Independent review regressions also cover external metadata/attribute changes,
conflicts after originals are staged, cancellation, and repeated staging intent.

The demonstration test opened a disposable copy of the synthetic
`preservation-unknown-files/WithExtras.pbip` fixture:

```text
Snapshot: 16 entries (10 files, 6 directories)
Managed change: WithExtras.pbip settings.enableAutoRecovery
Preserved regular files: 9 (including 4 unknown files)
Commit: SUCCESS
Post-save discovery: PBIR / TMDL
Unexpected modifications: 0

External edit: TempFiles/scratch.opaque
Next save: BLOCKED
Diagnostic: EXTERNAL_FILE_CHANGED
External edit preserved: YES
Unexpected modifications from blocked save: 0
```

Reproduce with:

```sh
cargo test -p biforgeworks-powerbi --test safe_writes demonstration_walkthrough_prints_evidence -- --nocapture
```

## Native Linux application

`pnpm dev` compiled and launched the actual Tauri application with default
features. Accessibility inspection and a screenshot confirmed the BI Forgeworks
window, branding, read-only message, and Open Power BI Project button rendered.
No startup errors occurred. The application was deliberately stopped with Ctrl+C
after validation; the resulting development-process exit is not a startup failure.
No frontend write command or apparent editing control was added.

## Real Power BI validation and limits

No approved, non-client Power BI Desktop-created sample was available in the
local development locations checked. No real PBIP round-trip or Power BI Desktop
reopen was performed. Synthetic tests do not establish Desktop compatibility.

Atomicity is per file. Multi-file crash atomicity and automatic recovery are not
provided. Retained recovery evidence requires manual inspection. Another tool can
still race the final check and rename. Snapshot limits, Linux-only writes, metadata
restrictions, and mount-boundary limitations are documented in
[the safe-write architecture](../architecture/powerbi-safe-writes.md).

The GitHub PR records remote CI and merge results. WP02 ends at this foundation:
no TMDL semantic parser, DAX editing, PBIR visual editing, or Fabric work was added.
