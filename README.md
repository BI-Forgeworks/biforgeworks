# BI Forgeworks

BI Forgeworks is intended to become a Linux-native, GUI-first, code-native
analytics development environment.

## Current status

**Read-only project discovery (WP01).** The Linux desktop can select a `.pbip`,
resolve its report and local semantic-model references, identify PBIR,
PBIR-Legacy, TMDL, and TMSL storage markers, and display structural diagnostics.
It does not edit or save projects, parse report/model semantics, authenticate,
or publish. Format identification is not semantic validation.

## Current milestone

> Phase 1: Linux-native PBIP/PBIR/TMDL editing and Power BI/Fabric
> publishing.

Editing and publishing remain future work. See
`docs/work-packages/` for how work is sequenced toward this milestone.

## Goals

- Open, inspect, safely edit, save, and publish Power BI PBIP/PBIR/TMDL
  projects, natively on Linux.
- Keep Rust authoritative for project and domain behavior, with React as a
  presentation layer only (see `docs/architecture/overview.md`).
- Grow deliberately, one work package at a time, with architecture
  decisions recorded as ADRs (`docs/adr/`).

Longer-term, possible future layers include a portable BI intermediate
representation, data connectors, a local analytical runtime, HTML/SVG and
geospatial visuals, and agent interfaces. These are not implemented and are
not committed to; see `docs/architecture/overview.md` for how they are
reserved without being built.

## Stack

- Rust (stable), `rustfmt`, Clippy, `cargo test`
- Tauri 2 with a Rust backend
- React, strict TypeScript, Vite
- pnpm workspaces
- Vitest for frontend unit tests, with a lightweight smoke test for the
  desktop shell

## Repository layout

```text
biforgeworks/
├── apps/
│   └── desktop/          # Tauri 2 desktop shell + React frontend
│       ├── src/
│       ├── src-tauri/
│       └── package.json
├── crates/
│   ├── biforgeworks-core/ # Shared Rust core library
│   └── biforgeworks-powerbi/ # Read-only project discovery and diagnostics
├── packages/
│   └── ui/                # Shared React/TypeScript UI package
├── connectors/            # Future data connectors (empty in WP00)
├── targets/                # Future publish targets (empty in WP00)
├── fixtures/powerbi/       # Minimal synthetic discovery fixtures
├── docs/
│   ├── architecture/       # Architecture overview
│   ├── adr/                 # Architecture decision records
│   ├── research/            # Research notes (empty in WP00)
│   └── work-packages/       # Work package definitions
├── scripts/                 # Developer/CI helper scripts
├── .github/workflows/       # CI
├── Cargo.toml / Cargo.lock
├── pnpm-workspace.yaml / package.json
├── rust-toolchain.toml
├── AGENTS.md / CONTRIBUTING.md / LICENSE
└── README.md
```

## Prerequisites

Install Rust via [rustup](https://rustup.rs) and Node.js 22.12+, then install
pnpm. The dependable path, which works regardless of whether Corepack is
present on your Node install, is:

```bash
npm install --global pnpm@12.4.1
```

If Corepack is already available on your system, it is a supported
alternative:

```bash
corepack enable
corepack prepare pnpm@12.4.1 --activate
```

### Tauri system dependencies — Arch / Omarchy

```bash
sudo pacman -S --needed webkit2gtk-4.1 gtk3 libayatana-appindicator \
  librsvg base-devel curl wget file openssl
```

### Tauri system dependencies — Debian / Ubuntu

```bash
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev \
  libssl-dev patchelf build-essential curl wget file
```

Package names track the current Tauri 2 Linux requirements at the time of
writing; consult the
[Tauri prerequisites documentation](https://tauri.app/start/prerequisites/)
if a package is missing on your distribution or version.

Linux is the development priority. Nothing here introduces
platform-specific build logic intended to block future macOS or Windows
support.

## Setup

```bash
git clone https://github.com/BI-Forgeworks/biforgeworks.git
cd biforgeworks
pnpm install --frozen-lockfile
```

## Running the desktop shell

Launch the native Tauri desktop shell from the repository root:

```bash
pnpm dev
```

The repository pins the Tauri CLI as a development dependency. Tauri starts
the Vite server and opens the native window. Choose **Open Power BI Project**
and select a `.pbip`. To try a synthetic example, select the file inside:

```text
fixtures/powerbi/valid-pbir-tmdl/
```

For browser-only frontend iteration without a native window, run:

```bash
pnpm dev:vite
```

## Validation

These are the authoritative validation commands; `scripts/check.sh` runs
them locally as a convenience wrapper but does not replace them:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
pnpm install --frozen-lockfile
pnpm lint
pnpm typecheck
pnpm test
```

Rust coverage exercises discovery, malformed inputs, path confinement, and
read-only preservation. Vitest covers project rendering, diagnostics, errors,
and cancellation with the native bridge mocked. Native Linux selection/open
is validated separately; Vitest is not native-window E2E automation.

See [discovery architecture](docs/architecture/powerbi-project-discovery.md)
for supported metadata, security limits, and timestamp-preserving Linux reads.
Remote model connections and references outside the selected project root are
diagnosed without following them.

## Build

```bash
pnpm build
```

CI (`.github/workflows/`) runs Rust formatting, Clippy with warnings
denied, Cargo tests, a frozen pnpm install, and frontend linting,
type-checking, tests, and build on every push to `main` and every pull
request.

## Contributing

See `CONTRIBUTING.md` for setup, branch naming, testing, formatting, and
the pull request process. See `AGENTS.md` for agent-specific working rules
and `docs/work-packages/` for how work is scoped and sequenced.

## License

Apache License 2.0 — see `LICENSE`.
