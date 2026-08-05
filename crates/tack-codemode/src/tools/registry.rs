use crate::ident::is_js_identifier;
use crate::tools::{ManifestImportError, ToolPolicy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use super::paths::BUILTIN_TOOL_PATHS;

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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolRegistry {
    /// Ordered tool descriptors exposed to snippets.
    ///
    /// The registry intentionally keeps descriptors in a plain vector because
    /// hosts commonly serialize, filter, and display the complete list. Call
    /// [`ToolRegistry::validate`] after direct mutation to enforce path
    /// invariants before execution.
    pub tools: Vec<ToolDescriptor>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_tools(tools: Vec<ToolDescriptor>) -> Result<Self, RegistryValidationError> {
        let registry = Self { tools };
        registry.validate()?;
        Ok(registry)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ToolDescriptor> {
        self.tools.iter()
    }

    pub fn push(&mut self, tool: ToolDescriptor) -> Result<(), RegistryValidationError> {
        validate_tool_path(&tool.path)?;
        if self.contains_path(&tool.path) {
            return Err(RegistryValidationError::DuplicateToolPath(tool.path));
        }
        self.tools.push(tool);
        Ok(())
    }

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
        super::manifest::registry_from_manifest_value(value)
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
