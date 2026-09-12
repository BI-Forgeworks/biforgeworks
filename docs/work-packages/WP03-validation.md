# WP03 validation

Validated locally on Linux on 2026-09-12 by Codex against
`feature/wp03-tmdl-semantic-reader`, based on
`31fb2a26a69798dfa0398d6f562a7e2b70fd4536`. GitHub check/merge identifiers are
reported in the PR and final handback rather than embedded before they exist.

## Repository gates

| Command | Result |
| --- | --- |
| `cargo check --workspace --locked` | PASS |
| `cargo fmt --check` | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --workspace` | PASS — 158 tests |
| `cargo test -p biforgeworks-powerbi --features safe_write_test_support` | PASS — 154 tests, overlapping the default suite and adding four fault-injection tests |
| `pnpm install --frozen-lockfile` | PASS |
| `pnpm lint` | PASS |
| `pnpm typecheck` | PASS |
| `pnpm test` | PASS — 27 tests across four files |
| `pnpm build` | PASS |
| `git diff --check` | PASS |
| `./scripts/check.sh` | PASS — runs the commands above |

New Rust coverage comprises 36 parser/projection cases, three confined-reader
security cases, one desktop bridge case, and expanded existing read-only fixture
coverage. Existing WP01/WP02 suites remain green. Eight new frontend tests cover
tree/table/column/measure/relationship/partition inspection, diagnostics, and TMSL.
The final complete gate run passed after fixing an enum alias in a new test.
An earlier unprivileged run hit the sandbox's Unix-socket restriction in an
existing security test; the authorized native-filesystem run passed unchanged.

## Demonstration and native Linux validation

Launched the actual application with `pnpm dev`, using Tauri/Rust and WebKitGTK,
not a browser-only mock. Selected the repository's synthetic
`fixtures/powerbi/tmdl-star-schema/Sales.pbip` through the native file dialog.
AT-SPI inspection and window screenshots verified the live explorer and inspector.

```text
Project: Sales
Report: PBIR
Semantic model: TMDL
Tables: 2
Columns: 7
Measures: 3
Relationships: 1
Roles: 1
Perspectives: 1
Discovery diagnostics: none
Semantic diagnostics: none
```

Selected `Gross Margin %` in Sales, with format `0.0%` and expression:

```dax
DIVIDE (
    [Gross Margin],
    [Sales Amount],
    0
)
```

Selected relationship `SalesDate`: `Sales[OrderDateKey]` (many) to
`Date[DateKey]` (one), active, one-direction filtering, references resolved.
The inspector exposes no editing controls. Source content and metadata preservation
are verified by automated tests; UI inspection is not a substitute for those tests.

Startup completed successfully. Automated native picker interaction emitted a
GTK `WIDGET_REALIZED_FOR_EVENT` assertion warning; selection and inspection still
worked. No application crash occurred. The development process was stopped
intentionally after validation.

## Performance

The deterministic 36,803-byte fixture has 50 tables, 500 columns, 250 measures,
and 100 relationships. The final debug test executable's isolated
`moderate_model_performance` run reported **38.55 ms**. Linux `getrusage` reported
**14,280 KiB peak RSS** for the entire test process (not isolated parser allocations).
This is a local debug observation, not a production latency guarantee or benchmark
across machines. Root declaration indexing avoids a previous linear search per
object; no obvious scaling issue appeared at this fixture size.

## Review and dependencies

**Claude workers used: 1.** The existing Opus worker performed only bounded
Microsoft syntax research and parser compatibility review, including throwaway
probes. It performed no repository edits or Git operations and spawned no workers.
Codex independently implemented and validated fixes for indentation widths,
closing-fence whitespace, literal fence deindentation, invalid-document projection,
source/BOM locations, role members, culture metadata, dotted functions, and
non-echoing diagnostics. Codex also reviewed partial declarations and resource
bounds. A requested final follow-up hit Claude's capacity limit; final integration
and validation were performed by Codex.

No new runtime/development dependencies or lockfile changes were introduced.
ADR-0006 records source-preserving syntax plus typed semantic projection.

## Real Desktop validation and limitations

No approved real Power BI Desktop-created TMDL PBIP was available. A filename-only
local search found repository synthetic examples, and no private report was opened
or committed. No Desktop reopen/round-trip compatibility is claimed.
**Real Desktop-generated corpus validation is a hard prerequisite before WP04
production semantic writes.**

- Structural typed coverage is smaller than TOM. Calculation groups and obscure
  objects remain preserved with warnings; complete translation and enum semantics
  are not implemented.
- DAX, M and function bodies are opaque source, not validated expression grammars.
- Invalid documents are retained but omitted from reliable semantic projection;
  other documents may produce a clearly marked partial model.
- Filesystem inspection is Linux-only and bounded. Reads detect observed changes
  but are not an atomic filesystem snapshot; inherited bind-mount limits remain.
- IDs without lineage change on rename; the project path scopes identities, so
  relocating a project changes IDs. This is documented, not cross-machine identity.
- TMSL is discovered but semantic inspection is explicitly unsupported.
- GTK emitted the native automation warning described above.

## Scope and next step

No TMDL writes, DAX parser, M parser, PBIR parsing, Fabric work, or BFIR were added.
WP02's existing controlled metadata transaction is unchanged. WP04 was not started.
The reader is ready for WP04 architecture review with the real-Desktop prerequisite
and the documented parser coverage limits carried forward.
