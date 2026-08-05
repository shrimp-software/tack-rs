# Tool Catalogs

`ToolCatalog` is the preferred host API when the same Rust code owns tool
metadata and execution. The catalog produces both the discovery registry and
the invoker that dispatches calls, so descriptors and implementation cannot
drift.

Descriptors can include annotations that help hosts and agents reason about
tool behavior:

```rust
use tack_core::{ToolAnnotations, ToolDescriptor};

let descriptor = ToolDescriptor::new("browser.storage.clear", "clear")
    .with_description("Clear browser storage")
    .with_annotations(
        ToolAnnotations::new()
            .destructive(true)
            .read_only(false)
            .capability("browser.storage"),
    );
```

`read_only` means the tool is intended to observe state without changing it.
`destructive` means the tool can delete, overwrite, clear, spend, submit, or
otherwise make changes that are hard to undo. `idempotent` means repeated calls
with the same input are expected to have the same effect as one call. These are
host-provided hints for discovery, filtering, and agent planning; hosts must
still enforce authorization at the tool boundary.

## Function Tools

Use `register_fn` when a tool naturally accepts and returns JSON values:

```rust
use serde_json::{Value, json};
use tack_core::{ToolCallOutput, ToolCatalog, ToolDescriptor};

let mut catalog = ToolCatalog::new();
catalog.register_fn(
    ToolDescriptor::new("inventory.lookup", "lookup")
        .with_description("Look up inventory by SKU"),
    |input| async move {
        let sku = input.get("sku").and_then(Value::as_str).unwrap_or("");
        ToolCallOutput::data(json!({ "sku": sku, "inStock": true }))
    },
)?;
```

Use `register_call_fn` when an adapter needs the resolved tool path as well as
the input:

```rust
catalog.register_call_fn(
    ToolDescriptor::new("proxy.call", "call"),
    |call| async move {
        ToolCallOutput::data(json!({
            "path": call.path,
            "input": call.input
        }))
    },
)?;
```

Build the engine directly from the catalog:

```rust
use tack_core::ExecutionEngine;
use tack_runtime_quickjs::QuickJsRuntime;

let engine = ExecutionEngine::from_catalog(catalog, QuickJsRuntime);
```

## Mounting Tool Sets

Use `mount` to expose a reusable group under a namespace. Relative paths inside
the mounted set are prefixed:

```rust
let mut browser = ToolCatalog::new();
browser.register_fn(ToolDescriptor::new("open", "open"), |input| async move {
    ToolCallOutput::data(input)
})?;

let mut catalog = ToolCatalog::new();
catalog.mount("eoka", browser)?;

assert!(catalog.registry().contains_path("eoka.open"));
```

## Typed Tools

Enable the `schemars` feature to generate input and output schemas from Rust
types:

```toml
[dependencies]
tack-core = { version = "0.0.2", features = ["schemars"] }
schemars = "1"
serde = { version = "1", features = ["derive"] }
```

```rust
use serde::{Deserialize, Serialize};
use tack_core::{ToolCatalog, ToolDescriptor, ToolResult};

#[derive(Deserialize, schemars::JsonSchema)]
struct LookupInput {
    sku: String,
}

#[derive(Serialize, schemars::JsonSchema)]
struct LookupOutput {
    sku: String,
    in_stock: bool,
}

let mut catalog = ToolCatalog::new();
catalog.register_typed_fn(
    ToolDescriptor::new("inventory.lookup", "lookup")
        .with_description("Look up inventory by SKU"),
    |input: LookupInput| async move {
        Ok::<_, tack_core::ToolCallError>(LookupOutput {
            sku: input.sku,
            in_stock: true,
        })
    },
)?;
```

Typed tools return `invalid_input` when JSON input cannot be deserialized and
`invalid_output` when output cannot be serialized.

Use `register_typed_fn` for compact closures. Use the `TypedTool` trait when a
tool needs its own struct, internal helpers, or a clearer implementation
boundary.

## Middleware

Catalog layers wrap invocation without changing runtime internals:

```rust
let catalog = ToolCatalog::new()
    .with_layer(TracingLayer)
    .with_layer(AuthzLayer::new(authz));
```

Layers run in registration order: in the example above `TracingLayer` sees the
call first and the response last.

Layers are the extension point for cross-cutting behavior such as tracing,
authorization, metrics, redaction, retry wrappers, and per-tool timeout
wrappers. A layer may pass the call through, rewrite it, observe the output, or
short-circuit with a failed `ToolCallOutput`.

## Policies

Path-only policies still work through `ToolPolicy::is_allowed`. Descriptor-aware
policies can override `ToolPolicy::is_descriptor_allowed` or use
`DescriptorPolicy`.

```rust
use tack_core::{DescriptorPolicy, ToolDescriptor};

let policy = DescriptorPolicy::new(|descriptor: &ToolDescriptor| {
    descriptor.annotations().read_only == Some(true)
});
```

Engine-level policy filtering hides denied descriptors from discovery and from
the execution registry. Path-only policy checks also remain in the invoker path
for compatibility with existing `PolicyAwareInvoker` usage.

## Low-Level API

`ToolRegistry` and `ToolInvoker` remain available for hosts that need fully
custom dispatch, external registries, or generated descriptors. Runtimes still
consume that low-level pair internally.
