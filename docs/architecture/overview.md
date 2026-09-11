# Architecture overview

## Status

This document describes the architecture implemented as of WP00
(repository bootstrap). It is intentionally minimal: WP00 establishes the
application shell and layering, not any Power BI or analytics
functionality.

## Implemented boundary

```text
React / TypeScript
       │
       ▼
   Tauri Commands
       │
       ▼
 Rust Core Libraries
```

- **React / TypeScript** (`apps/desktop/src`, `packages/ui`) is the
  presentation layer. It renders UI and calls into the backend through
  explicit Tauri command interfaces. It does not hold canonical business or
  domain logic.
- **Tauri Commands** (`apps/desktop/src-tauri`) form the explicit,
  typed boundary between the frontend and the Rust core. The Tauri backend
  depends on and calls into `crates/biforgeworks-core`.
- **Rust Core Libraries** (`crates/biforgeworks-core` and future crates)
  hold reusable application and domain logic. This is the authoritative
  layer for project and domain behavior.

This boundary exists so that GUI, CLI, and future agent interfaces can
converge on the same core commands rather than duplicating logic per
surface.

## Reserved conceptual layers

The following layers are reserved in the architecture but are **not
implemented** in WP00. They exist here only so that future work packages
have a named place to land, and so that early decisions do not foreclose
them:

- **Project** — the in-memory representation of an opened BI project.
- **BFIR** — a future, vendor-neutral BI intermediate representation.
- **Power BI Adapter** — translation between PBIP/PBIR/TMDL and BFIR.
- **Fabric** — integration with Microsoft Fabric publishing/workspaces.
- **Connectors** — data source connectors (see `connectors/`).
- **Runtime** — a local analytical runtime.
- **Visuals** — HTML/SVG and geospatial visual rendering.
- **Agents** — agent-facing interfaces built on the shared core commands.

None of these layers have code, schemas, or dependencies in WP00. They must
not be implemented opportunistically; each is scoped to a future work
package per `docs/work-packages/`.

## Related documents

- Architecture decisions: `docs/adr/`
- Work package definitions: `docs/work-packages/`
