# ADR-0004: Power BI interoperability isolated behind a dedicated Rust crate

## Status

Accepted in WP01.

## Context

WP01 introduces the first format-specific behavior. Project discovery must be
reusable outside the desktop shell and must handle untrusted local metadata
without granting React general filesystem access.

## Decision

Power BI reference resolution, format identification, metadata validation, and
diagnostics belong in `biforgeworks-powerbi`. Tauri owns native selection and
asynchronous command dispatch; React renders typed results. The existing
`biforgeworks-core` remains the home for shared vendor-neutral behavior, but no
artificial core dependency or BFIR types are introduced merely for this adapter.

Expose only a filtered PBIP selector and project-discovery command. The dialog
plugin is called from Rust; its general JavaScript commands and the filesystem
plugin are not granted to the frontend. Discovery performs no writes and loads
no remote resources or executable project content.

## Consequences

- GUI, future CLI, and future agent surfaces can share discovery and diagnostics.
- Power BI metadata remains explicitly vendor-specific; this is not BFIR.
- Native selection and UI tests can evolve independently of format handling.
- Strict confinement and timestamp-preserving reads may reject projects that
  Power BI Desktop can open; those restrictions must be documented and diagnosed.
- Editing, preservation/round-trip operations, semantic parsing, and publishing
  require later work packages.
