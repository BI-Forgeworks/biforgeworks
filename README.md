# BI Forgeworks

BI Forgeworks is intended to become a Linux-native, GUI-first, code-native
analytics development environment.

## Current status

**Read-only TMDL semantic inspection (WP03).** Open a `.pbip` on Linux to
inspect project references, report/model formats, and structural diagnostics.
For TMDL models, browse tables, columns, measures, hierarchies, partitions,
relationships, roles, perspectives, cultures, named expressions, and functions.
DAX and Power Query M are displayed as opaque source text. TMSL discovery works;
TMSL semantic inspection is not supported.

The desktop has no editing or saving controls. WP02's Rust transaction foundation
remains available internally, with no frontend filesystem write authority.
The reader retains source spans and unsupported syntax; it does not establish
complete TOM validation or Power BI Desktop compatibility.

## Current milestone

> Build a usable Linux-native Power BI viewer/editor first.

The near-term sequence is TMDL reading/editing, DAX editing UX, a model diagram,
PBIR reading, report canvas, basic visual editing, and round-trip hardening.
Fabric publishing, connectors, BFIR, alternative exports, visuals frameworks,
and agent integration are deferred. WP04 has not begun. An approved real
Desktop-created TMDL PBIP must be validated before production semantic writes.

## Goals

- Open, inspect, and eventually safely edit Power BI projects on Linux.
- Keep Rust authoritative for project/domain behavior and React for presentation.
- Deliver one reviewed work package at a time, recording architecture decisions.

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
│   └── biforgeworks-powerbi/ # Discovery, TMDL reader, snapshots, safe transactions
├── packages/
│   └── ui/                # Shared React/TypeScript UI package
├── connectors/            # Future data connectors (empty in WP00)
├── targets/                # Future publish targets (empty in WP00)
├── fixtures/powerbi/       # Synthetic discovery, preservation, and TMDL fixtures
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
fixtures/powerbi/tmdl-star-schema/Sales.pbip
```

For browser-only frontend iteration without a native window, run:

```bash
pnpm dev:vite
```

## Validation

These are the authoritative validation commands; `scripts/check.sh` runs
them locally as a convenience wrapper but does not replace them:

```bash
cargo check --workspace --locked
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test -p biforgeworks-powerbi --features safe_write_test_support
pnpm install --frozen-lockfile
pnpm lint
pnpm typecheck
pnpm test
pnpm build
git diff --check
```

Rust coverage exercises discovery, malformed inputs, path confinement,
preservation, explicit metadata edits, external conflicts, failure recovery,
and source-aware TMDL parsing and reference resolution.
The default-off `safe_write_test_support` feature adds test-only staging and
fault injection; it is never enabled for desktop production builds.
Vitest covers project rendering, diagnostics, errors,
and cancellation with the native bridge mocked. Native Linux selection/open
is validated separately; Vitest is not native-window E2E automation.

See [discovery architecture](docs/architecture/powerbi-project-discovery.md)
for supported metadata, security limits, and timestamp-preserving Linux reads.
Remote model connections and references outside the selected project root are
diagnosed without following them.

See [safe-write architecture](docs/architecture/powerbi-safe-writes.md) for the
transaction API, snapshot limits, atomicity and rollback guarantees, and manual
recovery instructions. Synthetic fixtures do not establish Power BI Desktop
round-trip compatibility.

See [TMDL reader architecture](docs/architecture/tmdl-reader.md) for grammar
coverage, source preservation, limits, and compatibility gaps.

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
