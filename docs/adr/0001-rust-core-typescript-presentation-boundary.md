# ADR-0001: Rust Core / TypeScript Presentation Boundary

## Status

Accepted

## Context

BI Forgeworks is GUI-first but code-native. It needs a shell that can host a
rich desktop UI while keeping the option open for a future CLI and agent
interfaces to reuse the same underlying logic, without duplicating business
and domain behavior per surface. React/TypeScript is well suited to
building the desktop UI; it is not well suited to being the single source
of truth for domain behavior that must also be reachable from a CLI or
agent interface.

## Decision

Rust is authoritative for project and domain behavior. Reusable
application logic lives in Rust core crates (starting with
`crates/biforgeworks-core`), not in the Tauri shell or in React. React is
presentation-layer code: it renders UI state and calls into the backend
only through explicit Tauri command interfaces. The Tauri backend
(`apps/desktop/src-tauri`) depends on and calls the core crate rather than
reimplementing logic itself.

## Consequences

- Domain logic gets Rust's type system, tests, and performance, and is
  reusable from a future CLI or agent surface without rewriting it in
  TypeScript.
- The frontend stays a thin presentation layer; React state (no Redux,
  MobX, or similar) is sufficient because the frontend does not own
  canonical state.
- Every capability exposed to the UI must be deliberately surfaced as a
  Tauri command, which keeps the frontend/backend contract explicit but
  adds a small amount of boilerplate per capability.
- Power BI format handling and any future BFIR layer belong in Rust core
  crates, not in the frontend or in ad hoc Tauri command bodies.
