use super::*;

#[test]
fn descriptor_builders_set_metadata() {
    let descriptor = ToolDescriptor::new("eoka.eval", "eval")
        .with_tool_id("tool-1")
        .with_title("Evaluate")
        .with_description("Run code")
        .with_input_schema(json!({"type":"object"}))
        .with_output_schema(json!({"type":"string"}))
        .with_tag("browser")
        .with_tags(["page", "code"])
        .with_read_only(false)
        .with_destructive(true)
        .with_idempotent(false)
        .with_version("v1")
        .with_capability("browser.code");

    assert_eq!(descriptor.path(), "eoka.eval");
    assert_eq!(descriptor.tool_id(), Some("tool-1"));
    assert_eq!(descriptor.name(), "eval");
    assert_eq!(descriptor.title(), Some("Evaluate"));
    assert_eq!(descriptor.description(), Some("Run code"));
    assert_eq!(descriptor.input_schema(), &json!({"type":"object"}));
    assert_eq!(descriptor.output_schema(), &json!({"type":"string"}));
    assert_eq!(descriptor.tags(), &["browser", "page", "code"]);
    assert_eq!(descriptor.annotations().read_only, Some(false));
    assert_eq!(descriptor.annotations().destructive, Some(true));
    assert_eq!(descriptor.annotations().idempotent, Some(false));
    assert_eq!(descriptor.annotations().version.as_deref(), Some("v1"));
    assert_eq!(
        descriptor.annotations().capability.as_deref(),
        Some("browser.code")
    );
}

#[test]
fn empty_annotations_do_not_change_descriptor_json_shape() {
    let value = serde_json::to_value(ToolDescriptor::new("example.one", "one")).unwrap();
    assert!(!value.as_object().unwrap().contains_key("annotations"));
}

#[test]
fn descriptor_policy_filters_by_metadata() {
    let registry = registry(vec![
        ToolDescriptor::new("example.read", "read").with_read_only(true),
        ToolDescriptor::new("example.write", "write").with_destructive(true),
    ]);
    let policy = DescriptorPolicy::new(|descriptor: &ToolDescriptor| {
        descriptor.annotations().read_only == Some(true)
    });
    let filtered = registry.filtered_by_policy(&policy);

    assert!(filtered.contains_path("example.read"));
    assert!(!filtered.contains_path("example.write"));
}

#[tokio::test]
async fn execution_engine_rejects_reserved_tool_paths_before_runtime_execution() {
    let registry = serde_json::from_value(json!({
        "tools": [{"path": "tools.search", "name": "search"}]
    }))
    .unwrap();
    let engine = ExecutionEngine::new(
        registry,
        TestRuntime::ok(json!(5)),
        Arc::new(TestInvoker::default()),
    );
    let result = engine.execute("return 5").await;
    assert!(!result.ok);
    assert_eq!(
        result.error.unwrap().message,
        "tool path `tools.search` is reserved"
    );
}

#[test]
fn registry_from_tools_rejects_invalid_tool_paths() {
    assert!(matches!(
        ToolRegistry::from_tools(vec![ToolDescriptor::new("eoka.fetch-url", "fetch-url")]),
        Err(RegistryValidationError::InvalidToolPath { .. })
    ));
}

#[test]
fn registry_extend_accepts_valid_tools() {
    let mut registry = ToolRegistry::new();
    registry
        .extend([
            ToolDescriptor::new("example.one", "one"),
            ToolDescriptor::new("example.two", "two"),
        ])
        .unwrap();

    assert_eq!(registry.len(), 2);
    assert_eq!(registry.as_slice()[0].path(), "example.one");
    assert_eq!(registry.as_slice()[1].path(), "example.two");
}

#[test]
fn registry_extend_rejects_reserved_paths_without_mutating() {
    let mut registry = registry(vec![ToolDescriptor::new("example.one", "one")]);
    let error = registry
        .extend([ToolDescriptor::new("tools.search", "search")])
        .unwrap_err();

    assert!(matches!(
        error,
        RegistryValidationError::ReservedToolPath(path) if path == "tools.search"
    ));
    assert_eq!(registry.len(), 1);
    assert!(registry.contains_path("example.one"));
}

#[test]
fn registry_extend_rejects_invalid_paths_without_mutating() {
    let mut registry = registry(vec![ToolDescriptor::new("example.one", "one")]);
    let error = registry
        .extend([ToolDescriptor::new("example.bad-path", "bad-path")])
        .unwrap_err();

    assert!(matches!(
        error,
        RegistryValidationError::InvalidToolPath { .. }
    ));
    assert_eq!(registry.len(), 1);
}

#[test]
fn registry_extend_rejects_duplicate_existing_paths_without_mutating() {
    let mut registry = registry(vec![ToolDescriptor::new("example.one", "one")]);
    let error = registry
        .extend([ToolDescriptor::new("example.one", "one")])
        .unwrap_err();

    assert!(matches!(
        error,
        RegistryValidationError::DuplicateToolPath(path) if path == "example.one"
    ));
    assert_eq!(registry.len(), 1);
}

#[test]
fn registry_extend_rejects_duplicate_batch_paths_without_mutating() {
    let mut registry = registry(vec![ToolDescriptor::new("example.one", "one")]);
    let error = registry
        .extend([
            ToolDescriptor::new("example.two", "two"),
            ToolDescriptor::new("example.two", "two"),
        ])
        .unwrap_err();

    assert!(matches!(
        error,
        RegistryValidationError::DuplicateToolPath(path) if path == "example.two"
    ));
    assert_eq!(registry.len(), 1);
    assert!(!registry.contains_path("example.two"));
}
