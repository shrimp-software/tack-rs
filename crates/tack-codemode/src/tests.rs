use super::*;
use crate::tools::schema::schema_to_type;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::test]
async fn engine_wraps_runtime_result() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        StaticRuntime::ok(json!(5)),
        Arc::new(MemoryInvoker::default()),
    );
    let result = engine.execute("return 5").await;
    assert!(result.ok);
    assert_eq!(result.result, Some(json!(5)));
}

#[tokio::test]
async fn builtins_search_and_describe() {
    let registry = ToolRegistry {
        tools: vec![ToolDescriptor {
            path: "eoka.eval".to_string(),
            tool_id: None,
            name: "eval".to_string(),
            title: Some("Evaluate JavaScript".to_string()),
            description: Some("Run JavaScript in the active page".to_string()),
            input_schema: json!({"type":"object","required":["code"],"properties":{"code":{"type":"string"}}}),
            output_schema: json!({"type":"object","properties":{"value":{"type":"string"}}}),
            tags: vec!["browser".to_string()],
        }],
    };
    let invoker = BuiltinAwareInvoker::new(registry.clone(), Arc::new(MemoryInvoker::default()));
    let search = invoker
        .call(ToolCall {
            path: "tools.search".to_string(),
            input: json!({"query":"javascript","limit":5}),
        })
        .await;
    assert!(search.ok);
    assert_eq!(search.data.unwrap()[0]["path"], "eoka.eval");

    let description = describe_tool(&registry, "eoka.eval").unwrap();
    assert!(description.type_signature.contains("code: string"));
}

#[tokio::test]
async fn builtin_invoke_dispatches_to_target_tool() {
    let registry = ToolRegistry {
        tools: vec![ToolDescriptor::new("eoka.eval", "eval")],
    };
    let invoker = BuiltinAwareInvoker::new(
        registry,
        Arc::new(MemoryInvoker::new(HashMap::from([(
            "eoka.eval".to_string(),
            json!({"value": 11}),
        )]))),
    );
    let output = invoker
        .call(ToolCall {
            path: "tools.invoke".to_string(),
            input: json!({"path":"eoka.eval","input":{"code":"return 11"}}),
        })
        .await;
    assert!(output.ok);
    assert_eq!(output.data, Some(json!({"value": 11})));

    let missing_path = invoker
        .call(ToolCall {
            path: "tools.invoke".to_string(),
            input: json!({}),
        })
        .await;
    assert!(!missing_path.ok);
}

#[tokio::test]
async fn policy_denies_exact_and_wildcard_paths() {
    let policy = Arc::new(DenyListPolicy::new(vec![
        "example.secret".to_string(),
        "example.admin.*".to_string(),
    ]));
    let invoker = PolicyAwareInvoker::new(policy, Arc::new(MemoryInvoker::default()));
    let exact = invoker
        .call(ToolCall {
            path: "example.secret".to_string(),
            input: json!({}),
        })
        .await;
    assert!(!exact.ok);
    assert_eq!(exact.error.unwrap().code.as_deref(), Some("policy_denied"));

    let wildcard = invoker
        .call(ToolCall {
            path: "example.admin.delete".to_string(),
            input: json!({}),
        })
        .await;
    assert!(!wildcard.ok);
}

#[tokio::test]
async fn registry_denies_undeclared_tool_paths() {
    let invoker = RegistryAwareInvoker::new(
        ToolRegistry {
            tools: vec![ToolDescriptor::new("example.allowed", "allowed")],
        },
        Arc::new(MemoryInvoker::new(HashMap::from([(
            "example.hidden".to_string(),
            json!({"leaked": true}),
        )]))),
    );
    let output = invoker
        .call(ToolCall {
            path: "example.hidden".to_string(),
            input: json!({}),
        })
        .await;
    assert!(!output.ok);
    assert_eq!(output.error.unwrap().code.as_deref(), Some("unknown_tool"));
}

#[tokio::test]
async fn policy_filters_discovery_registry() {
    let registry = ToolRegistry {
        tools: vec![
            ToolDescriptor::new("example.visible", "visible"),
            ToolDescriptor::new("example.secret", "secret"),
        ],
    };
    let engine = ExecutionEngine::new(
        registry,
        StaticRuntime::ok(json!(null)),
        Arc::new(MemoryInvoker::default()),
    )
    .with_policy(Arc::new(DenyListPolicy::new(vec![
        "example.secret".to_string(),
    ])));
    let invoker = BuiltinAwareInvoker::new(engine.execution_registry(), engine.host_invoker());
    let search = invoker
        .call(ToolCall {
            path: "tools.search".to_string(),
            input: json!({"query":"example","limit":10}),
        })
        .await;
    assert_eq!(search.data.unwrap().as_array().unwrap().len(), 1);

    let description = invoker
        .call(ToolCall {
            path: "tools.describe.tool".to_string(),
            input: json!({"path":"example.secret"}),
        })
        .await;
    assert!(!description.ok);
}

