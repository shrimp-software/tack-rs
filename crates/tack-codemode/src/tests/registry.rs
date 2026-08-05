use super::*;

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
