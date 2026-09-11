#!/usr/bin/env bash
#
# Aggregate local validation. This is a convenience wrapper only — it does
# not replace the authoritative commands in CONTRIBUTING.md / CI, and CI
# runs those commands independently rather than invoking this script.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

echo "==> cargo fmt --check"
cargo fmt --check

echo "==> cargo clippy --workspace --all-targets --all-features -- -D warnings"
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> pnpm install --frozen-lockfile"
pnpm install --frozen-lockfile

echo "==> pnpm lint"
pnpm lint

echo "==> pnpm typecheck"
pnpm typecheck

echo "==> pnpm test"
pnpm test

echo "==> pnpm build"
pnpm build

echo "==> all checks passed"
