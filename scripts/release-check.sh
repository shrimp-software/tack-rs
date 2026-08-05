#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --check
cargo check --workspace --all-features
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps

# Before tack-core 0.0.2 exists in crates.io, dependent workspace package
# verification cannot resolve. Verify the first crate fully and the workspace
# archive shape without dependency verification.
cargo package -p tack-core --allow-dirty
cargo package --workspace --allow-dirty --no-verify

cat <<'MSG'

Release check passed.

Publish order:
  1. cargo publish -p tack-core
  2. cargo publish -p tack-macros
  3. cargo publish -p tack-runtime-quickjs
MSG
