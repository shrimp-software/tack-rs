# Changelog

## 1.0.5

- Split large implementation modules into focused runtime, tool, schema, policy, and test modules.
- Added crate publishing metadata and repository license files.
- Aligned the packaged example with the README host-tool example.
- Added `ExecutionResult` constructors and clearer documentation for utility runtime/invoker types.

## 1.0.4

- Hardened registry validation for duplicate, invalid, and reserved tool paths.
- Preserved captured logs and emitted values on runtime failures and timeouts.
- Improved JSON Schema to TypeScript signatures for discovery output.
- Moved the QuickJS bootstrap body into an embedded JavaScript source file.
