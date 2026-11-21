use crate::runtime::transform_typescript;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use tack_core::FailurePhase;

fn strip(source: &str) -> String {
    let wrapped = format!("(async function(){{\n{source}\n}})()");
    let code = transform_typescript(source).unwrap();
    assert_eq!(code.len(), wrapped.len());
    assert!(
        code.match_indices('\n')
            .map(|(at, _)| at)
            .eq(wrapped.match_indices('\n').map(|(at, _)| at))
    );
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &code, SourceType::cjs()).parse();
    assert!(ret.errors.is_empty(), "{code}\n{:?}", ret.errors);
    code
}

fn body(source: &str) -> String {
    let code = strip(source);
    code["(async function(){\n".len()..code.len() - "\n})()".len()].to_string()
}

fn rejected(source: &str) -> String {
    let failure = transform_typescript(source).unwrap_err();
    assert_eq!(failure.phase, FailurePhase::Parse);
    failure.message
}

#[test]
fn keeps_the_function_wrapper() {
    assert_eq!(
        strip("const value: number = 2; return value;"),
        "(async function(){\nconst value         = 2; return value;\n})()"
    );
}

#[test]
fn strips_annotations_and_generics() {
    assert_eq!(
        body("function f<T>(this: Window, a: T, b?: number): T { return a } return f<number>(1)"),
        "function f   (              a   , b         )    { return a } return f        (1)"
    );
    assert_eq!(
        body("try {} catch (e: unknown) {} return { m(a?: number): void {} }"),
        "try {} catch (e         ) {} return { m(a         )       {} }"
    );
}

#[test]
fn strips_expression_syntax() {
    assert_eq!(
        body(
            "const a = (x as any)!.y satisfies unknown; const b = <string>x; const g = f<string>;"
        ),
        "const a = (x       ) .y                  ; const b =         x; const g = f        ;"
    );
    assert_eq!(
        body("let c!: number; return new Map<string, number>()"),
        "let c         ; return new Map                ()"
    );
}

#[test]
fn erases_type_only_statements_without_joining_lines() {
    assert_eq!(
        body("let x = 1\ntype A = string\ninterface I<T> { x: T }\n[1].forEach(f)"),
        "let x = 1\n;              \n;                      \n[1].forEach(f)"
    );
    assert_eq!(
        body("declare const d: number; declare function df(): void; declare enum E { A }"),
        ";                        ;                            ;                   "
    );
    assert_eq!(
        body("namespace N { export type X = 1 } declare global {}"),
        ";                                 ;                "
    );
    assert_eq!(
        body("import type { A } from 'a'\nexport type { B }"),
        ";                         \n;                "
    );
    assert_eq!(
        body("function o(a: string): void;\nfunction o(a: any) {}"),
        ";                           \nfunction o(a     ) {}"
    );
}

#[test]
fn strips_class_syntax() {
    let code = body(
        "abstract class B<T> extends A<T> implements I, J<T> {
  private readonly x: number = 1;
  declare y: string;
  abstract z: number;
  static override w?: number;
  q!: string;
  [k: string]: any;
  public static async *gen?(): AsyncGenerator<number> { yield 1 }
  over(a: string): void;
  abstract m(): void;
  protected get g(): number { return 1 }
  accessor acc!: number;
}",
    );
    assert_eq!(
        code.lines().map(str::trim_end).collect::<Vec<_>>(),
        [
            "         class B    extends A                       {",
            "                   x         = 1;",
            "  ;",
            "  ;",
            "  static          w         ;",
            "  q         ;",
            "  ;",
            "         static async *gen ()                         { yield 1 }",
            "  ;",
            "  ;",
            "            get g()         { return 1 }",
            "  accessor acc         ;",
            "}",
        ]
    );
}

#[test]
fn keeps_multiline_arrow_return_types_valid() {
    assert_eq!(
        body("const h = async <T,>(x: T)\n  : Promise<T> => x;"),
        "const h = async     (x    \n             ) => x;"
    );
}

#[test]
fn rejects_syntax_that_needs_code_generation() {
    for (source, syntax) in [
        ("enum E { A }", "enums"),
        ("const enum E { A }", "enums"),
        (
            "namespace N { export const x = 1 }",
            "namespaces with values",
        ),
        (
            "class C { constructor(private x: number) {} }",
            "parameter properties",
        ),
        (
            "class C { constructor(readonly x: number) {} }",
            "parameter properties",
        ),
        ("@dec class C {}", "decorators"),
        ("class C { @dec m() {} }", "decorators"),
        ("import x = require('x')", "import alias declarations"),
        ("import { a } from 'a'", "import and export declarations"),
        ("export const a = 1", "import and export declarations"),
    ] {
        let message = rejected(source);
        assert!(
            message.contains(&format!("{syntax} are not supported")),
            "{source}: {message}"
        );
    }
}

#[test]
fn reports_parse_errors() {
    assert!(rejected("const x: = 1").contains("Unexpected token"));
}
