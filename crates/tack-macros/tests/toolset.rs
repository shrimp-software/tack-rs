use serde::{Deserialize, Serialize};
use serde_json::json;
use tack_core::{ToolCall, ToolInvoker, ToolResult};
use tack_macros::toolset;

#[derive(Clone)]
struct BrowserTools;

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
    #[tool(path = "open", description = "Open a URL")]
    async fn open(&self, input: OpenInput) -> ToolResult<OpenOutput> {
        Ok(OpenOutput { url: input.url })
    }
}

#[tokio::test]
async fn toolset_macro_generates_typed_tool_set() {
    let mut catalog = tack_core::ToolCatalog::new();
    catalog.mount("", BrowserTools).unwrap();

    let descriptor = catalog.registry().by_path("browser.open").cloned().unwrap();
    assert_eq!(descriptor.description(), Some("Open a URL"));
    assert_eq!(descriptor.input_schema()["type"], "object");

    let output = catalog
        .call(ToolCall {
            path: "browser.open".to_string(),
            input: json!({ "url": "https://example.com" }),
        })
        .await;

    assert_eq!(output.data, Some(json!({ "url": "https://example.com" })));
}
