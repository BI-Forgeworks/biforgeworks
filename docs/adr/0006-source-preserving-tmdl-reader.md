# ADR-0006: TMDL parsing uses source-preserving syntax and typed semantic projection

## Status

Accepted in WP03.

## Context

The Linux viewer needs semantic objects without losing opaque expressions or
future metadata. A regex-only reader or direct deserialization into a reduced
model would not establish reliable scopes and source locations for later editing.

## Decision

Parse TMDL structurally in Rust inside `biforgeworks-powerbi`. Retain original
documents and source-aware syntax, then project supported objects into typed
Power BI-specific inspection structures. DAX, M, and function bodies remain opaque.
Unknown constructs survive with warnings; invalid documents are explicitly marked
and excluded from reliable semantic projection. Source and model are separate.

The desktop command inspects the backend-owned current project. React presents
the result and receives no arbitrary file-reading or writing interface. Existing
WP02 persistence APIs remain separate and unused by the reader.

## Consequences

- Source spans and explicit lineage/fallback identity support future bounded edits.
- UI DTOs do not become the authoritative serialization format.
- Typed coverage is intentionally smaller than TOM; unsupported syntax is retained.
- Tests must check expression bytes, source locations, scoping, diagnostics, and
  zero changes to source content and filesystem metadata.
- Real Desktop-generated corpus validation is required before WP04 production
  semantic writes. No writer or expression parser is introduced in WP03.

See [TMDL reader architecture](../architecture/tmdl-reader.md).
