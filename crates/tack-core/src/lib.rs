//! Host-independent contracts for tool-backed TypeScript execution.
//!
//! This crate defines the stable data model shared by hosts and runtimes: tool
//! descriptors, invocation envelopes, runtime limits, execution results,
//! built-in discovery helpers, and policy hooks. Hosts usually register tools
//! in a [`ToolCatalog`], runtimes implement [`CodeRuntime`], and
//! [`ExecutionEngine`] ties them together.
//!
//! See the workspace README for an end-to-end example, and the repository
//! `docs/` directory for the host quickstart and TypeScript API notes.

mod ident;
mod runtime;
mod tools;

pub use runtime::{
    CodeRuntime, ExecutionEngine, ExecutionResult, FailurePhase, RuntimeFailure, RuntimeInput,
    RuntimeLimits, RuntimeLimitsError, RuntimeOutput,
};
pub use tools::{
    BuiltinAwareInvoker, CatalogError, DenyListPolicy, DescriptorPolicy, FunctionTool,
    PolicyAwareInvoker, RegistryAwareInvoker, RegistryValidationError, SearchHit, Tool,
    ToolAnnotations, ToolCall, ToolCallError, ToolCallOutput, ToolCatalog, ToolDescription,
    ToolDescriptor, ToolInvoker, ToolLayer, ToolPolicy, ToolRegistry, ToolResult, ToolSet,
    ToolSetError, describe_tool, search_tools,
};
#[cfg(feature = "schemars")]
pub use tools::{TypedFunctionTool, TypedTool, TypedToolAdapter};

pub use ident::validate_aliases;

#[cfg(test)]
mod tests;
