# Architecture overview

## Status

WP00 established the application shell and layering. WP01 adds read-only
Power BI project discovery, format identification, and diagnostics. WP02 adds
snapshot-based explicit transactions inside the Power BI crate. The desktop
remains read-only; semantic parsing/editing and publishing remain future work.

## Implemented boundary

```text
React / TypeScript
       │
       ▼
   Tauri Commands
       ├── biforgeworks-core (application metadata)
       └── biforgeworks-powerbi (discovery)
                       │
                       ▼
             bounded local filesystem access
```

The same Rust crate also contains the WP02 write foundation:

```text
ProjectSession → ProjectSnapshot
       ↓
ProjectTransaction → explicit metadata operation
       ↓
validate / conflict checks → atomic file replacement
       ↓
rediscovery / preservation checks → fresh snapshot
       └── failure → rollback / retained recovery evidence
```

These write APIs are not exposed through Tauri in WP02. Future high-level
commands must use this boundary rather than grant frontend filesystem writes.

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
implemented** by WP02. They exist here only so that future work packages
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

The dedicated adapter contains outer project discovery and explicit safe-write
infrastructure, with no BFIR translation or semantic object model. The remaining layers must
not be implemented opportunistically; each is scoped to a future work
package per `docs/work-packages/`.

## Related documents

- Architecture decisions: `docs/adr/`
- Work package definitions: `docs/work-packages/`
- Discovery boundaries: [Power BI project discovery](powerbi-project-discovery.md)
- Dedicated crate decision: [ADR-0004](../adr/0004-powerbi-interoperability-crate.md)
- Save boundary: [Power BI safe writes](powerbi-safe-writes.md)
- Transaction decision: [ADR-0005](../adr/0005-snapshot-based-explicit-transactions.md)
