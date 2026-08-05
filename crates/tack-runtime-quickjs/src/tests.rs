mod execution;
mod host_api;
mod limits;
mod output;
mod transform;

use super::*;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tack_core::{
    CodeRuntime, DenyListPolicy, ExecutionEngine, FailurePhase, MemoryInvoker, RuntimeInput,
    RuntimeLimits, ToolCall, ToolCallOutput, ToolDescriptor, ToolInvoker, ToolRegistry,
};

struct DelayedInvoker;

#[async_trait]
impl ToolInvoker for DelayedInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        tokio::time::sleep(Duration::from_millis(10)).await;
        ToolCallOutput::data(json!({"value": call.input["value"].as_i64().unwrap()}))
    }
}

struct FailingInvoker;

#[async_trait]
impl ToolInvoker for FailingInvoker {
    async fn call(&self, _call: ToolCall) -> ToolCallOutput {
        ToolCallOutput::failure("boom")
    }
}

struct EchoInputInvoker;

#[async_trait]
impl ToolInvoker for EchoInputInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        ToolCallOutput::data(call.input)
    }
}

struct HangingInvoker;

#[async_trait]
impl ToolInvoker for HangingInvoker {
    async fn call(&self, _call: ToolCall) -> ToolCallOutput {
        tokio::time::sleep(Duration::from_secs(1)).await;
        ToolCallOutput::data(json!({"done": true}))
    }
}
