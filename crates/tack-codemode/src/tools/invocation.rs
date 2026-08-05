use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub path: String,
    #[serde(default)]
    pub input: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallOutput {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolCallError>,
}

impl ToolCallOutput {
    pub fn data(data: Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            text: None,
            raw: None,
            error: None,
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self::failure_with_code(message, None)
    }

    pub fn failure_with_code(message: impl Into<String>, code: Option<String>) -> Self {
        Self {
            ok: false,
            data: None,
            text: None,
            raw: None,
            error: Some(ToolCallError {
                message: message.into(),
                code,
                details: None,
            }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[async_trait]
pub trait ToolInvoker: Send + Sync {
    async fn call(&self, call: ToolCall) -> ToolCallOutput;
}

pub trait ToolPolicy: Send + Sync {
    fn is_allowed(&self, path: &str) -> bool;
}
