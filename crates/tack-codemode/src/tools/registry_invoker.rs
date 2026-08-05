use crate::tools::{ToolCall, ToolCallOutput, ToolInvoker, ToolRegistry};
use async_trait::async_trait;
use std::sync::Arc;

pub struct RegistryAwareInvoker {
    registry: ToolRegistry,
    inner: Arc<dyn ToolInvoker>,
}

impl RegistryAwareInvoker {
    pub fn new(registry: ToolRegistry, inner: Arc<dyn ToolInvoker>) -> Self {
        Self { registry, inner }
    }
}

#[async_trait]
impl ToolInvoker for RegistryAwareInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        if !self.registry.contains_path(&call.path) {
            let message = format!("Unknown tool: {}", call.path);
            let mut output = ToolCallOutput::failure_with_code(
                message.clone(),
                Some("unknown_tool".to_string()),
            );
            output.text = Some(message);
            return output;
        }
        self.inner.call(call).await
    }
}
