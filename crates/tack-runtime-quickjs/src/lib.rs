//! QuickJS runtime for tool-backed TypeScript snippets.
//!
//! This crate executes TypeScript snippets against the host-independent
//! contracts from `tack-core`. It uses Oxc to strip TypeScript syntax,
//! evaluates code in an isolated QuickJS context, exposes host tool calls,
//! `emit`, and `console.log`, and enforces runtime and payload limits.
//!
//! Run the examples from the workspace root with:
//!
//! ```text
//! cargo run -p tack-runtime-quickjs --example catalog
//! cargo run -p tack-runtime-quickjs --example dynamic_adapter
//! cargo run -p tack-runtime-quickjs --example basic
//! cargo run -p tack-runtime-quickjs --example discovery
//! cargo run -p tack-runtime-quickjs --example aliases_policy_limits
//! ```

mod runtime;

use async_trait::async_trait;
use std::sync::Arc;
use tack_core::{CodeRuntime, RuntimeFailure, RuntimeInput, RuntimeOutput, ToolInvoker};

#[derive(Clone, Debug, Default)]
pub struct QuickJsRuntime;

#[async_trait(?Send)]
impl CodeRuntime for QuickJsRuntime {
    async fn execute(
        &self,
        input: RuntimeInput,
        invoker: Arc<dyn ToolInvoker>,
    ) -> Result<RuntimeOutput, RuntimeFailure> {
        runtime::execute(input, invoker).await
    }
}

#[cfg(test)]
mod tests;
