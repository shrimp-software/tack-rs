# Releasing

Publish `tack-codemode` first, then `tack-runtime-quickjs`.

Before publishing:

```bash
rg '1\.0\.[0-9]+' README.md Cargo.toml crates/*/Cargo.toml CHANGELOG.md
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo package --workspace --list
cargo publish --dry-run -p tack-codemode
```

Publish the contract crate first:

```bash
cargo publish -p tack-codemode
```

After `tack-codemode` is available on crates.io:

```bash
cargo publish --dry-run -p tack-runtime-quickjs
cargo publish -p tack-runtime-quickjs
```
