use lexer::lex;
use parser::Parser;
use typecheck::typechecker::TypeChecker;

fn compile_file(path: &str) {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|_| panic!("Could not read {}", path));
    let tokens = lex(&source).unwrap_or_else(|e| panic!("Lex error in {}: {}", path, e));
    let mut parser = Parser::new(tokens);
    let module = parser
        .parse_module("test")
        .unwrap_or_else(|e| panic!("Parse error in {}: {}", path, e));
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
        .unwrap_or_else(|e| panic!("Type error in {}: {}", path, e));
}

#[test]
fn example_01_literals() {
    compile_file("examples/01_literals.aster");
}

#[test]
fn example_02_expressions_and_binary_ops() {
    compile_file("examples/02_expressions_and_binary_ops.aster");
}

#[test]
fn example_03_simple_function() {
    compile_file("examples/03_simple_function.aster");
}

#[test]
fn example_04_functions_using_vars() {
    compile_file("examples/04_functions_using_vars.aster");
}

#[test]
fn example_hello() {
    compile_file("examples/hello.aster");
}

#[test]
fn example_05_operators_and_precedence() {
    compile_file("examples/05_operators_and_precedence.aster");
}

#[test]
fn example_06_comparisons_and_logic() {
    compile_file("examples/06_comparisons_and_logic.aster");
}

#[test]
fn example_07_mixed_expressions() {
    compile_file("examples/07_mixed_expressions.aster");
}

#[test]
fn example_08_float_promotion() {
    compile_file("examples/08_float_promotion.aster");
}

#[test]
fn example_09_collections() {
    compile_file("examples/09_collections.aster");
}

#[test]
fn example_10_modules_and_builtins() {
    compile_file("examples/10_modules_and_builtins.aster");
}

#[test]
fn example_11_generics_and_traits() {
    compile_file("examples/11_generics_and_traits.aster");
}

#[test]
fn example_12_async_errors_matching() {
    compile_file("examples/12_async_errors_matching.aster");
}

#[test]
fn example_13_throws_and_extends() {
    compile_file("examples/13_throws_and_extends.aster");
}
