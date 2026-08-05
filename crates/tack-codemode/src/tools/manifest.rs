use crate::tools::{ToolDescriptor, ToolRegistry};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ManifestImportError {
    #[error("invalid Tack manifest: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid Tack registry: {0}")]
    Registry(#[from] super::RegistryValidationError),
}

pub(crate) fn registry_from_manifest_value(
    value: &Value,
) -> Result<ToolRegistry, ManifestImportError> {
    let manifest: TackManifestImport = serde_json::from_value(value.clone())?;
    let mut tools = manifest
        .tools
        .into_iter()
        .map(|(fallback_id, tool)| manifest_tool_descriptor(fallback_id, tool))
        .collect::<Vec<_>>();
    tools.sort_by(|left, right| left.path.cmp(&right.path));
    let registry = ToolRegistry { tools };
    registry.validate()?;
    Ok(registry)
}

fn manifest_tool_descriptor(fallback_id: String, tool: TackToolImport) -> ToolDescriptor {
    let id = tool.id.unwrap_or(fallback_id);
    let namespace = tool
        .namespace_name
        .unwrap_or_else(|| fallback_namespace(&id));
    let name = tool.sdk_name.unwrap_or_else(|| fallback_name(&id));
    ToolDescriptor {
        path: format!("{namespace}.{name}"),
        tool_id: Some(id),
        name,
        title: None,
        description: tool.description,
        input_schema: tool
            .input_schema
            .unwrap_or_else(|| json!({"type":"object","additionalProperties":true})),
        output_schema: tool.output_schema.unwrap_or(Value::Null),
        tags: vec![namespace],
    }
}

fn fallback_namespace(id: &str) -> String {
    id.split('.')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or("tool")
        .to_string()
}

fn fallback_name(id: &str) -> String {
    id.rsplit('.')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(id)
        .to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TackManifestImport {
    #[serde(default)]
    tools: HashMap<String, TackToolImport>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TackToolImport {
    id: Option<String>,
    namespace_name: Option<String>,
    sdk_name: Option<String>,
    description: Option<String>,
    input_schema: Option<Value>,
    output_schema: Option<Value>,
}
