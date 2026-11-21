# Changelog

## 0.0.3

- Strip TypeScript types by blanking them in place instead of running the Oxc transformer and codegen, dropping `oxc_transformer`, `oxc_semantic`, `oxc_codegen` and `oxc-browserslist` (106 to 76 crates in the runtime's normal dependency tree).
- Enums, namespaces with values, constructor parameter properties, decorators, `import x = require(...)` and value `import`/`export` now fail in the `parse` phase.
- Early errors such as duplicate `let` bindings are now reported by QuickJS in the `runtime` phase.

## 0.0.1

- Split large implementation modules into focused runtime, tool, schema, policy, and test modules.
- Added crate publishing metadata and an MIT license file.
- Aligned the packaged example with the README host-tool example.
- Added `ExecutionResult` constructors and clearer documentation for utility runtime/invoker types.
- Centralized QuickJS host binding names and split behavior-focused unit tests.
- Split runtime internals further and added registry constructors.
- Named the host-independent contract crate `tack-core`.
- Tightened registry and descriptor construction with private fields, builders, and all-or-nothing extension.
