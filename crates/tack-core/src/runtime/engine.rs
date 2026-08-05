use super::{
    CodeRuntime, ExecutionResult, FailurePhase, RuntimeFailure, RuntimeInput, RuntimeLimits,
};
use crate::ident::validate_aliases;
use crate::tools::{PolicyAwareInvoker, ToolCatalog, ToolInvoker, ToolPolicy, ToolRegistry};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct ExecutionEngine<R> {
    registry: ToolRegistry,
    runtime: R,
    invoker: Arc<dyn ToolInvoker>,
    policy: Option<Arc<dyn ToolPolicy>>,
    limits: RuntimeLimits,
    aliases: HashMap<String, String>,
}

impl<R> ExecutionEngine<R>
where
    R: CodeRuntime,
{
    pub fn new(registry: ToolRegistry, runtime: R, invoker: Arc<dyn ToolInvoker>) -> Self {
        Self {
            registry,
            runtime,
            invoker,
            policy: None,
            limits: RuntimeLimits::default(),
            aliases: HashMap::new(),
        }
    }

    pub fn from_catalog(catalog: ToolCatalog, runtime: R) -> Self {
        let registry = catalog.registry();
        Self::new(registry, runtime, Arc::new(catalog))
    }

    pub fn with_limits(mut self, limits: RuntimeLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_alias(mut self, namespace: impl Into<String>, prefix: impl Into<String>) -> Self {
        self.aliases.insert(namespace.into(), prefix.into());
        self
    }

    pub fn with_policy(mut self, policy: Arc<dyn ToolPolicy>) -> Self {
        self.policy = Some(policy);
        self
    }

    pub async fn execute(&self, source: impl Into<String>) -> ExecutionResult {
        let registry = self.execution_registry();
        if let Err(error) = validate_aliases(&self.aliases) {
            return ExecutionResult::failure(error);
        }
        if let Err(error) = self.limits.validate() {
            return ExecutionResult::failure(RuntimeFailure::new(
                FailurePhase::Runtime,
                error.to_string(),
            ));
        }
        if let Err(error) = registry.validate() {
            return ExecutionResult::failure(RuntimeFailure::new(
                FailurePhase::Runtime,
                error.to_string(),
            ));
        }

        let input = RuntimeInput {
            source: source.into(),
            registry,
            aliases: self.aliases.clone(),
            limits: self.limits.clone(),
        };

        match self.runtime.execute(input, self.host_invoker()).await {
            Ok(output) => ExecutionResult::ok(output),
            Err(error) => ExecutionResult::failure(error),
        }
    }

    pub(crate) fn execution_registry(&self) -> ToolRegistry {
        self.policy
            .as_ref()
            .map(|policy| self.registry.filtered_by_policy(policy.as_ref()))
            .unwrap_or_else(|| self.registry.clone())
    }

    pub(crate) fn host_invoker(&self) -> Arc<dyn ToolInvoker> {
        match &self.policy {
            Some(policy) => Arc::new(PolicyAwareInvoker::new(
                policy.clone(),
                self.invoker.clone(),
            )),
            None => self.invoker.clone(),
        }
    }
}
