use crate::tools::{ToolCall, ToolCallOutput, ToolDescriptor, ToolInvoker, ToolPolicy};
use async_trait::async_trait;
use std::sync::Arc;

/// Tool policy that denies exact paths and wildcard path prefixes.
pub struct DenyListPolicy {
    denied_paths: Vec<String>,
}

impl DenyListPolicy {
    pub fn new(denied_paths: Vec<String>) -> Self {
        Self { denied_paths }
    }
}

impl ToolPolicy for DenyListPolicy {
    fn is_allowed(&self, path: &str) -> bool {
        !self.denied_paths.iter().any(|denied| {
            denied == path
                || denied
                    .strip_suffix(".*")
                    .is_some_and(|prefix| path == prefix || path.starts_with(&format!("{prefix}.")))
        })
    }
}

/// Tool policy that allows only descriptors matching a predicate.
pub struct DescriptorPolicy<F> {
    predicate: F,
}

impl<F> DescriptorPolicy<F> {
    pub fn new(predicate: F) -> Self {
        Self { predicate }
    }
}

impl<F> ToolPolicy for DescriptorPolicy<F>
where
    F: Fn(&ToolDescriptor) -> bool + Send + Sync,
{
    fn is_allowed(&self, _path: &str) -> bool {
        true
    }

    fn is_descriptor_allowed(&self, descriptor: &ToolDescriptor) -> bool {
        (self.predicate)(descriptor)
    }
}

pub struct PolicyAwareInvoker {
    policy: Arc<dyn ToolPolicy>,
    inner: Arc<dyn ToolInvoker>,
}

impl PolicyAwareInvoker {
    pub fn new(policy: Arc<dyn ToolPolicy>, inner: Arc<dyn ToolInvoker>) -> Self {
        Self { policy, inner }
    }
}

#[async_trait]
impl ToolInvoker for PolicyAwareInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        if !self.policy.is_allowed(&call.path) {
            let message = format!("Operation denied by policy: {}", call.path);
            let mut output = ToolCallOutput::failure_with_code(
                message.clone(),
                Some("policy_denied".to_string()),
            );
            output.text = Some(message);
            return output;
        }
        self.inner.call(call).await
    }
}
