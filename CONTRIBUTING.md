# Contributing to BI Forgeworks

## Prerequisites

- Rust (stable), installed via [rustup](https://rustup.rs); this
  repository pins a toolchain via `rust-toolchain.toml`.
- Node.js 22.12+ and pnpm. Install pnpm with
  `npm install --global pnpm@12.4.1` — this is the dependable path and
  works whether or not Corepack is present. If Corepack is already
  available, `corepack enable && corepack prepare pnpm@12.4.1 --activate`
  is a supported alternative.
- Tauri 2 Linux system dependencies — see the Arch/Omarchy and
  Debian/Ubuntu sections in `README.md`.

## Setup

```bash
git clone https://github.com/BI-Forgeworks/biforgeworks.git
cd biforgeworks
pnpm install --frozen-lockfile
```

`pnpm dev` launches the native Tauri desktop shell. Use `pnpm dev:vite` for
browser-only frontend iteration without a native window.

## Work-package-driven development

Work is organized into numbered work packages under `docs/work-packages/`
(`WPNN-short-description.md`). Each package defines its own scope and
acceptance criteria. Implement only the work package you are assigned;
do not opportunistically start the next one. See `AGENTS.md` for the full
set of working principles, including ownership boundaries between
ChatGPT, Codex, and Claude/Herdr workers.

If a change affects a layer boundary, a foundational tool choice, or a
previously recorded decision, add or update an ADR in `docs/adr/` (see
`docs/adr/README.md` for the process) alongside the code change.

## Branch naming

- Default branch: `main`
- Preferred prefixes: `feature/`, `fix/`, `docs/`
- Branches are short-lived and merged via pull request; name a branch for
  the work package or fix it addresses, e.g. `feature/wp00-repository-bootstrap`.

## Testing

Tests belong with the implementation they cover, in the same change.

- Rust: `cargo test --workspace`. The Tauri backend depends on and calls
  `biforgeworks-core`; changes to that integration need a test proving it.
- Frontend: `pnpm test` (Vitest). The desktop shell smoke test verifies the
  app startup path renders `BI Forgeworks` without a frontend failure while
  mocking the native command bridge. WP00 validates the native Tauri window
  manually rather than through automated end-to-end coverage.

## Formatting and linting

Before opening a pull request, run:

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

`scripts/check.sh` runs these in sequence as a local convenience wrapper;
it does not replace running them directly, and CI runs them independently.

The default-off `safe_write_test_support` feature exposes fault injection and
controlled multi-file staging for tests only. Do not enable it for the desktop
application or expose its methods through Tauri commands. Production operations
must remain explicit, with validation inside `biforgeworks-powerbi`.

## Pull requests

- Keep pull requests scoped to a single work package or fix.
- Describe what changed and why, and reference the relevant work package.
- Ensure CI (Rust and frontend workflows) passes before requesting review.
- Do not commit credentials, secrets, or `.env` files; see the security
  baseline in `.gitignore`.

## Dependencies

Add a new dependency only when it is maintained, license-compatible,
materially useful, and not reasonably replaceable by functionality already
in the standard library or an existing dependency. Avoid unnecessary
dependencies, and avoid global frontend state frameworks (Redux, Zustand,
MobX, or similar) and speculative UI component frameworks per
`docs/work-packages/`.
