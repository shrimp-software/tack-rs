use crate::ident::is_js_identifier;
use crate::tools::ToolPolicy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use super::paths::BUILTIN_TOOL_PATHS;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_id: Option<String>,
    name: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_schema: Value,
    #[serde(default)]
    output_schema: Value,
    #[serde(default)]
    tags: Vec<String>,
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

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn tool_id(&self) -> Option<&str> {
        self.tool_id.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn input_schema(&self) -> &Value {
        &self.input_schema
    }

    pub fn output_schema(&self) -> &Value {
        &self.output_schema
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn with_tool_id(mut self, tool_id: impl Into<String>) -> Self {
        self.tool_id = Some(tool_id.into());
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_input_schema(mut self, input_schema: Value) -> Self {
        self.input_schema = input_schema;
        self
    }

    pub fn with_output_schema(mut self, output_schema: Value) -> Self {
        self.output_schema = output_schema;
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn with_tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tags.extend(tags.into_iter().map(Into::into));
        self
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolRegistry {
    tools: Vec<ToolDescriptor>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_tools(tools: Vec<ToolDescriptor>) -> Result<Self, RegistryValidationError> {
        let mut registry = Self::new();
        registry.extend(tools)?;
        Ok(registry)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ToolDescriptor> {
        self.tools.iter()
    }

    pub fn as_slice(&self) -> &[ToolDescriptor] {
        &self.tools
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn push(&mut self, tool: ToolDescriptor) -> Result<(), RegistryValidationError> {
        self.extend([tool])
    }

    pub fn extend<I>(&mut self, tools: I) -> Result<(), RegistryValidationError>
    where
        I: IntoIterator<Item = ToolDescriptor>,
    {
        let tools = tools.into_iter().collect::<Vec<_>>();
        validate_tool_batch(&self.tools, &tools)?;
        self.tools.extend(tools);
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

fn validate_tool_batch(
    existing: &[ToolDescriptor],
    added: &[ToolDescriptor],
) -> Result<(), RegistryValidationError> {
    for tool in added {
        validate_tool_path(&tool.path)?;
    }
    let mut paths = existing
        .iter()
        .chain(added.iter())
        .map(|tool| tool.path.as_str())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    if let Some(duplicate) = paths.windows(2).find(|paths| paths[0] == paths[1]) {
        return Err(RegistryValidationError::DuplicateToolPath(
            duplicate[0].to_string(),
        ));
    }
    Ok(())
}
