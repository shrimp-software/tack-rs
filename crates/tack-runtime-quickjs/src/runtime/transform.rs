use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_data_structures::code_buffer::IndentChar;
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{HelperLoaderMode, TransformOptions, Transformer};
use std::path::Path;
use tack_codemode::{FailurePhase, RuntimeFailure};

const TRANSFORM_PATH: &str = "codemode.ts";

pub(crate) fn transform_typescript(source: &str) -> Result<String, RuntimeFailure> {
    let wrapped_source = format!("(async function(){{\n{source}\n}})()");
    let allocator = Allocator::default();
    let path = Path::new(TRANSFORM_PATH);
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, &wrapped_source, source_type).parse();
    if !ret.errors.is_empty() {
        return Err(parse_failure(&ret.errors[0], &wrapped_source));
    }
    let mut program = ret.program;
    let semantic = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program);
    if !semantic.errors.is_empty() {
        return Err(parse_failure(&semantic.errors[0], &wrapped_source));
    }
    let mut options = TransformOptions::default();
    options.helper_loader.mode = HelperLoaderMode::External;
    let transform = Transformer::new(&allocator, path, &options)
        .build_with_scoping(semantic.semantic.into_scoping(), &mut program);
    if !transform.errors.is_empty() {
        return Err(parse_failure(&transform.errors[0], &wrapped_source));
    }
    let code = Codegen::new()
        .with_options(CodegenOptions {
            indent_char: IndentChar::Space,
            indent_width: 2,
            ..CodegenOptions::default()
        })
        .build(&program)
        .code;
    Ok(code)
}

fn parse_failure(error: &OxcDiagnostic, source: &str) -> RuntimeFailure {
    RuntimeFailure::new(
        FailurePhase::Parse,
        format!("{:?}", error.clone().with_source_code(source.to_string())),
    )
}
