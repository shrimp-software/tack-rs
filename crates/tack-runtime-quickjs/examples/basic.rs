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
        json!({"message":"hello from tack-rs"}),
    )]));
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(invoker));
    let result = engine
        .execute("const out = await tools.example.echo({}); return out.data.message;")
        .await;
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}
