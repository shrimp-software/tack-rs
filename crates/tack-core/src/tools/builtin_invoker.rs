use crate::tools::{ToolCall, ToolCallOutput, ToolInvoker, ToolRegistry};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use super::paths;

pub struct BuiltinAwareInvoker {
    registry: ToolRegistry,
    inner: Arc<dyn ToolInvoker>,
}

impl BuiltinAwareInvoker {
    pub fn new(registry: ToolRegistry, inner: Arc<dyn ToolInvoker>) -> Self {
        Self { registry, inner }
    }
}

#[async_trait]
impl ToolInvoker for BuiltinAwareInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        match call.path.as_str() {
            paths::SEARCH => self.search(call.input),
            paths::DESCRIBE_TOOL => self.describe(call.input),
            paths::INVOKE => self.invoke(call.input).await,
            _ => self.inner.call(call).await,
        }
    }
}

impl BuiltinAwareInvoker {
    fn search(&self, input: Value) -> ToolCallOutput {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
        ToolCallOutput::data(json!(super::discovery::search_tools(
            &self.registry,
            query,
            limit
        )))
    }

    fn describe(&self, input: Value) -> ToolCallOutput {
        let path = input
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match super::discovery::describe_tool(&self.registry, path) {
            Some(description) => ToolCallOutput::data(json!(description)),
            None => ToolCallOutput::failure(format!("Unknown tool: {path}")),
        }
    }

    async fn invoke(&self, input: Value) -> ToolCallOutput {
        let Some(path) = input.get("path").and_then(Value::as_str) else {
            return ToolCallOutput::failure("tool path is required");
        };
        if path.is_empty() || path == paths::INVOKE {
            return ToolCallOutput::failure("tool path is required");
        }
        let input = input.get("input").cloned().unwrap_or_else(|| json!({}));
        self.call(ToolCall {
            path: path.to_string(),
            input,
        })
        .await
    }
}
