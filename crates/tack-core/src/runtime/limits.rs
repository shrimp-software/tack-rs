use serde::{Deserialize, Serialize};
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
