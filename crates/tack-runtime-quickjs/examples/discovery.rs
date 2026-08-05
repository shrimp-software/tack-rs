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
                ToolCallOutput::data(json!({
                    "sku": sku,
                    "name": "Keyboard",
                    "quantity": 17
                }))
            }
            "inventory.reserve" => {
                let sku = call.input.get("sku").and_then(Value::as_str).unwrap_or("");
                let quantity = call
                    .input
                    .get("quantity")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                ToolCallOutput::data(json!({
                    "reservationId": "res-1001",
                    "sku": sku,
                    "quantity": quantity
                }))
            }
            _ => ToolCallOutput::failure(format!("unknown tool: {}", call.path)),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let registry = ToolRegistry::from_tools(vec![
        ToolDescriptor::new("inventory.lookup", "lookup")
            .with_title("Inventory lookup")
            .with_description("Look up current inventory for a SKU")
            .with_input_schema(json!({
                "type": "object",
                "required": ["sku"],
                "properties": {
                    "sku": { "type": "string" }
                }
            }))
            .with_output_schema(json!({
                "type": "object",
                "required": ["sku", "name", "quantity"],
                "properties": {
                    "sku": { "type": "string" },
                    "name": { "type": "string" },
                    "quantity": { "type": "integer" }
                }
            })),
        ToolDescriptor::new("inventory.reserve", "reserve")
            .with_title("Reserve inventory")
            .with_description("Reserve a quantity for a SKU")
            .with_input_schema(json!({
                "type": "object",
                "required": ["sku", "quantity"],
                "properties": {
                    "sku": { "type": "string" },
                    "quantity": { "type": "integer", "minimum": 1 }
                }
            })),
    ])
    .unwrap();

    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(HostTools));
    let output = engine
        .execute(
            r#"
            const matches = await tools.search({ query: "inventory", limit: 5 });
            emit({ discovered: matches.data.map((tool) => tool.path) });

            const lookup = await tools.describe.tool({ path: "inventory.lookup" });
            console.log(lookup.data.typeSignature);

            const item = await tools.inventory.lookup({ sku: "kb-001" });
            if (!item.ok || item.data.quantity < 1) {
                return { reserved: false };
            }

            const reservation = await tools.invoke("inventory.reserve", {
                sku: item.data.sku,
                quantity: 2,
            });

            return {
                reserved: reservation.ok,
                reservationId: reservation.data.reservationId,
            };
            "#,
        )
        .await;

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
