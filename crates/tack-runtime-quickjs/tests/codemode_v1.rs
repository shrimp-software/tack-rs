use async_trait::async_trait;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tack_core::{
    ExecutionEngine, FailurePhase, RuntimeLimits, ToolCall, ToolCallOutput, ToolDescriptor,
    ToolInvoker, ToolRegistry,
};
use tack_runtime_quickjs::QuickJsRuntime;

const CONTRACT: &str = include_str!("fixtures/codemode-v1.fixture.json");

struct MemoryInvoker {
    outputs: HashMap<String, Value>,
}

impl MemoryInvoker {
    fn new(outputs: HashMap<String, Value>) -> Self {
        Self { outputs }
    }
}

#[async_trait]
impl ToolInvoker for MemoryInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        self.outputs
            .get(&call.path)
            .cloned()
            .map(ToolCallOutput::data)
            .unwrap_or_else(|| ToolCallOutput::failure(format!("Unknown tool: {}", call.path)))
    }
}

fn registry(tools: Vec<ToolDescriptor>) -> ToolRegistry {
    ToolRegistry::from_tools(tools).unwrap()
}

#[test]
fn defaults_match_contract_fixture() {
    let contract: Value = serde_json::from_str(CONTRACT).unwrap();
    let defaults = RuntimeLimits::default();
    assert_eq!(defaults.timeout_ms, contract["defaults"]["timeoutMs"]);
    assert_eq!(
        defaults.memory_bytes / 1024 / 1024,
        contract["defaults"]["memoryMb"]
    );
    assert_eq!(defaults.stack_bytes, contract["defaults"]["maxStackBytes"]);
    assert_eq!(
        defaults.max_output_bytes,
        contract["defaults"]["maxOutputBytes"]
    );
    assert_eq!(
        defaults.max_tool_calls,
        contract["defaults"]["maxToolCalls"]
    );
    assert_eq!(
        defaults.tool_call_timeout_ms,
        contract["defaults"]["toolCallTimeoutMs"]
    );
    assert_eq!(
        defaults.max_tool_request_bytes,
        contract["defaults"]["maxToolRequestBytes"]
    );
    assert_eq!(
        defaults.max_tool_response_bytes,
        contract["defaults"]["maxToolResponseBytes"]
    );
}

#[test]
fn error_phases_match_contract_fixture() {
    let contract: Value = serde_json::from_str(CONTRACT).unwrap();
    let phases = serde_json::to_value([
        FailurePhase::Parse,
        FailurePhase::Runtime,
        FailurePhase::Timeout,
    ])
    .unwrap();
    assert_eq!(phases, contract["errorPhases"]);
}

#[test]
fn builtins_match_contract_fixture() {
    let contract: Value = serde_json::from_str(CONTRACT).unwrap();
    assert_eq!(
        contract["builtins"],
        json!(["search", "describe.tool", "invoke"])
    );
}

#[test]
fn tool_envelopes_match_contract_fixture() {
    let contract: Value = serde_json::from_str(CONTRACT).unwrap();
    let success = serde_json::to_value(ToolCallOutput {
        ok: true,
        data: Some(json!({"value": 3})),
        text: Some("{\"value\":3}".to_string()),
        raw: None,
        error: None,
    })
    .unwrap();
    let failure = serde_json::to_value(ToolCallOutput {
        ok: false,
        data: None,
        text: Some("denied".to_string()),
        raw: None,
        error: Some(tack_core::ToolCallError {
            message: "denied".to_string(),
            code: None,
            details: None,
        }),
    })
    .unwrap();
    assert_eq!(success, contract["toolResult"]["success"]);
    assert_eq!(failure, contract["toolResult"]["failure"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_executes_contract_shape() {
    let registry = registry(vec![ToolDescriptor::new("example.echo", "echo")]);
    let invoker = MemoryInvoker::new(HashMap::from([(
        "example.echo".to_string(),
        json!({"value": 3}),
    )]));
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(invoker));
    let result = engine
        .execute(
            "emit({phase:'start'}); console.log('running'); const out = await tools.example.echo({}); return out.data;",
        )
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.result, Some(json!({"value": 3})));
    assert_eq!(result.emitted, vec![json!({"phase":"start"})]);
    assert_eq!(result.logs, vec!["running".to_string()]);
}
