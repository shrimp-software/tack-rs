mod builtins;
mod invocation;
mod registry;
mod runtime;
mod type_signature;

use super::*;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
struct TestInvoker {
    outputs: HashMap<String, Value>,
}

impl TestInvoker {
    fn new(outputs: HashMap<String, Value>) -> Self {
        Self { outputs }
    }
}

#[async_trait]
impl ToolInvoker for TestInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        self.outputs
            .get(&call.path)
            .cloned()
            .map(ToolCallOutput::data)
            .unwrap_or_else(|| ToolCallOutput::failure(format!("Unknown tool: {}", call.path)))
    }
}

#[derive(Clone, Debug)]
struct TestRuntime {
    output: Result<RuntimeOutput, RuntimeFailure>,
}

impl TestRuntime {
    fn ok(result: Value) -> Self {
        Self {
            output: Ok(RuntimeOutput {
                result,
                emitted: Vec::new(),
                logs: Vec::new(),
            }),
        }
    }
}

#[async_trait(?Send)]
impl CodeRuntime for TestRuntime {
    async fn execute(
        &self,
        _input: RuntimeInput,
        _invoker: Arc<dyn ToolInvoker>,
    ) -> Result<RuntimeOutput, RuntimeFailure> {
        self.output.clone()
    }
}

fn registry(tools: Vec<ToolDescriptor>) -> ToolRegistry {
    ToolRegistry::from_tools(tools).unwrap()
}
