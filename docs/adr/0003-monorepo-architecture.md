# ADR-0003: Monorepo Architecture under `apps/`, `crates/`, and `packages/`

## Status

Accepted

## Context

BI Forgeworks combines a Rust workspace, a Tauri desktop shell, a React
frontend, and future connectors/targets/fixtures for Power BI project
formats. These pieces are developed together, versioned together, and need
to build and test against each other (for example, the Tauri backend must
depend on and call `biforgeworks-core`). Splitting them into separate
repositories would add coordination overhead without a corresponding
benefit at this stage.

## Decision

Use a single monorepo with a top-level layout of:

- `apps/` — deployable applications, starting with `apps/desktop` (the
  Tauri 2 desktop shell and its React frontend).
- `crates/` — shared Rust libraries, starting with
  `crates/biforgeworks-core`.
- `packages/` — shared TypeScript/React packages, starting with
  `packages/ui`.
- `connectors/`, `targets/`, `fixtures/` — future-facing directories for
  data connectors, publish targets, and test fixtures, kept empty or
  near-empty until the work packages that need them.

The Cargo workspace (resolver 2 or the stable equivalent) and the pnpm
workspace (`apps/*`, `packages/*`) both live at the repository root.

## Consequences

- A single PR can atomically change the Rust core, the Tauri commands that
  expose it, and the React code that calls it, keeping the frontend/backend
  contract in sync.
- CI validates the whole workspace together, which is simpler than
  coordinating versions and releases across multiple repositories, at the
  cost of a single CI run covering more ground (mitigated by running Rust
  and frontend validation as separate jobs).
- New top-level directories should fit one of the existing categories
  (`apps/`, `crates/`, `packages/`, or a documented future-facing
  directory) rather than proliferating ad hoc top-level folders.
- This decision does not by itself decide package boundaries within
  `crates/` or `packages/` beyond what WP00 creates; further splits are
  future work-package decisions.
