mod builtin_invoker;
mod catalog;
mod discovery;
mod invocation;
mod paths;
mod policy;
mod registry;
mod registry_invoker;
pub(crate) mod type_signature;

pub use builtin_invoker::BuiltinAwareInvoker;
pub use catalog::{
    CatalogError, FunctionTool, Tool, ToolCatalog, ToolLayer, ToolSet, ToolSetError,
};
#[cfg(feature = "schemars")]
pub use catalog::{TypedFunctionTool, TypedTool, TypedToolAdapter};
pub use discovery::{SearchHit, ToolDescription, describe_tool, search_tools};
pub use invocation::{
    ToolCall, ToolCallError, ToolCallOutput, ToolInvoker, ToolPolicy, ToolResult,
};
pub use policy::{DenyListPolicy, DescriptorPolicy, PolicyAwareInvoker};
pub use registry::{RegistryValidationError, ToolAnnotations, ToolDescriptor, ToolRegistry};
pub use registry_invoker::RegistryAwareInvoker;
