# tack-rs

[![CI](https://github.com/shrimp-software/tack-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/shrimp-software/tack-rs/actions/workflows/ci.yml)
[![docs.rs](https://docs.rs/tack-codemode/badge.svg)](https://docs.rs/tack-codemode)
[![crates.io](https://img.shields.io/crates/v/tack-codemode.svg)](https://crates.io/crates/tack-codemode)

Tool-backed TypeScript execution for Rust applications.

`tack-rs` lets a host expose named tools, then run short TypeScript snippets
that can discover those tools, call them, emit structured events, log progress,
and return JSON. It is useful when one programmatic tool call is too small, but
embedding a full scripting environment with broad ambient access is too much.

## Crates

- [`tack-codemode`](https://docs.rs/tack-codemode): host-independent contracts, registries, policies, discovery helpers, and the execution engine.
- [`tack-runtime-quickjs`](https://docs.rs/tack-runtime-quickjs): a QuickJS + Oxc runtime for executing TypeScript snippets.

## Install

```toml
[dependencies]
tack-codemode = "1.0.5"
tack-runtime-quickjs = "1.0.5"
```

## Example

```rust
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tack_codemode::{
    ExecutionEngine, ToolCall, ToolCallOutput, ToolDescriptor, ToolInvoker, ToolRegistry,
};
use tack_runtime_quickjs::QuickJsRuntime;

struct HostTools;

#[async_trait]
impl ToolInvoker for HostTools {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        match call.path.as_str() {
            "inventory.lookup" => {
                let sku = call.input.get("sku").and_then(Value::as_str).unwrap_or("");
                ToolCallOutput::data(json!({ "sku": sku, "inStock": true }))
            }
            _ => ToolCallOutput::failure(format!("Unknown tool: {}", call.path)),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let registry = ToolRegistry {
        tools: vec![ToolDescriptor::new("inventory.lookup", "lookup")],
    };
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(HostTools));

    let output = engine
        .execute(
            r#"
            const item = await tools.inventory.lookup({ sku: "abc-123" });
            emit({ checked: item.data.sku });
            return item.data.inStock;
            "#,
        )
        .await;

    assert!(output.ok);
    assert_eq!(output.result, Some(json!(true)));
    assert_eq!(output.emitted, vec![json!({ "checked": "abc-123" })]);
}
```

## Execution Model

Each execution runs a TypeScript function body in a fresh QuickJS context. Oxc
strips TypeScript syntax before evaluation. The snippet receives a small global
API:

- `tools.search({ query, limit })`
- `tools.describe.tool({ path })`
- lazy calls such as `tools.inventory.lookup(input)`
- explicit calls such as `tools.invoke("inventory.lookup", input)`
- configured aliases such as `inventory.lookup(input)`
- `emit(value)`
- `console.log(...values)`

Tool calls through `tools.*` return envelopes:

```json
{ "ok": true, "data": { "value": 1 } }
```

Configured aliases unwrap successful tool results and throw on failed tool
calls. This makes aliases convenient for concise scripts while preserving
envelopes for lower-level `tools.*` calls.

Tool paths must be valid dotted JavaScript identifiers, such as
`inventory.lookup` or `browser.tabs.list`. Built-in paths under `tools.*` are
reserved and cannot be registered by host manifests.

## Serialization

Returned values, emitted values, logged object values, and tool inputs use
JavaScript `JSON.stringify` semantics.

- top-level `undefined` returns `null`
- `BigInt` and cyclic return values fail execution with a runtime error
- cyclic or non-serializable tool inputs return a failed tool envelope without calling the host
- use `tools.invoke(path, input)` for paths with JavaScript proxy edge-case segments such as `then`

## Limits

Default runtime limits:

- execution timeout: 30 seconds
- memory: 128 MiB
- stack: 1,000,000 bytes
- output payload: 1,000,000 bytes
- tool request payload: 1,000,000 bytes
- tool response payload: 1,000,000 bytes
- tool calls: 100
- per-tool-call timeout: 30 seconds

All configured limits must be greater than zero.

The QuickJS runtime does not install ambient `fetch`, `process`, `require`,
filesystem, or module-loader access. Tool calls are limited to paths declared in
the registry after policy filtering, including direct `QuickJsRuntime` use. Logs
and emitted values count against the output byte limit while code is running,
then the exact serialized response is checked again before returning.

Tool request size is checked in JavaScript immediately after
`JSON.stringify(input)`, then checked again in Rust by encoded byte length before
the host invoker is called. The JavaScript-side check avoids host calls for
oversized requests but does not remove the temporary allocation required to
serialize arbitrary JavaScript values; the QuickJS memory limit bounds that
allocation.

Tool-call timeouts cancel the awaited Rust future. Host invokers should be
cancellation-safe and avoid detached blocking work if they need hard cleanup
guarantees.

## Security

`tack-rs` is designed to reduce ambient authority:

- each execution uses a fresh QuickJS context
- no browser, Node, filesystem, or module globals are installed
- tool paths must be declared and can be filtered by policy
- CPU, memory, stack, output, request, response, and tool-call limits are enforced

The host application is still responsible for authorization, audit logging,
input validation at tool boundaries, and process or container isolation when
running untrusted snippets.

## Verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo package --workspace
```

## Publishing

See [RELEASING.md](RELEASING.md).
