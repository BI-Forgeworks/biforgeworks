# Architecture overview

## Status

WP00 established the application shell and layering. WP01 adds read-only
Power BI project discovery, format identification, and diagnostics. Editing,
semantic parsing, and publishing remain future work.

## Implemented boundary

```text
React / TypeScript
       │
       ▼
   Tauri Commands
       ├── biforgeworks-core (application metadata)
       └── biforgeworks-powerbi (read-only project discovery)
                       │
                       ▼
             bounded local filesystem access
```

- **React / TypeScript** (`apps/desktop/src`, `packages/ui`) is the
  presentation layer. It renders UI and calls into the backend through
  explicit Tauri command interfaces. It does not hold canonical business or
  domain logic.
- **Tauri Commands** (`apps/desktop/src-tauri`) form the explicit,
  typed boundary between the frontend and the Rust core. The Tauri backend
  provides native PBIP selection and delegates discovery to
  `crates/biforgeworks-powerbi`. Application metadata still comes from core.
- **Rust Core Libraries** (`crates/biforgeworks-core`, `crates/biforgeworks-powerbi`, and future crates)
  hold reusable application and domain logic. This is the authoritative
  layer for project and domain behavior.

This boundary exists so that GUI, CLI, and future agent interfaces can
converge on the same core commands rather than duplicating logic per
surface.

## Reserved conceptual layers

The following layers are reserved in the architecture but are **not
implemented** by WP01. They exist here only so that future work packages
have a named place to land, and so that early decisions do not foreclose
them:

- **Project** — a general project/domain layer beyond WP01's Power BI summary.
- **BFIR** — a future, vendor-neutral BI intermediate representation.
- **Power BI Adapter** — translation between PBIP/PBIR/TMDL and BFIR.
- **Fabric** — integration with Microsoft Fabric publishing/workspaces.
- **Connectors** — data source connectors (see `connectors/`).
- **Runtime** — a local analytical runtime.
- **Visuals** — HTML/SVG and geospatial visual rendering.
- **Agents** — agent-facing interfaces built on the shared core commands.

WP01's dedicated adapter contains only outer project discovery, with no BFIR
translation or semantic object model. The remaining layers must
not be implemented opportunistically; each is scoped to a future work
package per `docs/work-packages/`.

## Related documents

- Architecture decisions: `docs/adr/`
- Work package definitions: `docs/work-packages/`
- Discovery boundaries: [Power BI project discovery](powerbi-project-discovery.md)
- Dedicated crate decision: [ADR-0004](../adr/0004-powerbi-interoperability-crate.md)
