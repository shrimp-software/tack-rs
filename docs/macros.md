# Tool Macros

`tack-macros` provides an attribute macro for typed tool sets. It is separate
from `tack-core`; generated code uses the public `ToolSet`, `TypedTool`, and
`TypedToolAdapter` APIs.

Use it with `tack-core`'s `schemars` feature:

```toml
[dependencies]
tack-core = { version = "0.0.2", features = ["schemars"] }
tack-macros = "0.0.2"
async-trait = "0.1"
schemars = "1"
serde = { version = "1", features = ["derive"] }
```

```rust
use serde::{Deserialize, Serialize};
use tack_core::ToolResult;
use tack_macros::toolset;

#[derive(Clone)]
struct BrowserTools {
    client: BrowserClient,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct OpenInput {
    url: String,
}

#[derive(Serialize, schemars::JsonSchema)]
struct OpenOutput {
    url: String,
}

#[toolset(namespace = "browser")]
impl BrowserTools {
    #[tool(path = "open", description = "Navigate to a URL")]
    async fn open(&self, input: OpenInput) -> ToolResult<OpenOutput> {
        self.client.open(input.url).await
    }
}
```

The tool set can be mounted directly:

```rust
let mut catalog = tack_core::ToolCatalog::new();
catalog.mount("", BrowserTools { client })?;
```

Macro-generated tool sets clone the receiver into each generated tool wrapper.
Use cheap clone handles such as `Arc`, connection pools, or client structs that
already share state internally.
