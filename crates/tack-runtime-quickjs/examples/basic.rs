use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tack_core::{
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

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
