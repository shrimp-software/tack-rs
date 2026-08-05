use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn captures_emit_and_console_log() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    );
    let result = engine
        .execute("emit({step: 1}); console.log('hello'); return 7;")
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.result, Some(json!(7)));
    assert_eq!(result.emitted, vec![json!({"step": 1})]);
    assert_eq!(result.logs, vec!["hello".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn preserves_captured_output_on_runtime_failure() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    );
    let result = engine
        .execute("emit({step: 1}); console.log('before'); throw new Error('boom');")
        .await;
    assert!(!result.ok);
    assert_eq!(result.emitted, vec![json!({"step": 1})]);
    assert_eq!(result.logs, vec!["before".to_string()]);
    let error = result.error.unwrap();
    assert_eq!(error.emitted, vec![json!({"step": 1})]);
    assert_eq!(error.logs, vec!["before".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn preserves_captured_output_on_timeout() {
    let limits = RuntimeLimits {
        timeout_ms: 50,
        ..RuntimeLimits::default()
    };
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    )
    .with_limits(limits);
    let result = engine
        .execute("console.log('before'); while (true) {}")
        .await;
    assert!(!result.ok);
    assert_eq!(result.logs, vec!["before".to_string()]);
    assert_eq!(result.error.unwrap().logs, vec!["before".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn console_log_formats_multiple_arguments() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    );
    let result = engine
        .execute("console.log('value', {x: 1}, undefined); return 1;")
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.logs, vec!["value {\"x\":1} undefined".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn tool_request_serialization_errors_return_envelopes() {
    let registry = ToolRegistry {
        tools: vec![ToolDescriptor::new("example.echo", "echo")],
    };
    let engine = ExecutionEngine::new(
        registry,
        QuickJsRuntime,
        Arc::new(MemoryInvoker::new(HashMap::from([(
            "example.echo".to_string(),
            json!({"unexpected":"host call"}),
        )]))),
    );
    let result = engine
        .execute("const input = {}; input.self = input; return await tools.example.echo(input);")
        .await;
    assert!(result.ok, "{:?}", result.error);
    let value = result.result.unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["error"]["message"],
        "tool request is not JSON serializable"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn result_serialization_matches_json_stringify_semantics() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    );

    let undefined = engine.execute("return undefined;").await;
    assert!(undefined.ok, "{:?}", undefined.error);
    assert_eq!(undefined.result, Some(Value::Null));

    let bigint = engine.execute("return 1n;").await;
    assert!(!bigint.ok);
    assert_eq!(bigint.error.unwrap().phase, FailurePhase::Runtime);

    let cyclic = engine
        .execute("const value = {}; value.self = value; return value;")
        .await;
    assert!(!cyclic.ok);
    assert_eq!(cyclic.error.unwrap().phase, FailurePhase::Runtime);
}
