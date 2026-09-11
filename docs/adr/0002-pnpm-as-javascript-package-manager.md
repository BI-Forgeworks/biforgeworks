# ADR-0002: pnpm as JavaScript Package Manager

## Status

Accepted

## Context

The repository uses a JavaScript/TypeScript workspace (`apps/desktop`'s
frontend and `packages/ui`) alongside a Cargo workspace. A package manager
choice is needed for installing dependencies, running workspace scripts,
and keeping CI installs fast and reproducible. Candidates were npm, Yarn,
and pnpm.

## Decision

Use pnpm with pnpm workspaces (`pnpm-workspace.yaml`, members `apps/*` and
`packages/*`) as the JavaScript/TypeScript package manager. CI and local
development use `pnpm install --frozen-lockfile` against a committed
`pnpm-lock.yaml`.

## Consequences

- pnpm's content-addressable store and strict dependency resolution reduce
  install time and avoid phantom dependencies (a package used without
  being declared), which matters for a monorepo with multiple workspace
  members.
- Contributors and CI must have pnpm available (via Corepack or a pinned
  install) rather than plain npm/Yarn.
- The lockfile (`pnpm-lock.yaml`) is committed and CI installs are frozen,
  so dependency changes must go through `pnpm install` locally and be
  committed deliberately rather than drifting in CI.
- Root scripts (`pnpm dev`, `pnpm build`, `pnpm test`, `pnpm lint`,
  `pnpm typecheck`) are expected to fan out to workspace members via pnpm's
  workspace-aware script execution.
