use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn enforces_capture_limit_during_logs() {
    let limits = RuntimeLimits {
        max_output_bytes: 32,
        ..RuntimeLimits::default()
    };
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    )
    .with_limits(limits);
    let result = engine
        .execute("for (let i = 0; i < 10; i++) console.log('xxxxxxxxxx'); return 1;")
        .await;
    assert!(!result.ok);
    assert!(result.error.unwrap().message.contains("output exceeded"));
}

#[tokio::test(flavor = "multi_thread")]
async fn enforces_output_limit() {
    let limits = RuntimeLimits {
        max_output_bytes: 32,
        ..RuntimeLimits::default()
    };
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    )
    .with_limits(limits);
    let result = engine.execute("return 'x'.repeat(100);").await;
    assert!(!result.ok);
    assert!(result.error.unwrap().message.contains("output exceeded"));
}

#[tokio::test(flavor = "multi_thread")]
async fn enforces_tool_call_limit() {
    let echo_registry = registry(vec![ToolDescriptor::new("example.echo", "echo")]);
    let mut outputs = HashMap::new();
    outputs.insert("example.echo".to_string(), json!({"ok": true}));
    let limits = RuntimeLimits {
        max_tool_calls: 1,
        ..RuntimeLimits::default()
    };
    let engine = ExecutionEngine::new(
        echo_registry,
        QuickJsRuntime,
        Arc::new(MemoryInvoker::new(outputs)),
    )
    .with_limits(limits);
    let result = engine
        .execute("await tools.example.echo({}); return await tools.example.echo({});")
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.result.unwrap()["ok"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn enforces_tool_payload_limits() {
    let request_registry = registry(vec![ToolDescriptor::new("example.echo", "echo")]);
    let mut outputs = HashMap::new();
    outputs.insert(
        "example.echo".to_string(),
        json!({"value": "x".repeat(100)}),
    );
    let limits = RuntimeLimits {
        max_tool_request_bytes: 40,
        max_tool_response_bytes: 40,
        ..RuntimeLimits::default()
    };
    let engine = ExecutionEngine::new(
        request_registry,
        QuickJsRuntime,
        Arc::new(MemoryInvoker::new(outputs)),
    )
    .with_limits(limits.clone());
    let request_result = engine
        .execute("return await tools.example.echo({value: 'x'.repeat(100)});")
        .await;
    assert!(request_result.ok, "{:?}", request_result.error);
    let request = request_result.result.unwrap();
    assert_eq!(request["ok"], false);
    assert_eq!(request["error"]["message"], "tool request too large");

    let engine = ExecutionEngine::new(
        registry(vec![ToolDescriptor::new("example.echo", "echo")]),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::new(HashMap::from([(
            "example.echo".to_string(),
            json!({"value": "x".repeat(100)}),
        )]))),
    )
    .with_limits(RuntimeLimits {
        max_tool_request_bytes: 1_000,
        max_tool_response_bytes: 40,
        ..RuntimeLimits::default()
    });
    let response_result = engine.execute("return await tools.example.echo({});").await;
    assert!(response_result.ok, "{:?}", response_result.error);
    assert_eq!(response_result.result.unwrap()["ok"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn times_out_pending_promises() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    )
    .with_limits(RuntimeLimits {
        timeout_ms: 25,
        ..RuntimeLimits::default()
    });
    let result = engine.execute("return await new Promise(() => {});").await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().phase, FailurePhase::Timeout);
}

#[tokio::test(flavor = "multi_thread")]
async fn direct_runtime_rejects_invalid_runtime_limits_before_timeout() {
    let input = RuntimeInput {
        source: "return 1;".to_string(),
        registry: ToolRegistry::default(),
        aliases: HashMap::new(),
        limits: RuntimeLimits {
            timeout_ms: 0,
            ..RuntimeLimits::default()
        },
    };
    let error = QuickJsRuntime
        .execute(input, Arc::new(MemoryInvoker::default()))
        .await
        .unwrap_err();
    assert_eq!(error.phase, FailurePhase::Runtime);
    assert!(error.message.contains("timeoutMs"));
}

#[tokio::test(flavor = "multi_thread")]
async fn enforces_tool_call_timeout() {
    let registry = registry(vec![ToolDescriptor::new("example.hang", "hang")]);
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(HangingInvoker))
        .with_limits(RuntimeLimits {
            tool_call_timeout_ms: 10,
            ..RuntimeLimits::default()
        });
    let result = engine.execute("return await tools.example.hang({});").await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(
        result.result.unwrap()["error"]["message"],
        "tool call timed out"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupts_cpu_bound_code_at_timeout() {
    let limits = RuntimeLimits {
        timeout_ms: 25,
        ..RuntimeLimits::default()
    };
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    )
    .with_limits(limits);
    let result = engine.execute("while (true) {}").await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().phase, FailurePhase::Timeout);
}
