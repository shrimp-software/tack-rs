use tack_codemode::{FailurePhase, RuntimeFailure, RuntimeLimitsError, RuntimeOutput};

pub(crate) fn runtime_limits_error(error: RuntimeLimitsError) -> RuntimeFailure {
    RuntimeFailure::new(FailurePhase::Runtime, error.to_string())
}

pub(crate) fn runtime_error(error: rquickjs::Error) -> RuntimeFailure {
    RuntimeFailure::new(FailurePhase::Runtime, error.to_string())
}

pub(crate) fn enforce_output_limit(
    output: &RuntimeOutput,
    max_output_bytes: usize,
) -> Result<(), RuntimeFailure> {
    let bytes = serde_json::to_vec(output).map_err(|error| {
        RuntimeFailure::new(
            FailurePhase::Runtime,
            format!("execution output is not JSON serializable: {error}"),
        )
        .with_output(output.emitted.clone(), output.logs.clone())
    })?;
    if bytes.len() > max_output_bytes {
        return Err(RuntimeFailure::new(
            FailurePhase::Runtime,
            format!("execution output exceeded {max_output_bytes} bytes"),
        )
        .with_output(output.emitted.clone(), output.logs.clone()));
    }
    Ok(())
}
