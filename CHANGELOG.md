# Changelog

## 0.0.1

- Split large implementation modules into focused runtime, tool, schema, policy, and test modules.
- Added crate publishing metadata and an MIT license file.
- Aligned the packaged example with the README host-tool example.
- Added `ExecutionResult` constructors and clearer documentation for utility runtime/invoker types.
- Centralized QuickJS host binding names and split behavior-focused unit tests.
- Split runtime internals further and added registry constructors.
- Named the host-independent contract crate `tack-core`.
- Tightened registry and descriptor construction with private fields, builders, and all-or-nothing extension.
