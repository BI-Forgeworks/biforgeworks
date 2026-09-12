# WP03 — TMDL Semantic Model Reader and Inspection

## Objective and baseline

Deliver source-aware, read-only TMDL semantic inspection in the Linux desktop.
Accepted main: `31fb2a26a69798dfa0398d6f562a7e2b70fd4536` (WP00–WP02 merged).
Implementation branch: `feature/wp03-tmdl-semantic-reader`.

The near-term priority is a usable Linux-native Power BI viewer/editor. Fabric
publishing, connectors, BFIR portability, alternative exports, visuals, and agent
interfaces are deferred. This package does not authorize any semantic writes.

## Preparation and ownership

Confirm clean synchronized main before modifying files. Read AGENTS, README,
CONTRIBUTING, architecture overview/discovery/safe-writes, all ADRs, WP00, WP01,
WP01-validation, WP02, and WP02-validation. Inspect the current Power BI crate,
Tauri command bridge, and React UI before choosing interfaces.

Codex owns research, architecture, implementation, fixtures, UI, tests,
performance, documentation, integration, independent review, Git, PR, CI, merge,
and handback. At most **one Claude worker** may be used, solely for bounded TMDL
parser/compatibility review. It must not spawn workers, edit broadly, own
architecture decisions, or perform Git/PR/merge operations.

## Compatibility research

Use current official Microsoft documentation for syntax, indentation, folder
layout, object definitions, quoted identifiers, descriptions, annotations,
lineage, expressions, and functions. Record compatibility findings. Do not claim
complete TOM compatibility from documentation or synthetic tests alone.

## Architecture and preservation

Keep TMDL behavior inside `crates/biforgeworks-powerbi/tmdl` (under `src`).
Pipeline: source → lexical/structural syntax → typed semantic projection → UI DTO.
Do not use a collection of regular expressions as the parser.

Support indentation/scoping, quoted names, multiline/fenced expressions, source
locations, unsupported syntax, and diagnostics. Retain original files, byte spans,
line/column locations, and raw DAX/M expressions. Do not normalize source or force
future editors to reconstruct entire files from a reduced semantic model.

Read `definition/` including database/model, tables, relationships, roles,
perspectives, cultures, expressions, and functions. Optional files may be absent.
Do not load TMDL view scripts as model definitions. DAX and M remain opaque text.

## Semantic coverage

- Database/model identity, culture, descriptions, annotations, source query
  culture, and useful known properties; preserve unfamiliar metadata.
- Tables: names, descriptions, hidden state, lineage, data category, annotations.
- Data/calculated/calculated-table columns where distinguishable: type, source,
  expression, format, summarization, hidden state, description, category, lineage,
  sort reference, annotations.
- Measures: names, opaque DAX, format, description, display folder, hidden state,
  lineage, annotations. Faithful multiline handling is a priority.
- Hierarchies and ordered levels with column references and diagnostics.
- Partitions: name, mode, source kind, opaque source expression.
- Relationships: endpoints, cardinality, filtering/security behavior, active
  state, annotations; resolve table/column references where possible.
- Roles: permissions and table/model filter information; no RLS execution.
- Perspectives: included tables, columns, measures, hierarchies; resolve references.
- Cultures: name, source, available translation metadata and useful counts.
- Named expressions: names, bodies, kind/content metadata.
- Functions: typed declarations and opaque DAX bodies, never silently skipped.

Use typed domain structures rather than arbitrary JSON maps. Retain lineage IDs;
otherwise derive deterministic project-scoped identities, never array positions.
Distinguish invalid syntax errors from unsupported constructs retained with
warnings. Diagnostics carry severity, stable code, message, and source location.
Cover syntax errors, unknown properties, duplicates, unresolved relationships and
hierarchies, and unsupported objects.

## Desktop

Add a narrow semantic-inspection command operating on the backend's open project;
do not expose `read_tmdl_file(path)` or filesystem access to React. Expand the
project experience with semantic groups and a selectable, read-only inspector.
Show measure DAX/format/folder, relationship endpoints/behavior, and partition
mode/source/M. Include roles, perspectives, cultures, expressions and functions.
Search is optional and must not delay parser completion. TMSL remains discoverable
but explicitly unsupported for semantic inspection.

## Fixtures and tests

Use non-sensitive synthetic fixtures, documenting any public-example provenance.
Cover minimal models, star schema, measures/multiline DAX, hierarchy, partitions/M,
roles, perspectives, cultures, expressions, functions, unknown properties, broken
references, and invalid syntax. Related cases may share a representative fixture.

Rust tests cover every supported object, quoting, duplicate declarations,
indentation errors, source locations, opaque expression fidelity, and preservation.
WP01/WP02 tests remain green. Frontend tests cover the semantic tree, table/column/
measure/relationship/partition inspectors, diagnostics, and unsupported TMSL state.

Include a moderate synthetic model with 50 tables, 500 columns, 250 measures, and
100 relationships. Measure parse duration and memory observations where practical.
Attempt to locate an approved Desktop-created TMDL PBIP; never use private client
data. Synthetic coverage may complete WP03, but real Desktop validation becomes a
**hard prerequisite before WP04 production semantic writes** if unavailable.

## Documentation and validation

Create `docs/architecture/tmdl-reader.md` and ADR-0006 for source-preserving syntax
with typed semantic projection. Update README and overview only for delivered
capabilities. Run independently:

```sh
cargo check --workspace --locked
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
pnpm install --frozen-lockfile
pnpm lint
pnpm typecheck
pnpm test
pnpm build
git diff --check
./scripts/check.sh
```

Launch and validate the actual native Linux Tauri application. Show a representative
summary plus an inspected measure and relationship in the handback.

## Acceptance, Git, and stop condition

Acceptance requires the read-only parser and inspector, typed source-aware objects,
opaque expressions, useful diagnostics, preservation, performance evidence,
passing local/CI gates, and native validation. Review the complete scoped diff,
commit, push, open PR, verify CI, review the PR diff, merge only the reviewed head
with passing checks, and return main clean and synchronized.

No TMDL writes, semantic editing, DAX/M parser, PBIR parser/canvas, visual editing,
Fabric/auth/publishing, connectors, DuckDB, Arrow, BFIR, HTML/Mapbox visuals, MCP,
or agent integration. **Stop after WP03; do not begin WP04.**

Handback: Complete/Partial/Blocked; repository/branch/PR/merge/final main; delivered
pipeline and semantic coverage; diagnostics/preservation; every gate and counts;
performance; native validation; Claude count/assignment/findings; real Desktop
validation or gap; limitations; explicit scope confirmations; WP04 readiness.

Planned later packages, not authorized here: WP04 Edit TMDL, WP05 DAX Editing UX,
WP06 Model Diagram, WP07 Read PBIR, WP08 Report Canvas, WP09 Basic Visual Editing,
WP10 PBIP Round-Trip Hardening.
