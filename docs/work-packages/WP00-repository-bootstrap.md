# WP00 — BI Forgeworks Repository Bootstrap

## Objective

Establish the production-quality repository, monorepo structure, Linux-native desktop application shell, engineering standards, CI pipeline, agent instructions, and architectural documentation framework required for all subsequent BI Forgeworks work packages.

WP00 does not implement Power BI functionality. It creates a clean, repeatable foundation for later work packages.

## Project identity

- Product: **BI Forgeworks**
- Canonical slug, repository name, and future CLI name: `biforgeworks`
- GitHub organization: **BI Forgeworks**
- Initial application identifier: `com.biforgeworks.desktop`
- Initial version: `0.0.1`

The old `biforge` slug must not be used.

## Product context

BI Forgeworks is intended to become a Linux-native, GUI-first, code-native analytics development environment. The first product milestone after the foundation is to open, inspect, safely edit, save, and publish Power BI PBIP/PBIR/TMDL projects from Linux.

Longer-term layers may include a portable BI intermediate representation, connectors, a local analytical runtime, HTML/SVG and geospatial visuals, agent interfaces, and standalone exports. These are future concepts only and must not be implemented in WP00.

## Required stack

- Stable Rust with `rustfmt`, Clippy, and `cargo test`
- Tauri 2 with a Rust backend
- React, strict TypeScript, and Vite
- pnpm workspaces
- Vitest for frontend tests
- A lightweight Playwright or Vite/React smoke test, with any limitation documented

Nightly Rust, global frontend state frameworks, and speculative UI component frameworks are excluded.

## Required repository structure

```text
biforgeworks/
├── apps/
│   └── desktop/
│       ├── src/
│       ├── src-tauri/
│       └── package.json
├── crates/
│   └── biforgeworks-core/
├── packages/
│   └── ui/
├── connectors/
├── targets/
├── fixtures/
├── docs/
│   ├── architecture/
│   ├── adr/
│   ├── research/
│   └── work-packages/
├── scripts/
├── .github/workflows/
├── Cargo.toml
├── Cargo.lock
├── pnpm-workspace.yaml
├── package.json
├── rust-toolchain.toml
├── .editorconfig
├── .gitignore
├── AGENTS.md
├── CONTRIBUTING.md
├── LICENSE
└── README.md
```

Empty future-facing directories may use `.gitkeep`. Only the minimal shared core crate is created in WP00.

## Cargo and core architecture

The repository root is a Cargo workspace using resolver 2 or the stable equivalent. Initial members are:

```text
apps/desktop/src-tauri
crates/biforgeworks-core
```

`biforgeworks-core` provides minimal reusable application metadata. The Tauri backend must depend on and call the core crate, with a test proving the integration. Reusable project and domain behavior belongs in Rust core libraries rather than the Tauri shell or React.

## Desktop shell

Create a native Tauri 2 desktop app displaying:

```text
BI Forgeworks
Linux-native analytics engineering
Developer Preview
```

The display name is `BI Forgeworks`, identifier is `com.biforgeworks.desktop`, and version is `0.0.1`. No report editor or Power BI imitation is included.

## Frontend rules

1. React is presentation-layer code, not the canonical business or domain layer.
2. Reusable application logic moves toward Rust core crates.
3. The frontend communicates with Rust through explicit Tauri command interfaces.
4. React state is sufficient for the initial shell; do not add Redux, Zustand, MobX, or similar frameworks.
5. Dependencies must remain minimal and framework-level choices must be justified.
6. TypeScript enables `strict`, `noImplicitAny`, and `noUncheckedIndexedAccess` where practical. Avoid `any` unless justified.

## pnpm workspace and developer commands

Workspace members include `apps/*` and `packages/*`. Root scripts expose:

```bash
pnpm dev
pnpm build
pnpm test
pnpm lint
pnpm typecheck
```

The authoritative validation commands are:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
pnpm install
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

An aggregate `scripts/check.sh` may call these commands but does not replace them.

## Linux development

Document Tauri build prerequisites for Arch/Omarchy and, where straightforward, Debian/Ubuntu. Linux is the priority, without introducing platform-specific build logic that blocks future macOS or Windows support.

## Continuous integration

GitHub Actions runs on pushes to `main` and pull requests. It validates Rust formatting, Clippy with warnings denied, Cargo tests, frozen pnpm installation, frontend linting, type checking, tests, and build. The desktop Rust code must compile sufficiently to catch Tauri integration failures; a full bundle is optional when system dependencies make it unnecessarily heavy.

## Repository workflow

- Default branch: `main`
- WP00 branch: `feature/wp00-repository-bootstrap`
- Preferred prefixes: `feature/`, `fix/`, and `docs/`
- Short-lived branches and pull requests into `main`

## Agent instructions

Root `AGENTS.md` must state:

