mod common;

#[test]
fn build_executes_hello_example() {
    let output = common::build_and_run("examples/executable/hello.aster");
    assert!(output.status.success(), "{}", common::output_text(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello\nYes\n");
}
