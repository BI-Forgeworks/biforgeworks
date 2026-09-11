# Architecture Decision Records

This directory records the significant architectural decisions made for BI
Forgeworks, using lightweight Architecture Decision Records (ADRs).

## When to write an ADR

Write an ADR when a change:

- Establishes or alters a boundary between layers (for example, the
  Rust/TypeScript boundary described in
  `docs/architecture/overview.md`).
- Chooses or replaces a foundational tool, framework, or package manager.
- Introduces a new category of dependency (for example, a data runtime, an
  auth library, or a UI framework).
- Changes something a prior ADR decided.

Routine implementation work inside an already-decided boundary does not
need a new ADR.

## Process

1. Copy the format below into a new file named `NNNN-short-title.md`, using
   the next sequential four-digit number.
2. Fill in Context, Decision, and Consequences.
3. Set Status to `Proposed` while under discussion, and `Accepted` once
   adopted. A later ADR that reverses a decision should mark the old one
   `Superseded by ADR-NNNN` rather than deleting it.
4. Keep ADRs short and specific to one decision.

## Format

```markdown
# ADR-NNNN: Title

## Status

Accepted

## Context

What problem or question this decision addresses.

## Decision

What was decided.

## Consequences

What this makes easier, harder, or constrains going forward.
```

## Index

- [ADR-0001: Rust Core / TypeScript Presentation Boundary](0001-rust-core-typescript-presentation-boundary.md)
- [ADR-0002: pnpm as JavaScript Package Manager](0002-pnpm-as-javascript-package-manager.md)
- [ADR-0003: Monorepo Architecture](0003-monorepo-architecture.md)