1. BI Forgeworks is GUI-first but code-native.
2. Rust is authoritative for project and domain behavior.
3. React is the presentation layer, not the canonical semantic layer.
4. Power BI formats are adapters, not the ultimate internal architecture.
5. Future BFIR remains vendor-neutral.
6. Credentials are never committed to project files or fixtures.
7. Unknown valid external constructs should eventually be preserved rather than silently deleted.
8. GUI, CLI, and future agent interfaces converge on shared core commands.
9. Work-package boundaries are authoritative.
10. Future work packages are not implemented opportunistically.
11. Architecture changes require documentation and an ADR where appropriate.
12. Tests belong with implementation.
13. Unnecessary dependencies are avoided.
14. Agents may not silently broaden scope.

Ownership is divided as follows:

- ChatGPT owns architecture, planning, WP specifications, acceptance criteria, and cross-WP design.
- Codex owns repository coordination, integration, review, Git operations, PR preparation, and test verification.
- Claude/Herdr workers may implement bounded assignments, run focused tests, and report findings. They do not independently own architecture, merges, unbounded refactoring, or repository-wide Git integration.

## Architecture documentation

`docs/architecture/overview.md` contains this implemented boundary:

```text
React / TypeScript
       │
       ▼
   Tauri Commands
       │
       ▼
 Rust Core Libraries
```

It may reserve, without implementing, the conceptual layers Project, BFIR, Power BI Adapter, Fabric, Connectors, Runtime, Visuals, and Agents.

## Architecture decisions

Create an ADR process README and only these WP00 decisions:

- ADR-0001: Rust Core / TypeScript Presentation Boundary
- ADR-0002: pnpm as JavaScript Package Manager
- ADR-0003: Monorepo Architecture under `apps/`, `crates/`, and `packages/`

Do not create speculative BFIR or Arrow decisions.

## Documentation

The root README covers the product, current status, goals, stack, repository layout, prerequisites, setup, desktop run commands, validation, build, and current milestone:

> Phase 1: Linux-native PBIP/PBIR/TMDL editing and Power BI/Fabric publishing.

It must make clear that those capabilities do not exist yet. `CONTRIBUTING.md` covers prerequisites, setup, branch naming, testing, formatting, pull requests, and WP-driven development. `docs/work-packages/README.md` explains WP naming.

## License

Use Apache License 2.0 unless project ownership or commercial strategy creates material uncertainty. If so, leave an explicit decision TODO and report it rather than guessing.

## Security and dependency baseline

- Ignore `.env` files and common secret-bearing local variants.
- Commit dependency lockfiles.
- Put no credentials in examples or fixtures.
- Generate no Tauri secrets.
- Expose no broad shell execution from the frontend.
- Add dependencies only when maintained, license-compatible, materially useful, and not replaceable by standard functionality.

Defer DuckDB, Arrow, Mapbox, Vega, D3, Microsoft authentication libraries, PBIR/TMDL parsers, MCP SDKs, and all future analytics libraries.

## Tests and launch validation

The frontend smoke test verifies that the application renders and includes `BI Forgeworks` without startup failure. A reliable Vite/React smoke test is sufficient when Tauri-level E2E is too complex, provided the limitation is documented.

The Tauri backend depends on and calls `biforgeworks-core`; tests must prove the core crate loads and behaves normally. The native Tauri desktop application must also be launched on Linux and checked for startup errors and rendered branding.

## Explicitly out of scope

Do not implement PBIP, PBIR, TMDL, DAX, Power BI authentication, Fabric APIs, workspace browsing, report canvases, model diagrams, BFIR, data connectors, DuckDB, Arrow, Power Query, HTML visuals, Mapbox, Vega, MCP, AI integration, publishing, or functional CLI behavior.

## Acceptance criteria

WP00 is complete when:

- The `BI Forgeworks / biforgeworks` repository exists with default branch `main` and the required monorepo structure.
- The Cargo workspace builds and the Tauri backend uses `biforgeworks-core`.
- `cargo fmt --check`, strict Clippy, and workspace tests pass.
- The React/TypeScript app exists with strict TypeScript and all pnpm lint, typecheck, test, and build commands pass.
- The Tauri 2 application launches on Linux with BI Forgeworks branding and no startup errors.
- CI validates Rust and frontend work on PRs and pushes to `main`.
- README, CONTRIBUTING, AGENTS, architecture overview, ADR process and decisions, and WP documents exist.
- Secrets and credentials are excluded and none are committed.

## Stop condition and handback

After WP00 is merged, stop. Do not begin WP01. The handback reports:

- WP00 status
- GitHub organization, repository, branch, PR, and merge commit
- Delivered foundation and final high-level tree
- Actual Rust, Cargo, Node, pnpm, Tauri, React, TypeScript, and Vite versions
- Every authoritative validation command and result
- Actual Linux desktop validation method
- ADRs and major dependencies with rationale
- Known issues and readiness for WP01
- Explicit confirmation: `No Power BI/PBIP/PBIR/TMDL functionality was implemented.`
