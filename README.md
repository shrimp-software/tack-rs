# tack-rs

Rust crates for Tack Code Mode.

This workspace contains:

- `tack-codemode`: host-independent Code Mode contracts, tool registry types, built-in discovery helpers, policy hooks, and the execution engine.
- `tack-runtime-quickjs`: a QuickJS + Oxc runtime for executing TypeScript Code Mode snippets.

The repository is prepared for internal release validation, but publishing still requires an explicit release decision.

## Example

```rust
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tack_codemode::{ExecutionEngine, MemoryInvoker, ToolDescriptor, ToolRegistry};
use tack_runtime_quickjs::QuickJsRuntime;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let registry = ToolRegistry {
        tools: vec![ToolDescriptor::new("example.echo", "echo")],
    };

    let invoker = MemoryInvoker::new(HashMap::from([(
        "example.echo".to_string(),
        json!({"message":"hello"}),
    )]));

    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(invoker));
    let result = engine
        .execute("return (await tools.example.echo({})).data.message;")
        .await;

    assert_eq!(result.result, Some(json!("hello")));
}
```

## Contract

Code Mode executes a TypeScript function body in a fresh QuickJS context. The runtime exposes:

- `tools.search({ query, limit })`
- `tools.describe.tool({ path })`
- lazy tool calls such as `tools.example.echo(input)`
- explicit path calls such as `tools.invoke("example.echo", input)`
- host aliases such as `eoka.*` when configured
- `emit(value)`
- `console.log(value)`

`tools.*` calls return Tack envelopes:

```json
{ "ok": true, "data": { "value": 1 } }
```

Aliases unwrap successful data and throw failed tool calls.

Return values, `emit` values, `console.log` object values, and tool-call inputs are serialized with JavaScript `JSON.stringify` semantics. Unsupported top-level return values such as `undefined` become `null`; `BigInt` and cyclic return values fail the execution with a runtime error. Cyclic or otherwise non-serializable tool inputs return a normal failed Tack envelope instead of calling the host tool. Use `tools.invoke(path, input)` for tool paths that contain JavaScript proxy edge-case segments such as `then`.

## Limits

Defaults:

- timeout: 30 seconds
- memory: 128 MiB
- stack: 1,000,000 bytes
- output/request/response payload: 1,000,000 bytes
- tool calls: 100
- per-tool-call timeout: 30 seconds

All configured limits must be greater than zero. Ambient `fetch`, `process`, `require`, filesystem, and module access are not provided by the runtime. Tool calls are limited to paths declared in the registry after policy filtering, including direct `QuickJsRuntime` use. Logs and emitted values are counted against the output byte limit while code is running, then the exact serialized response is checked again before returning.

Tool request size is checked in JavaScript immediately after `JSON.stringify(input)`, then checked again in Rust by encoded byte length before the host invoker is called. The JavaScript-side check avoids host calls for oversized requests but does not remove the temporary allocation required to serialize arbitrary JS values; the QuickJS memory limit bounds that allocation.

Tool-call timeouts cancel the awaited Rust future. Host invokers should be cancellation-safe and avoid detached blocking work if they need hard cleanup guarantees.

## Verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo package --workspace
```

## Release Checklist

Before publishing:

- run the verification commands above on Rust 1.94
- verify parity against the Tack Code Mode v1 fixture
- confirm semver version bumps in both crates
- publish `tack-codemode` first, then `tack-runtime-quickjs`
- do not publish from this local checkout without explicit authorization
