use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use tack_core::{FailurePhase, RuntimeFailure};

const MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "readonly",
    "abstract",
    "override",
    "declare",
];

pub(crate) fn transform_typescript(source: &str) -> Result<String, RuntimeFailure> {
    let wrapped_source = format!("(async function(){{\n{source}\n}})()");
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &wrapped_source, SourceType::ts()).parse();
    if !ret.errors.is_empty() {
        return Err(parse_failure(&ret.errors[0], &wrapped_source));
    }
    let mut stripper = Stripper {
        source: &wrapped_source,
        out: wrapped_source.as_bytes().to_vec(),
        unsupported: None,
    };
    stripper.visit_program(&ret.program);
    if let Some((span, syntax)) = stripper.unsupported {
        let error = OxcDiagnostic::error(format!(
            "{syntax} are not supported; only erasable TypeScript syntax is allowed"
        ))
        .with_label(span);
        return Err(parse_failure(&error, &wrapped_source));
    }
    Ok(String::from_utf8(stripper.out).expect("blanking keeps utf-8"))
}

fn parse_failure(error: &OxcDiagnostic, source: &str) -> RuntimeFailure {
    RuntimeFailure::new(
        FailurePhase::Parse,
        format!("{:?}", error.clone().with_source_code(source.to_string())),
    )
}

struct Stripper<'s> {
    source: &'s str,
    out: Vec<u8>,
    unsupported: Option<(Span, &'static str)>,
}

