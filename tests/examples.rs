mod common;

#[test]
fn example_01_literals() {
    common::compile_file("examples/01_literals.aster");
}

#[test]
fn example_02_expressions_and_binary_ops() {
    common::compile_file("examples/02_expressions_and_binary_ops.aster");
}

#[test]
fn example_03_simple_function() {
    common::compile_file("examples/03_simple_function.aster");
}

#[test]
fn example_04_functions_using_vars() {
    common::compile_file("examples/04_functions_using_vars.aster");
}

#[test]
fn example_hello() {
    common::compile_file("examples/executable/hello.aster");
}

#[test]
fn example_05_operators_and_precedence() {
    common::compile_file("examples/05_operators_and_precedence.aster");
}

#[test]
fn example_06_comparisons_and_logic() {
    common::compile_file("examples/06_comparisons_and_logic.aster");
}

#[test]
fn example_07_mixed_expressions() {
    common::compile_file("examples/07_mixed_expressions.aster");
}

#[test]
fn example_08_float_promotion() {
    common::compile_file("examples/08_float_promotion.aster");
}

#[test]
fn example_09_collections() {
    common::compile_file("examples/spec/09_collections.aster");
}

#[test]
fn example_10_modules_and_builtins() {
    common::compile_file("examples/spec/10_modules_and_builtins.aster");
}

#[test]
fn example_11_generics_and_traits() {
    common::compile_file("examples/spec/11_generics_and_traits.aster");
}

#[test]
fn example_12_async_errors_matching() {
    common::compile_file("examples/spec/12_async_errors_matching.aster");
}

#[test]
fn example_13_throws_and_extends() {
    common::compile_file("examples/spec/13_throws_and_extends.aster");
}
