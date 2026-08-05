use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_data_structures::code_buffer::IndentChar;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{HelperLoaderMode, TransformOptions, Transformer};
use std::path::Path;
use tack_codemode::{FailurePhase, RuntimeFailure};

pub(crate) fn transform_typescript(source: &str) -> Result<String, RuntimeFailure> {
    let wrapped_source = format!("(async function(){{\n{source}\n}})()");
    let allocator = Allocator::default();
    let path = Path::new("codemode.ts");
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, &wrapped_source, source_type).parse();
    if !ret.errors.is_empty() {
        return Err(RuntimeFailure::new(
            FailurePhase::Parse,
            format!(
                "{:?}",
                ret.errors[0]
                    .clone()
                    .with_source_code(wrapped_source.clone())
            ),
        ));
    }
    let mut program = ret.program;
    let semantic = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program);
    if !semantic.errors.is_empty() {
        return Err(RuntimeFailure::new(
            FailurePhase::Parse,
            format!(
                "{:?}",
                semantic.errors[0]
                    .clone()
                    .with_source_code(wrapped_source.clone())
            ),
        ));
    }
    let mut options = TransformOptions::default();
    options.helper_loader.mode = HelperLoaderMode::External;
    let transform = Transformer::new(&allocator, path, &options)
        .build_with_scoping(semantic.semantic.into_scoping(), &mut program);
    if !transform.errors.is_empty() {
        return Err(RuntimeFailure::new(
            FailurePhase::Parse,
            format!(
                "{:?}",
                transform.errors[0]
                    .clone()
                    .with_source_code(wrapped_source.clone())
            ),
        ));
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
