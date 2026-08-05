# tack-rs

[![CI](https://github.com/shrimp-software/tack-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/shrimp-software/tack-rs/actions/workflows/ci.yml)
[![docs.rs](https://docs.rs/tack-core/badge.svg)](https://docs.rs/tack-core)
[![crates.io](https://img.shields.io/crates/v/tack-core.svg)](https://crates.io/crates/tack-core)

Tool-backed TypeScript execution for Rust applications.

`tack-rs` lets a host expose named tools, then run short TypeScript snippets
that can discover those tools, call them, emit structured events, log progress,
and return JSON. It is useful when one programmatic tool call is too small, but
embedding a full scripting environment with broad ambient access is too much.

## Crates

- [`tack-core`](https://docs.rs/tack-core): host-independent contracts, registries, policies, discovery helpers, and the execution engine.
- `tack-macros`: optional macros for typed tool-set definitions.
- [`tack-runtime-quickjs`](https://docs.rs/tack-runtime-quickjs): a QuickJS + Oxc runtime for executing TypeScript snippets.

## Install

```toml
[dependencies]
tack-core = "0.0.2"
tack-runtime-quickjs = "0.0.2"

# Optional typed-tool macros:
tack-macros = "0.0.2"
```

## Example

```rust
use serde_json::{Value, json};
use tack_core::{ExecutionEngine, ToolCallOutput, ToolCatalog, ToolDescriptor};
use tack_runtime_quickjs::QuickJsRuntime;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut inventory = ToolCatalog::new();
    inventory
        .register_fn(
            ToolDescriptor::new("lookup", "lookup")
            .with_description("Look up inventory by SKU")
            .with_input_schema(json!({
                "type": "object",
                "required": ["sku"],
                "properties": { "sku": { "type": "string" } }
            })),
            |input| async move {
                let sku = input.get("sku").and_then(Value::as_str).unwrap_or("");
                ToolCallOutput::data(json!({ "sku": sku, "inStock": true }))
            },
        )
        .unwrap();

    let mut catalog = ToolCatalog::new();
    catalog.mount("inventory", inventory).unwrap();

    let engine = ExecutionEngine::from_catalog(catalog, QuickJsRuntime);

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

Runnable examples are included in the QuickJS runtime crate:

```bash
cargo run -p tack-runtime-quickjs --example catalog
cargo run -p tack-runtime-quickjs --example dynamic_adapter
cargo run -p tack-runtime-quickjs --example basic
cargo run -p tack-runtime-quickjs --example discovery
cargo run -p tack-runtime-quickjs --example aliases_policy_limits
```

More focused docs:

- [Quickstart](docs/quickstart.md)
- [Tool Catalogs](docs/catalog.md)
- [Tool Macros](docs/macros.md)
- [TypeScript API](docs/typescript-api.md)

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

Lazy calls resolve by dotted path at call time: `tools.inventory.lookup(input)`
calls the host tool registered as `inventory.lookup`. `tools.search(...)` and
`tools.describe.tool(...)` let agents discover and inspect tools on demand, so
hosts do not need to print every full tool schema into context up front.

Tool calls through `tools.*` return envelopes:

```json
{ "ok": true, "data": { "value": 1 } }
```

Configured aliases unwrap successful tool results and throw on failed tool
calls. This makes aliases convenient for concise scripts while preserving
envelopes for lower-level `tools.*` calls.

Tool paths must be valid dotted JavaScript identifiers, such as
`inventory.lookup` or `browser.tabs.list`. Built-in paths under `tools.*` are
reserved and cannot be registered by host registries.

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
bash scripts/release-check.sh
```
