use crate::runtime::{CALL_JSON, EMIT_JSON, LOG_LINE, runtime_error};
use rquickjs::{
    Ctx,
    prelude::{Async, Func},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tack_codemode::{RuntimeFailure, RuntimeInput, ToolCall, ToolCallOutput, ToolInvoker};
use tokio::time;

use super::CapturedOutput;

pub(crate) fn install_host_api(
    ctx: &Ctx<'_>,
    input: &RuntimeInput,
    invoker: Arc<dyn ToolInvoker>,
    call_count: Arc<Mutex<usize>>,
    captured: CapturedOutput,
) -> Result<(), RuntimeFailure> {
    let global = ctx.globals();
    let captured_for_emit = captured.clone();
    global
        .set(
            EMIT_JSON,
            Func::from(move |value: String| {
                captured_for_emit.emit_json(&value);
            }),
        )
        .map_err(runtime_error)?;

    let captured_for_log = captured.clone();
    global
        .set(
            LOG_LINE,
            Func::from(move |value: String| {
                captured_for_log.log_line(value);
            }),
        )
        .map_err(runtime_error)?;

    let handler = ToolCallHandler::new(invoker, call_count, input);
    global
        .set(
            CALL_JSON,
            Func::from(Async(move |path: String, input_json: String| {
                let handler = handler.clone();
                async move { handler.call(path, input_json).await }
            })),
        )
        .map_err(runtime_error)
}

#[derive(Clone)]
struct ToolCallHandler {
    invoker: Arc<dyn ToolInvoker>,
    call_count: Arc<Mutex<usize>>,
    max_calls: usize,
    tool_call_timeout: std::time::Duration,
    max_request: usize,
    max_response: usize,
}

impl ToolCallHandler {
    fn new(
        invoker: Arc<dyn ToolInvoker>,
        call_count: Arc<Mutex<usize>>,
        input: &RuntimeInput,
    ) -> Self {
        Self {
            invoker,
            call_count,
            max_calls: input.limits.max_tool_calls,
            tool_call_timeout: input.limits.tool_call_timeout(),
            max_request: input.limits.max_tool_request_bytes,
            max_response: input.limits.max_tool_response_bytes,
        }
    }

    async fn call(&self, path: String, input_json: String) -> String {
        match self.build_call(path, input_json) {
            Ok(call) => self.invoke(call).await,
            Err(message) => serialize_tool_output(&ToolCallOutput::failure(message)),
        }
    }

    fn build_call(&self, path: String, input_json: String) -> Result<ToolCall, String> {
        self.reserve_call()?;
        if input_json.len() > self.max_request {
            return Err("tool request too large".to_string());
        }
        let input = serde_json::from_str::<Value>(&input_json)
            .map_err(|error| format!("tool request is not valid JSON: {error}"))?;
        Ok(ToolCall { path, input })
    }

    fn reserve_call(&self) -> Result<(), String> {
        let mut count = self.call_count.lock().expect("call count lock");
        *count += 1;
        if *count > self.max_calls {
            Err("tool call limit exceeded".to_string())
        } else {
            Ok(())
        }
    }

    async fn invoke(&self, call: ToolCall) -> String {
        let output = match time::timeout(self.tool_call_timeout, self.invoker.call(call)).await {
            Ok(output) => output,
            Err(_) => ToolCallOutput::failure("tool call timed out"),
        };
        let output_json = serialize_tool_output(&output);
        if output_json.len() > self.max_response {
            serialize_tool_output(&ToolCallOutput::failure("tool response too large"))
        } else {
            output_json
        }
    }
}

fn serialize_tool_output(output: &ToolCallOutput) -> String {
    serde_json::to_string(output).unwrap_or_else(|error| {
        json!({"ok":false,"error":{"message":error.to_string()}}).to_string()
    })
}
