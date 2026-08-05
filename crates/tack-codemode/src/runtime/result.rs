use super::{RuntimeFailure, RuntimeOutput};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default)]
    pub emitted: Vec<Value>,
    #[serde(default)]
    pub logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RuntimeFailure>,
}

impl ExecutionResult {
    pub fn ok(output: RuntimeOutput) -> Self {
        Self {
            ok: true,
            result: Some(output.result),
            emitted: output.emitted,
            logs: output.logs,
            error: None,
        }
    }

    pub fn failure(error: RuntimeFailure) -> Self {
        Self {
            ok: false,
            result: None,
            emitted: error.emitted.clone(),
            logs: error.logs.clone(),
            error: Some(error),
        }
    }
}