#[tokio::test]
async fn invalid_aliases_fail_before_runtime_execution() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        StaticRuntime::ok(json!(5)),
        Arc::new(MemoryInvoker::default()),
    )
    .with_alias("tools", "eoka");
    let result = engine.execute("return 5").await;
    assert!(!result.ok);
    assert!(result.error.unwrap().message.contains("invalid alias"));
}

#[tokio::test]
async fn execution_engine_rejects_reserved_tool_paths_before_runtime_execution() {
    let engine = ExecutionEngine::new(
        ToolRegistry {
            tools: vec![ToolDescriptor::new("tools.search", "search")],
        },
        StaticRuntime::ok(json!(5)),
        Arc::new(MemoryInvoker::default()),
    );
    let result = engine.execute("return 5").await;
    assert!(!result.ok);
    assert_eq!(
        result.error.unwrap().message,
        "tool path `tools.search` is reserved"
    );
}

#[test]
fn registry_validation_rejects_invalid_tool_paths() {
    let registry = ToolRegistry {
        tools: vec![ToolDescriptor::new("eoka.fetch-url", "fetch-url")],
    };
    assert!(matches!(
        registry.validate(),
        Err(RegistryValidationError::InvalidToolPath { .. })
    ));
}

#[test]
fn schema_type_handles_objects() {
    let ty = schema_to_type(&json!({
        "type":"object",
        "required":["url", "content-type"],
        "properties":{"url":{"type":"string"},"timeout":{"type":"integer"},"content-type":{"type":"string"}}
    }));
    assert_eq!(
        ty,
        "{ \"content-type\": string; timeout?: number; url: string }"
    );
}

#[test]
fn schema_type_handles_unions_enums_and_maps() {
    let nullable = schema_to_type(&json!({"type":["string","null"]}));
    assert_eq!(nullable, "string | null");

    let choices = schema_to_type(&json!({"enum":["low","high",null]}));
    assert_eq!(choices, "\"low\" | \"high\" | null");

    let union = schema_to_type(&json!({
        "oneOf":[{"type":"integer"},{"const":"auto"}]
    }));
    assert_eq!(union, "number | \"auto\"");

    let map = schema_to_type(&json!({
        "type":"object",
        "additionalProperties":{"type":"boolean"}
    }));
    assert_eq!(map, "Record<string, boolean>");
}

#[test]
fn tool_call_output_omits_absent_fields() {
    let value = serde_json::to_value(ToolCallOutput::data(json!({"ok":true}))).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.get("ok"), Some(&Value::Bool(true)));
    assert!(!object.contains_key("error"));
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
        StaticRuntime::ok(json!(5)),
        Arc::new(MemoryInvoker::default()),
    )
    .with_limits(RuntimeLimits {
        timeout_ms: 0,
        ..RuntimeLimits::default()
    });
    let result = engine.execute("return 5").await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().phase, FailurePhase::Runtime);
}

#[test]
fn registry_imports_tack_manifest_tools() {
    let registry = ToolRegistry::from_manifest_value(&json!({
        "version":"0.1",
        "generatedAt":"2026-01-01T00:00:00.000Z",
        "servers":{},
        "tools":{
            "grafana.list_datasources":{
                "id":"grafana.list_datasources",
                "serverId":"grafana",
                "namespaceName":"grafana",
                "sdkName":"list_datasources",
                "upstreamName":"list_datasources",
                "description":"List data sources",
                "inputSchema":{"type":"object","additionalProperties":false}
            }
        }
    }))
    .unwrap();
    assert_eq!(registry.tools[0].path, "grafana.list_datasources");
    assert_eq!(
        registry.tools[0].tool_id.as_deref(),
        Some("grafana.list_datasources")
    );
}

#[test]
fn registry_import_rejects_duplicate_sdk_paths() {
    let error = ToolRegistry::from_manifest_value(&json!({
        "tools": {
            "left": {"namespaceName":"eoka","sdkName":"eval"},
            "right": {"namespaceName":"eoka","sdkName":"eval"}
        }
    }))
    .unwrap_err();
    assert!(matches!(
        error,
        ManifestImportError::Registry(RegistryValidationError::DuplicateToolPath(path))
            if path == "eoka.eval"
    ));
}

#[test]
fn registry_import_rejects_reserved_sdk_paths() {
    let error = ToolRegistry::from_manifest_value(&json!({
        "tools": {
            "left": {"namespaceName":"tools","sdkName":"search"}
        }
    }))
    .unwrap_err();
    assert!(matches!(
        error,
        ManifestImportError::Registry(RegistryValidationError::ReservedToolPath(path))
            if path == "tools.search"
    ));
}
