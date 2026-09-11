# WP01 validation record

Validated on Linux/Omarchy on 2026-09-11, against the WP00 baseline
`7edb4578889228e1b3e87d5237f9a42eeade3f19`.

## Coordinator review

Codex reviewed the complete implementation, reference resolution, filesystem
access, serialization contract, fixtures, tests, capability configuration, and
documentation before PR creation. Claude workers handled the bounded Rust and
frontend assignments; Codex integrated and independently validated the result.

Review corrections included isolating the branding CSS from diagnostic text,
explicit initial-selection cancellation coverage, full typed command-result
comparison, and ensuring preservation snapshots do not themselves update
symlink access times. No blocking findings remain.

## Local gates

| Command | Result |
| --- | --- |
| `cargo check --workspace --locked` | PASS |
| `cargo fmt --check` | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --workspace` | PASS: 62 tests |
| `pnpm install --frozen-lockfile` | PASS |
| `pnpm lint` | PASS |
| `pnpm typecheck` | PASS |
| `pnpm test` | PASS: 19 tests, 3 files |
| `pnpm build` | PASS |
| `git diff --check` | PASS |

Rust coverage comprises 4 core tests, 3 desktop-command tests, and 55 Power BI
tests (18 unit, 18 discovery, 4 preservation, 15 security). PBIR/TMDL and
PBIR-Legacy/TMSL fixtures pass, along with both cross-format combinations.
Preservation tests compare bytes, directory entries, identity, permissions,
ownership, and timestamps across repeated discovery. A control checks whether
ordinary reads advance stale access times on the test filesystem.

The restricted execution sandbox initially blocked the Unix socket created by
one nonregular-file security test. The full suite was rerun outside that sandbox
and passed; no security assertion was removed or bypassed.

## Actual native Linux validation

Launched with `pnpm dev` (Tauri, GTK/WebKitGTK, Vite). Used the native filtered
file chooser and inspected the resulting application through Linux accessibility
APIs and a screenshot of the rendered window. The Tauri command bridge was real,
not mocked.

Selected `/home/matt/Development/biforgeworks/fixtures/powerbi/valid-pbir-tmdl/Sales.pbip`:

```text
Project: Sales
Report: Sales.Report — Found — PBIR
Semantic Model: Sales.SemanticModel — Found — TMDL
Diagnostics: None
Mode: Read-only
```

Reopened and cancelled the picker: the Sales summary remained visible.
Selected `fixtures/powerbi/malformed-pbip/Malformed.pbip`: the application
displayed `PBIP_INVALID_JSON` (line 8, column 0) and remained usable.

The examples are synthetic discovery fixtures, not complete Power BI-authored
reports. GTK/ATK logged assertions during accessibility-driven picker testing;
there was no startup failure or failed discovery. Native automation is not yet
part of CI. The development process was stopped deliberately after validation.

## Toolchain and limits

Rust/Cargo 1.98.0 stable; Node 26.7.0; pnpm 12.4.1; Tauri 2.11.5,
dialog plugin 2.7.3; React 19.3.0; TypeScript 6.0.3; Vite 8.3.0.

See [discovery architecture](../architecture/powerbi-project-discovery.md) for
supported metadata versions, Linux-only preservation, ownership restrictions,
symlink/confinement policy, remote-model handling, and concurrent/privileged
filesystem-change limitations. CI results and merge identifiers belong to the PR
and final handback.

No PBIP editing/saving, TMDL/PBIR semantic parsing, or Fabric publishing was
implemented. WP02 has not begun.
