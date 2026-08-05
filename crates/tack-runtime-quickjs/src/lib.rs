//! QuickJS runtime for tool-backed TypeScript snippets.
//!
//! This crate executes TypeScript snippets against the host-independent
//! contracts from `tack-codemode`. It uses Oxc to strip TypeScript syntax,
//! evaluates code in an isolated QuickJS context, exposes host tool calls,
//! `emit`, and `console.log`, and enforces runtime and payload limits.

mod runtime;

use async_trait::async_trait;
use std::sync::Arc;
use tack_codemode::{CodeRuntime, RuntimeFailure, RuntimeInput, RuntimeOutput, ToolInvoker};

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
