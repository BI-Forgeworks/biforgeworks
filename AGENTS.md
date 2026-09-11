# Agent instructions

This file governs how automated agents (ChatGPT, Codex, Claude/Herdr workers,
and any future agent) work in the BI Forgeworks repository. It applies to the
whole repository unless a more specific `AGENTS.md` overrides it for a
subdirectory.

## Principles

1. BI Forgeworks is GUI-first but code-native.
2. Rust is authoritative for project and domain behavior.
3. React is the presentation layer, not the canonical semantic layer.
4. Power BI formats are adapters, not the ultimate internal architecture.
5. Future BFIR (BI Forgeworks Intermediate Representation) remains
   vendor-neutral.
6. Credentials are never committed to project files or fixtures.
7. Unknown valid external constructs should eventually be preserved rather
   than silently deleted.
8. GUI, CLI, and future agent interfaces converge on shared core commands.
9. Work-package boundaries are authoritative.
10. Future work packages are not implemented opportunistically.
11. Architecture changes require documentation and an ADR where appropriate.
12. Tests belong with implementation.
13. Unnecessary dependencies are avoided.
14. Agents may not silently broaden scope.

## Ownership

- **ChatGPT** owns architecture, planning, work-package specifications,
  acceptance criteria, and cross-WP design.
- **Codex** owns repository coordination, integration, review, Git
  operations, PR preparation, and test verification.
- **Claude/Herdr workers** may implement bounded assignments, run focused
  tests, and report findings. They do not independently own architecture,
  merges, unbounded refactoring, or repository-wide Git integration.

## Working within an assignment

- Implement only the scope given for the current work package or task
  assignment. Do not opportunistically start the next work package.
- Respect file and directory ownership assigned for a task. In a shared
  checkout, leave staging, committing, branching, pushing, merging, and PR
  creation to the agent designated to own Git operations for that
  assignment (normally Codex) unless explicitly instructed otherwise.
- Do not spawn additional workers or subagents from within a bounded
  assignment unless explicitly authorized.
- Report changes, validation results, and blockers back to the agent that
  issued the assignment.

## Work packages

Work packages live in `docs/work-packages/` and are named
`WPNN-short-description.md`. Each package defines its own scope and
acceptance criteria. See `docs/work-packages/README.md`. Work must stop at
the package boundary until the next package is authorized — do not begin
implementing a future work package's functionality opportunistically.

## Architecture documentation

Architecture is documented in `docs/architecture/`. Architectural decisions
are recorded as Architecture Decision Records (ADRs) in `docs/adr/`,
following the process in `docs/adr/README.md`. A change that affects the
boundary between layers, introduces a new dependency category, or alters a
previously recorded decision requires an ADR.
