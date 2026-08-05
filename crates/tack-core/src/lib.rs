//! Host-independent contracts for tool-backed TypeScript execution.
//!
//! This crate defines the stable data model shared by hosts and runtimes: tool
//! descriptors, invocation envelopes, runtime limits, execution results,
//! built-in discovery helpers, and policy hooks. Hosts provide a
//! [`ToolInvoker`], runtimes implement [`CodeRuntime`], and
//! [`ExecutionEngine`] ties them together.

mod ident;
mod runtime;
mod tools;

pub use runtime::{
    CodeRuntime, ExecutionEngine, ExecutionResult, FailurePhase, RuntimeFailure, RuntimeInput,
    RuntimeLimits, RuntimeLimitsError, RuntimeOutput,
};
pub use tools::{
    BuiltinAwareInvoker, DenyListPolicy, PolicyAwareInvoker, RegistryAwareInvoker,
    RegistryValidationError, SearchHit, ToolCall, ToolCallError, ToolCallOutput, ToolDescription,
    ToolDescriptor, ToolInvoker, ToolPolicy, ToolRegistry, describe_tool, search_tools,
};

pub use ident::validate_aliases;

#[cfg(test)]
mod tests;
