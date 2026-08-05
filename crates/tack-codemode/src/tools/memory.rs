use crate::tools::{ToolCall, ToolCallOutput, ToolInvoker};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

/// In-memory tool invoker useful for examples, tests, and simple static hosts.
#[derive(Default)]
pub struct MemoryInvoker {
    outputs: tokio::sync::Mutex<HashMap<String, Value>>,
}

impl MemoryInvoker {
    pub fn new(outputs: HashMap<String, Value>) -> Self {
        Self {
            outputs: tokio::sync::Mutex::new(outputs),
        }
    }
}

#[async_trait]
impl ToolInvoker for MemoryInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        self.outputs
            .lock()
            .await
            .get(&call.path)
            .cloned()
            .map(ToolCallOutput::data)
            .unwrap_or_else(|| ToolCallOutput::failure(format!("Unknown tool: {}", call.path)))
    }
}
