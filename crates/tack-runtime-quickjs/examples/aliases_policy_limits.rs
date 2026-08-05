use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tack_core::{
    DenyListPolicy, ExecutionEngine, RuntimeLimits, ToolCall, ToolCallOutput, ToolDescriptor,
    ToolInvoker, ToolRegistry,
};
use tack_runtime_quickjs::QuickJsRuntime;

struct HostTools;

#[async_trait]
impl ToolInvoker for HostTools {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        match call.path.as_str() {
            "inventory.lookup" => {
                let sku = call.input.get("sku").and_then(Value::as_str).unwrap_or("");
                ToolCallOutput::data(json!({ "sku": sku, "quantity": 4 }))
            }
            "inventory.delete" => ToolCallOutput::data(json!({ "deleted": true })),
            _ => ToolCallOutput::failure(format!("unknown tool: {}", call.path)),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let registry = ToolRegistry::from_tools(vec![
        ToolDescriptor::new("inventory.lookup", "lookup")
            .with_description("Look up inventory by SKU"),
        ToolDescriptor::new("inventory.delete", "delete")
            .with_description("Delete inventory by SKU"),
    ])
    .unwrap();

    let limits = RuntimeLimits {
        max_tool_calls: 3,
        timeout_ms: 5_000,
        ..RuntimeLimits::default()
    };

    let policy = DenyListPolicy::new(vec!["inventory.delete".to_string()]);
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(HostTools))
        .with_alias("inventory", "inventory")
        .with_limits(limits)
        .with_policy(Arc::new(policy));

    let output = engine
        .execute(
            r#"
            // Aliases unwrap successful data and throw if the tool fails.
            const item = await inventory.lookup({ sku: "kb-001" });
            emit({ quantity: item.quantity });

            // Policy-filtered tools are removed from the execution registry.
            const denied = await tools.inventory.delete({ sku: "kb-001" });

            return { denied: !denied.ok, code: denied.error.code };
            "#,
        )
        .await;

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