impl Stripper<'_> {
    fn blank(&mut self, start: u32, end: u32) {
        for byte in &mut self.out[start as usize..end as usize] {
            if !matches!(byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }

    fn erase(&mut self, span: Span) {
        self.blank(span.start, span.end);
        self.out[span.start as usize] = b';';
    }

    fn blank_next(&mut self, from: u32, marker: &str) {
        if let Some(offset) = self.source[from as usize..].find(marker) {
            let at = from + offset as u32;
            self.blank(at, at + marker.len() as u32);
        }
    }

    fn blank_type_suffix(&mut self, expression: Span, span: Span) {
        self.blank(expression.end, span.end);
        let rest = &self.source[span.end as usize..];
        let next = rest.trim_start();
        if rest[..rest.len() - next.len()].contains('\n') && next.starts_with(['(', '[', '`']) {
            self.out[expression.end as usize] = b';';
        }
    }

    fn blank_modifiers(&mut self, start: u32, end: u32) {
        let region = &self.source[start as usize..end as usize];
        let mut words = Vec::new();
        let mut word_start = None;
        for (index, ch) in region.char_indices().chain([(region.len(), ' ')]) {
            match (word_start, ch.is_alphanumeric() || ch == '_' || ch == '$') {
                (None, true) => word_start = Some(index),
                (Some(begin), false) => {
                    if MODIFIERS.contains(&&region[begin..index]) {
                        words.push((begin, index));
                    }
                    word_start = None;
                }
                _ => {}
            }
        }
        for (begin, finish) in words {
            self.blank(start + begin as u32, start + finish as u32);
        }
    }

    fn blank_member_modifiers(&mut self, span: Span, decorators: &[Decorator], key: &PropertyKey) {
        let start = decorators
            .last()
            .map_or(span.start, |decorator| decorator.span.end);
        self.blank_modifiers(start, key.span().start);
    }

    fn reject(&mut self, span: Span, syntax: &'static str) {
        self.unsupported.get_or_insert((span, syntax));
    }
}

impl<'a> Visit<'a> for Stripper<'_> {
    fn visit_ts_type_annotation(&mut self, it: &TSTypeAnnotation<'a>) {
        self.blank(it.span.start, it.span.end);
    }

    fn visit_ts_type_parameter_declaration(&mut self, it: &TSTypeParameterDeclaration<'a>) {
        self.blank(it.span.start, it.span.end);
    }

    fn visit_ts_type_parameter_instantiation(&mut self, it: &TSTypeParameterInstantiation<'a>) {
        self.blank(it.span.start, it.span.end);
    }

    fn visit_ts_this_parameter(&mut self, it: &TSThisParameter<'a>) {
        let rest = &self.source[it.span.end as usize..];
        let end = match rest.trim_start().strip_prefix(',') {
            Some(after) => self.source.len() - after.len(),
            None => it.span.end as usize,
        };
        self.blank(it.span.start, end as u32);
    }

    fn visit_ts_as_expression(&mut self, it: &TSAsExpression<'a>) {
        self.visit_expression(&it.expression);
        self.blank_type_suffix(it.expression.span(), it.span);
    }

    fn visit_ts_satisfies_expression(&mut self, it: &TSSatisfiesExpression<'a>) {
        self.visit_expression(&it.expression);
        self.blank_type_suffix(it.expression.span(), it.span);
    }

    fn visit_ts_non_null_expression(&mut self, it: &TSNonNullExpression<'a>) {
        self.visit_expression(&it.expression);
        self.blank(it.expression.span().end, it.span.end);
    }

    fn visit_ts_type_assertion(&mut self, it: &TSTypeAssertion<'a>) {
        self.blank(it.span.start, it.expression.span().start);
        self.visit_expression(&it.expression);
    }

    fn visit_ts_instantiation_expression(&mut self, it: &TSInstantiationExpression<'a>) {
        self.visit_expression(&it.expression);
        self.blank(it.expression.span().end, it.span.end);
    }

    fn visit_declaration(&mut self, it: &Declaration<'a>) {
        match it {
            _ if is_type_only_declaration(it) => self.erase(it.span()),
            Declaration::TSEnumDeclaration(decl) => self.reject(decl.span, "enums"),
            Declaration::TSModuleDeclaration(decl) => {
                self.reject(decl.span, "namespaces with values");
            }
            Declaration::TSImportEqualsDeclaration(decl) => {
                self.reject(decl.span, "import alias declarations");
            }
            _ => walk::walk_declaration(self, it),
        }
    }

    fn visit_module_declaration(&mut self, it: &ModuleDeclaration<'a>) {
        if is_type_only_module_declaration(it) {
            self.erase(it.span());
        } else {
            self.reject(it.span(), "import and export declarations");
        }
    }

    fn visit_decorator(&mut self, it: &Decorator<'a>) {
        self.reject(it.span, "decorators");
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        if it.r#abstract {
            self.blank_next(it.span.start, "abstract");
        }
        if let (Some(first), Some(last)) = (it.implements.first(), it.implements.last()) {
            let keyword = self.source[..first.span.start as usize]
                .rfind("implements")
                .expect("implements keyword") as u32;
            self.blank(keyword, last.span.end);
        }
        walk::walk_class(self, it);
    }

    fn visit_ts_class_implements(&mut self, _it: &TSClassImplements<'a>) {}

    fn visit_class_element(&mut self, it: &ClassElement<'a>) {
        let erasable = match it {
            ClassElement::TSIndexSignature(_) => true,
            ClassElement::MethodDefinition(method) => method.value.body.is_none(),
            ClassElement::PropertyDefinition(prop) => {
                prop.declare || prop.r#type == PropertyDefinitionType::TSAbstractPropertyDefinition
            }
            ClassElement::AccessorProperty(prop) => {
                prop.r#type == AccessorPropertyType::TSAbstractAccessorProperty
            }
            ClassElement::StaticBlock(_) => false,
        };
        if erasable {
            self.erase(it.span());
        } else {
            walk::walk_class_element(self, it);
        }
    }

    fn visit_method_definition(&mut self, it: &MethodDefinition<'a>) {
        self.blank_member_modifiers(it.span, &it.decorators, &it.key);
        if it.optional {
            self.blank_next(it.key.span().end, "?");
        }
        walk::walk_method_definition(self, it);
    }

    fn visit_property_definition(&mut self, it: &PropertyDefinition<'a>) {
        self.blank_member_modifiers(it.span, &it.decorators, &it.key);
        if it.optional {
            self.blank_next(it.key.span().end, "?");
        }
        if it.definite {
            self.blank_next(it.key.span().end, "!");
        }
        walk::walk_property_definition(self, it);
    }

    fn visit_accessor_property(&mut self, it: &AccessorProperty<'a>) {
        self.blank_member_modifiers(it.span, &it.decorators, &it.key);
        if it.definite {
            self.blank_next(it.key.span().end, "!");
        }
        walk::walk_accessor_property(self, it);
    }

    fn visit_formal_parameter(&mut self, it: &FormalParameter<'a>) {
        if it.accessibility.is_some() || it.readonly || it.r#override {
            self.reject(it.span, "parameter properties");
        }
        if it.optional {
            self.blank_next(it.pattern.span().end, "?");
        }
        walk::walk_formal_parameter(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if it.definite {
            self.blank_next(it.id.span().end, "!");
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        walk::walk_arrow_function_expression(self, it);
        if let Some(return_type) = &it.return_type {
            let params_end = it.params.span.end;
            if self.source[params_end as usize..return_type.span.end as usize].contains('\n') {
                self.blank(params_end - 1, params_end);
                self.out[return_type.span.end as usize - 1] = b')';
            }
        }
    }
}

fn is_type_only_statement(stmt: &Statement) -> bool {
    stmt.as_declaration().is_some_and(is_type_only_declaration)
        || stmt
            .as_module_declaration()
            .is_some_and(is_type_only_module_declaration)
}

fn is_type_only_declaration(decl: &Declaration) -> bool {
    match decl {
        Declaration::VariableDeclaration(decl) => decl.declare,
        Declaration::FunctionDeclaration(func) => func.declare || func.body.is_none(),
        Declaration::ClassDeclaration(class) => class.declare,
        Declaration::TSTypeAliasDeclaration(_)
        | Declaration::TSInterfaceDeclaration(_)
        | Declaration::TSGlobalDeclaration(_) => true,
        Declaration::TSEnumDeclaration(decl) => decl.declare,
        Declaration::TSModuleDeclaration(decl) => is_type_only_module(decl),
        Declaration::TSImportEqualsDeclaration(decl) => decl.import_kind.is_type(),
    }
}

fn is_type_only_module_declaration(decl: &ModuleDeclaration) -> bool {
    match decl {
        ModuleDeclaration::ImportDeclaration(decl) => decl.import_kind.is_type(),
        ModuleDeclaration::ExportAllDeclaration(decl) => decl.export_kind.is_type(),
        ModuleDeclaration::ExportNamedDeclaration(decl) => {
            decl.export_kind.is_type()
                || decl
                    .declaration
                    .as_ref()
                    .is_some_and(is_type_only_declaration)
        }
        _ => false,
    }
}

fn is_type_only_module(decl: &TSModuleDeclaration) -> bool {
    decl.declare
        || match &decl.body {
            None => true,
            Some(TSModuleDeclarationBody::TSModuleDeclaration(inner)) => is_type_only_module(inner),
            Some(TSModuleDeclarationBody::TSModuleBlock(block)) => {
                block.body.iter().all(is_type_only_statement)
            }
        }
}
