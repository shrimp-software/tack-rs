# tack-rs

Rust crates for tool-backed TypeScript execution. A host application exposes a
registry of named tools, then `tack-rs` runs short TypeScript snippets that can
discover those tools, call them, emit structured values, and return JSON.

This workspace contains:

- `tack-codemode`: host-independent contracts, tool registry types, built-in discovery helpers, policy hooks, and the execution engine.
- `tack-runtime-quickjs`: a QuickJS + Oxc runtime for executing TypeScript snippets.

The repository is ready for private/internal v1 usage. Publishing to crates.io
still requires an explicit release decision.

## When To Use It

Use `tack-rs` when you want a Rust host application to expose a constrained set
of tools to short-lived TypeScript snippets:

- CLI automation that needs richer control flow than shell one-liners
- MCP-style tool discovery and invocation
- host-controlled browser, API, database, or local workflow automation
- internal agents where the host owns the tool registry and policy layer

In practice, this is the Rust runtime/library layer behind Tack's "Code Mode":
instead of asking an agent to issue one tool call at a time, the agent can write
a small program that searches available tools, calls them in loops or batches,
combines their responses, and returns one structured result.

Do not treat `tack-runtime-quickjs` as the only security boundary for hostile
public multi-tenant code. It removes ambient browser/Node capabilities and
enforces runtime limits, but production systems should still run it behind normal
process, container, identity, logging, and abuse controls.

## Install

```toml
[dependencies]
tack-codemode = "1.0.4"
tack-runtime-quickjs = "1.0.4"
```

For local development before crates.io publishing:

```toml
[dependencies]
tack-codemode = { path = "../tack-rs/crates/tack-codemode" }
tack-runtime-quickjs = { path = "../tack-rs/crates/tack-runtime-quickjs" }
```

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

## Execution Contract

The runtime executes a TypeScript function body in a fresh QuickJS context. The
host provides a `ToolRegistry` and a `ToolInvoker`; the snippet gets:

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

Tool paths must be valid dotted JavaScript identifiers such as `eoka.eval` or
`browser.tabs.list`. Built-in paths under `tools.*` are reserved and cannot be
registered by a host manifest.

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

## Security Model

The runtime is designed to reduce ambient authority:

- no `fetch`, `process`, `require`, filesystem, or module loader is installed
- each execution uses a fresh QuickJS context
- tool calls must resolve to declared registry paths
- policies can filter tools before discovery and invocation
- CPU, memory, stack, output, request, response, and tool-call limits are enforced

The host is still responsible for:

- deciding which tools are exposed
- validating tool input at the tool boundary
- enforcing authorization and audit logging
- isolating execution at the process/container level when snippets are untrusted

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
- verify parity against the Tack tool-execution v1 fixture
- confirm semver version bumps in both crates
- publish `tack-codemode` first, then `tack-runtime-quickjs`
- do not publish from this local checkout without explicit authorization
