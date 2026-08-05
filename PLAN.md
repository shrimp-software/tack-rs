# Eoka Tool Catalog Completion Audit

## Implemented

- `eoka-protocol` now owns the operation catalog as the source of truth.
- The protocol operation macro emits:
  - `Request`
  - `OperationId`
  - `OperationSpec`
  - `OPERATIONS`
  - operation lookup by path and command
  - request construction from path or command
  - zero-argument validation
  - input schema lookup
  - manifest projection
  - request command and args extraction
- `eoka tools manifest --json` is generated from protocol catalog metadata.
- `eoka-tack` builds Tack descriptors from the same protocol projection.
- CLI daemon-backed command coverage is tested against the protocol catalog.
- MCP reuses compatible protocol input structs for shared operations and has a
  coverage test for overlapping protocol commands.
- Shared protocol input schemas keep MCP-friendly field descriptions for the
  reused request types, so moving ownership to `eoka-protocol` does not reduce
  generated schema quality.
- New split Eoka crates include crate readmes and docs metadata for publishable
  crates.io entries.
- Tack crates are versioned for the new catalog API as `0.0.2`.
- `eoka-tools` normal dependencies point at `tack-core = 0.0.2` and
  `tack-runtime-quickjs = 0.0.2` with no checked-in sibling checkout override.
- Pre-publish local verification can still use an isolated temporary Cargo
  config with local Tack patch overrides without changing committed Eoka
  manifests.
- Publish workflows encode the required dependency order for Tack and Eoka
  crates.
- `scripts/release-check.sh` in each repository provides repeatable release
  verification, with `eoka-tools` supporting `--with-local-tack` for
  pre-publish integration checks.

## Published

- `tack-core 0.0.2`
- `tack-macros 0.0.2`
- `tack-runtime-quickjs 0.0.2`
- `eoka-protocol 0.1.0`
- `eoka-sdk 0.1.0`
- `eoka-mcp 0.1.1`
- `eoka-tack 0.1.0`
- `eoka-cli 0.1.15`

## Verified

- `cargo check --workspace --all-features` passed in `tack-rs`.
- `cargo test --workspace --all-features` passed in `tack-rs`.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  passed in `tack-rs`.
- `cargo package --workspace --allow-dirty --no-verify` passed in `tack-rs`.
- `cargo package -p tack-core --allow-dirty` passed in `tack-rs`.
- `bash scripts/release-check.sh` passed in `tack-rs`.
- `rg` found no checked-in Eoka dependency or `[patch.crates-io]` entry pointing
  at the sibling `../tack-rs` checkout.
- `bash scripts/release-check.sh` passed in `eoka-tools` using registry-sourced
  Tack `0.0.2` dependencies.
- `cargo package --workspace --allow-dirty` passed in `eoka-tools` after all
  newly published crates became visible in crates.io.
- Eoka publish workflow metadata extraction resolves the intended package names
  and versions in dependency order.
- Crates.io visibility was verified with `cargo info` for every published Tack
  and Eoka crate listed above.
