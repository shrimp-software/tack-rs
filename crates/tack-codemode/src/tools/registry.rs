use crate::ident::is_js_identifier;
use crate::tools::ToolPolicy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub output_schema: Value,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl ToolDescriptor {
    pub fn new(path: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            tool_id: None,
            name: name.into(),
            title: None,
            description: None,
            input_schema: json!({"type":"object","properties":{},"additionalProperties":true}),
            output_schema: Value::Null,
            tags: Vec::new(),
        }
    }
}

const BUILTIN_TOOL_PATHS: &[&str] = &["tools.search", "tools.describe.tool", "tools.invoke"];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolRegistry {
    pub tools: Vec<ToolDescriptor>,
}

impl ToolRegistry {
    pub fn validate(&self) -> Result<(), RegistryValidationError> {
        let mut paths = self
            .tools
            .iter()
            .map(|tool| tool.path.as_str())
            .collect::<Vec<_>>();
        paths.sort_unstable();
        if let Some(duplicate) = paths.windows(2).find(|paths| paths[0] == paths[1]) {
            return Err(RegistryValidationError::DuplicateToolPath(
                duplicate[0].to_string(),
            ));
        }
        for tool in &self.tools {
            validate_tool_path(&tool.path)?;
        }
        Ok(())
    }

    pub fn by_path(&self, path: &str) -> Option<&ToolDescriptor> {
        self.tools.iter().find(|tool| tool.path == path)
    }

    pub fn contains_path(&self, path: &str) -> bool {
        self.by_path(path).is_some()
    }

    pub fn filtered_by_policy(&self, policy: &dyn ToolPolicy) -> Self {
        Self {
            tools: self
                .tools
                .iter()
                .filter(|tool| policy.is_allowed(&tool.path))
                .cloned()
                .collect(),
        }
    }

    pub fn from_manifest_value(value: &Value) -> Result<Self, ManifestImportError> {
        let manifest: TackManifestImport = serde_json::from_value(value.clone())?;
        let mut tools = Vec::with_capacity(manifest.tools.len());
        for (fallback_id, tool) in manifest.tools {
            let id = tool.id.unwrap_or(fallback_id);
            let namespace = tool.namespace_name.unwrap_or_else(|| {
                id.split('.')
                    .next()
                    .filter(|part| !part.is_empty())
                    .unwrap_or("tool")
                    .to_string()
            });
            let name = tool.sdk_name.unwrap_or_else(|| {
                id.rsplit('.')
                    .next()
                    .filter(|part| !part.is_empty())
                    .unwrap_or(id.as_str())
                    .to_string()
            });
            tools.push(ToolDescriptor {
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
            });
        }
        tools.sort_by(|left, right| left.path.cmp(&right.path));
        let registry = Self { tools };
        registry.validate()?;
        Ok(registry)
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RegistryValidationError {
    #[error("tool path must not be empty")]
    EmptyToolPath,
    #[error("tool path `{path}` has invalid segment `{segment}`")]
    InvalidToolPath { path: String, segment: String },
    #[error("tool path `{0}` is reserved")]
    ReservedToolPath(String),
    #[error("tool registry contains duplicate path `{0}`")]
    DuplicateToolPath(String),
}

fn validate_tool_path(path: &str) -> Result<(), RegistryValidationError> {
    if path.is_empty() {
        return Err(RegistryValidationError::EmptyToolPath);
    }
    if path.starts_with("__tack")
        || BUILTIN_TOOL_PATHS.contains(&path)
        || path.starts_with("tools.")
    {
        return Err(RegistryValidationError::ReservedToolPath(path.to_string()));
    }
    for segment in path.split('.') {
        if !is_js_identifier(segment) {
            return Err(RegistryValidationError::InvalidToolPath {
                path: path.to_string(),
                segment: segment.to_string(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ManifestImportError {
    #[error("invalid Tack manifest: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid Tack registry: {0}")]
    Registry(#[from] RegistryValidationError),
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
