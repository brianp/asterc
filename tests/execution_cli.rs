mod common;

#[test]
fn run_executes_hello_example() {
    let output = common::cli(&["run", "examples/executable/hello.aster"]);
    assert!(output.status.success(), "{}", common::output_text(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello\nYes\n");
}

#[test]
fn run_returns_exit_code_from_main() {
    let output = common::cli(&["run", "examples/executable/fibonacci.aster"]);
    assert_eq!(
        output.status.code(),
        Some(55),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_reports_not_executable_yet_for_spec_example() {
    let output = common::cli(&["run", "examples/spec/09_collections.aster"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        common::output_text(&output)
    );

    let text = common::output_text(&output);
    assert!(text.contains("not executable yet"), "{text}");
    assert!(text.contains("top-level `for`"), "{text}");
    assert!(!text.contains("Discriminant("), "{text}");
}
