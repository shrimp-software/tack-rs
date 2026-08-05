use crate::ident::validate_aliases;
use crate::tools::{PolicyAwareInvoker, ToolInvoker, ToolPolicy, ToolRegistry};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLimits {
    pub timeout_ms: u64,
    pub memory_bytes: usize,
    pub stack_bytes: usize,
    pub max_output_bytes: usize,
    pub max_tool_calls: usize,
    pub tool_call_timeout_ms: u64,
    pub max_tool_request_bytes: usize,
    pub max_tool_response_bytes: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            memory_bytes: 128 * 1024 * 1024,
            stack_bytes: 1_000_000,
            max_output_bytes: 1_000_000,
            max_tool_calls: 100,
            tool_call_timeout_ms: 30_000,
            max_tool_request_bytes: 1_000_000,
            max_tool_response_bytes: 1_000_000,
        }
    }
}

impl RuntimeLimits {
    pub fn validate(&self) -> Result<(), RuntimeLimitsError> {
        ensure_limit_nonzero("timeoutMs", self.timeout_ms)?;
        ensure_limit_nonzero("memoryBytes", self.memory_bytes)?;
        ensure_limit_nonzero("stackBytes", self.stack_bytes)?;
        ensure_limit_nonzero("maxOutputBytes", self.max_output_bytes)?;
        ensure_limit_nonzero("maxToolCalls", self.max_tool_calls)?;
        ensure_limit_nonzero("toolCallTimeoutMs", self.tool_call_timeout_ms)?;
        ensure_limit_nonzero("maxToolRequestBytes", self.max_tool_request_bytes)?;
        ensure_limit_nonzero("maxToolResponseBytes", self.max_tool_response_bytes)?;
        Ok(())
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    pub fn tool_call_timeout(&self) -> Duration {
        Duration::from_millis(self.tool_call_timeout_ms)
    }
}

/// Error returned when runtime limits would disable a required guard.
///
/// All limits must be greater than zero. A zero memory or stack limit is
/// rejected instead of relying on backend-specific "unlimited" semantics.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RuntimeLimitsError {
    #[error("runtime limit `{field}` must be greater than zero")]
    NonZero { field: &'static str },
}

fn ensure_limit_nonzero<T>(field: &'static str, value: T) -> Result<(), RuntimeLimitsError>
where
    T: PartialEq + Default,
{
    if value == T::default() {
        Err(RuntimeLimitsError::NonZero { field })
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInput {
    pub source: String,
    pub registry: ToolRegistry,
    pub aliases: HashMap<String, String>,
    pub limits: RuntimeLimits,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOutput {
    #[serde(default)]
    pub result: Value,
    #[serde(default)]
    pub emitted: Vec<Value>,
    #[serde(default)]
    pub logs: Vec<String>,
}

#[async_trait(?Send)]
pub trait CodeRuntime: Send + Sync {
    async fn execute(
        &self,
        input: RuntimeInput,
        invoker: Arc<dyn ToolInvoker>,
    ) -> Result<RuntimeOutput, RuntimeFailure>;
}

#[derive(Clone, Debug, Error, Serialize, Deserialize, PartialEq, Eq)]
#[error("{phase}: {message}")]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFailure {
    pub phase: FailurePhase,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emitted: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<String>,
}

impl RuntimeFailure {
    pub fn new(phase: FailurePhase, message: impl Into<String>) -> Self {
        Self {
            phase,
            message: message.into(),
            emitted: Vec::new(),
            logs: Vec::new(),
        }
    }

    pub fn with_output(mut self, emitted: Vec<Value>, logs: Vec<String>) -> Self {
        self.emitted = emitted;
        self.logs = logs;
        self
    }

    pub fn from_output(
        phase: FailurePhase,
        message: impl Into<String>,
        output: &RuntimeOutput,
    ) -> Self {
        Self::new(phase, message).with_output(output.emitted.clone(), output.logs.clone())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FailurePhase {
    Parse,
    Runtime,
    Timeout,
}

impl fmt::Display for FailurePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse => f.write_str("parse"),
            Self::Runtime => f.write_str("runtime"),
            Self::Timeout => f.write_str("timeout"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default)]
    pub emitted: Vec<Value>,
    #[serde(default)]
    pub logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RuntimeFailure>,
}

impl ExecutionResult {
    pub fn ok(output: RuntimeOutput) -> Self {
        Self {
            ok: true,
            result: Some(output.result),
            emitted: output.emitted,
            logs: output.logs,
            error: None,
        }
    }

    pub fn failure(error: RuntimeFailure) -> Self {
        Self {
            ok: false,
            result: None,
            emitted: error.emitted.clone(),
            logs: error.logs.clone(),
            error: Some(error),
        }
    }
}

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

/// Runtime implementation that always returns a fixed result.
///
/// This is useful for host-side tests that need to exercise [`ExecutionEngine`]
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
