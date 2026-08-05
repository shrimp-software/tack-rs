use super::*;

#[tokio::test]
async fn engine_wraps_runtime_result() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        TestRuntime::ok(json!(5)),
        Arc::new(TestInvoker::default()),
    );
    let result = engine.execute("return 5").await;
    assert!(result.ok);
    assert_eq!(result.result, Some(json!(5)));
}

#[tokio::test]
async fn invalid_aliases_fail_before_runtime_execution() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        TestRuntime::ok(json!(5)),
        Arc::new(TestInvoker::default()),
    )
    .with_alias("tools", "eoka");
    let result = engine.execute("return 5").await;
    assert!(!result.ok);
    assert!(result.error.unwrap().message.contains("invalid alias"));
}

#[test]
fn runtime_limits_deserialize_missing_new_fields_from_defaults() {
    let limits: RuntimeLimits = serde_json::from_value(json!({
        "timeoutMs": 1,
        "memoryBytes": 2,
        "stackBytes": 3,
        "maxOutputBytes": 4,
        "maxToolCalls": 5,
        "maxToolRequestBytes": 6,
        "maxToolResponseBytes": 7
    }))
    .unwrap();
    assert_eq!(limits.timeout_ms, 1);
    assert_eq!(
        limits.tool_call_timeout_ms,
        RuntimeLimits::default().tool_call_timeout_ms
    );
}

#[test]
fn runtime_limits_reject_zero_values() {
    let limits = RuntimeLimits {
        memory_bytes: 0,
        ..RuntimeLimits::default()
    };
    assert_eq!(
        limits.validate(),
        Err(RuntimeLimitsError::NonZero {
            field: "memoryBytes"
        })
    );
}

#[tokio::test]
async fn execution_engine_rejects_invalid_runtime_limits() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        TestRuntime::ok(json!(5)),
        Arc::new(TestInvoker::default()),
    )
    .with_limits(RuntimeLimits {
        timeout_ms: 0,
        ..RuntimeLimits::default()
    });
    let result = engine.execute("return 5").await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().phase, FailurePhase::Runtime);
}
