# WP01 — PBIP Project Discovery and Read-Only Open

## Objective and baseline

Open an existing Power BI Project (`.pbip`) on Linux, discover its report and
semantic-model components, identify formats, validate its outer structure, and
display a read-only project summary. WP00 must be merged; the expected baseline
is `7edb4578889228e1b3e87d5237f9a42eeade3f19`. Work on
`feature/wp01-pbip-project-open`, never directly on main.

## Required reading

Read AGENTS.md, README.md, CONTRIBUTING.md, docs/architecture/overview.md,
docs/adr/*, WP00-repository-bootstrap.md, this specification, and the existing
Tauri command bridge before implementation.

## Architecture and scope

Create `crates/biforgeworks-powerbi` for reusable Power BI discovery, format
identification, typed metadata, and diagnostics. React is presentation only.
Tauri commands delegate to that crate; shared core types are reused only where
appropriate. Do not introduce BFIR or put parsing in Tauri handlers.

The native UI provides **Open Power BI Project**, selects a `.pbip`, and displays
the project, report, semantic model, formats, and diagnostics. Use the narrowest
native file-selection interface; never expose unrestricted filesystem APIs or
`read_file(path)` to React. Selection cancellation preserves current state.

## Discovery contract

Discover project file, root, name, report reference/folder, and semantic-model
reference/folder. Resolve relative references on Linux, including a model beside
the report through `../`, without accepting references outside the project root.
Report formats: `PBIR`, `PBIR_LEGACY`, `UNKNOWN`, `MISSING`.
Model formats: `TMDL`, `TMSL`, `UNKNOWN`, `MISSING`.
Use documented content/structural markers rather than folder names. Inspect only
outer metadata and markers; never parse TMDL grammar, DAX, PBIR visuals, pages,
tables, measures, relationships, or other semantic objects.

Return typed serializable summaries with project file/root/name, components
(path, exists, format), and diagnostics (severity, stable code, message, path).
Handle missing PBIP, invalid JSON, missing references/directories, unknown
formats, and escaped references distinctly. Suggested diagnostic codes include
PBIP_NOT_FOUND, PBIP_INVALID_JSON, REPORT_REFERENCE_MISSING,
REPORT_FOLDER_NOT_FOUND, SEMANTIC_MODEL_REFERENCE_MISSING,
SEMANTIC_MODEL_FOLDER_NOT_FOUND, UNKNOWN_REPORT_FORMAT, UNKNOWN_MODEL_FORMAT,
and REFERENCE_OUTSIDE_PROJECT. Do not panic on untrusted inputs.

## Security and read-only guarantee

Treat project metadata as untrusted. Address traversal, escaping references,
symlinks, malformed JSON, invalid UTF-8 paths/content, missing files, unreasonable
metadata sizes, and nonregular files. Do not execute content, fetch remote
schemas, load visuals, or emit secrets/connection strings in diagnostics.
Selected project files receive zero writes: no saving, formatting, normalization,
migration, backups, timestamp changes, temporary files, or deletion. Automated
regression coverage must compare contents and metadata before/after discovery.
Temporary fixture construction belongs in separate temporary directories.

## UI

Initial view: BI Forgeworks, Power BI Engineering, Open Power BI Project.
Opened view: high-level Project → Report / Semantic Model explorer and summary
showing paths, formats, structural status, and clear diagnostics. No lower-level
objects or editing controls. Logging remains minimal and local.

## Fixtures and tests

Add minimal synthetic fixtures beneath `fixtures/powerbi/`:
valid-pbir-tmdl, valid-legacy, missing-report, missing-model, malformed-pbip,
unknown-formats. Do not commit customer reports or secrets.

Rust tests cover valid discovery, relative resolution, all four formats, missing
references/directories, invalid JSON, unknown formats, traversal/escape, symlinks,
invalid inputs, and read-only preservation. Test the Tauri delegation and typed
response. Frontend tests cover initial/open state, successful summary, diagnostics,
and cancellation/no selection.

## Documentation and decisions

Update README and architecture overview after implementation. Add
`docs/architecture/powerbi-project-discovery.md` covering selection, references,
markers, diagnostics, read-only behavior, and security boundaries. Record ADR-0004
for the dedicated Power BI interoperability crate, or explain in handback why it
is unnecessary. Document limits honestly: discovery is not semantic validation.

## Acceptance gates

- Dedicated crate and native Linux PBIP selection work.
- Report/model references and four formats are discovered accurately.
- Malformed/missing/unknown inputs produce useful diagnostics.
- Escapes are rejected or safely diagnosed; no project content is executed.
- No unrestricted filesystem API or project write operations are introduced.
- Automated read-only regression coverage passes.
- UI renders components, formats, and diagnostics.
- `cargo fmt --check` passes.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
- `cargo test --workspace` passes.
- `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build` pass.
- GitHub CI passes and actual native Linux selection/open is validated.

## Orchestration and Git

Codex owns coordination, integration, review, Git, final tests, PR, and merge.
Claude/Herdr workers implement bounded assignments and report tests/findings;
they do not own architecture, PR creation, merge, or repository-wide Git actions.
Review complete diff, commit scoped work, push, create PR, verify CI, merge only
when established conditions pass, and return main clean and synchronized.

## Exclusions and stop condition

No project editing/saving, TMDL AST/writer, DAX editor, model diagram, PBIR semantic
parser/report canvas, visual editing, Fabric authentication/APIs/publishing,
workspace browsing, Power BI Service, BFIR, DuckDB, Arrow, connectors, HTML
visuals, Mapbox, MCP, or other future packages.

After WP01 is complete and merged, STOP. Do not begin WP02 (lossless preservation
and safe round-trip foundations).

## Required handback

Report status (Complete/Partial/Blocked), branch/PR/merge commit/final main,
delivered capabilities, architecture/deviations, actual PBIR/PBIR-Legacy/TMDL/TMSL
tests, security/read-only/malformed-input behavior, all gate results and counts,
native Linux validation, representative local discovery result, important
files/crates, limitations, and readiness for WP02. Explicitly confirm no PBIP
editing/saving, no TMDL/PBIR semantic parsing, and no Fabric publishing work.
