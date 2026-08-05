# Quickstart

This guide shows the host-side pieces needed to run a TypeScript snippet with
tools exposed from Rust.

## Install

```toml
[dependencies]
tack-core = "0.0.2"
tack-runtime-quickjs = "0.0.2"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt"] }
```

## Build a Catalog

Register each host operation once. The catalog provides both discovery metadata
and invocation dispatch:

```rust
use serde_json::{Value, json};
use tack_core::{ToolCallOutput, ToolCatalog, ToolDescriptor};

let mut inventory = ToolCatalog::new();
inventory.register_fn(
    ToolDescriptor::new("lookup", "lookup")
        .with_description("Look up inventory by SKU")
        .with_input_schema(json!({
            "type": "object",
            "required": ["sku"],
            "properties": {
                "sku": { "type": "string" }
            }
        })),
    |input| async move {
        let sku = input.get("sku").and_then(Value::as_str).unwrap_or("");
        ToolCallOutput::data(json!({ "sku": sku, "inStock": true }))
    },
)?;

let mut catalog = ToolCatalog::new();
catalog.mount("inventory", inventory)?;
```

Tool paths must be dotted JavaScript identifiers, such as `inventory.lookup` or
`browser.tabs.list`. Paths below `tools.*` are reserved for built-ins.

## Execute Code

Create an `ExecutionEngine` with a runtime and invoke it with a TypeScript
function body:

```rust
use tack_core::ExecutionEngine;
use tack_runtime_quickjs::QuickJsRuntime;

let engine = ExecutionEngine::from_catalog(catalog, QuickJsRuntime);
let output = engine
    .execute(r#"
        const item = await tools.inventory.lookup({ sku: "abc-123" });
        emit({ checked: item.data.sku });
        return item.data.inStock;
    "#)
    .await;

assert!(output.ok);
```

Run the included examples from the workspace root:

```bash
cargo run -p tack-runtime-quickjs --example catalog
cargo run -p tack-runtime-quickjs --example basic
cargo run -p tack-runtime-quickjs --example discovery
cargo run -p tack-runtime-quickjs --example aliases_policy_limits
```
