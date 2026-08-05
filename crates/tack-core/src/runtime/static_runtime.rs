use super::{CodeRuntime, RuntimeFailure, RuntimeInput, RuntimeOutput};
use crate::tools::ToolInvoker;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Runtime implementation that always returns a fixed result.
///
/// This is useful for host-side tests that need to exercise [`super::ExecutionEngine`]
/// policy, registry, and result wrapping without starting a JavaScript runtime.
#[derive(Clone, Debug)]
pub struct StaticRuntime {
    output: Result<RuntimeOutput, RuntimeFailure>,
}

impl StaticRuntime {
    pub fn ok(result: Value) -> Self {
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
impl CodeRuntime for StaticRuntime {
    async fn execute(
        &self,
        _input: RuntimeInput,
        _invoker: Arc<dyn ToolInvoker>,
    ) -> Result<RuntimeOutput, RuntimeFailure> {
        self.output.clone()
    }
}
