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
                    "properties": {
                        "sku": { "type": "string" }
                    }
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

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
