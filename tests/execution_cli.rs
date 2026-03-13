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
    // Spec file with trait definitions at top level — still not executable
    let output = common::cli(&["run", "examples/spec/11_generics_and_traits.aster"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        common::output_text(&output)
    );

    let text = common::output_text(&output);
    assert!(text.contains("not executable yet"), "{text}");
    assert!(text.contains("top-level `trait`"), "{text}");
    assert!(!text.contains("Discriminant("), "{text}");
}

#[test]
fn run_top_level_control_flow() {
    // Top-level if/while/for should execute
    let dir = common::make_temp_dir("tl-ctrl");
    let src = dir.join("top_level.aster");
    std::fs::write(
        &src,
        "\
let x = 0
let total = 0
while x < 5
  total = total + x
  x = x + 1

if total > 5
  total = total + 100

def main() -> Int
  total
",
    )
    .unwrap();
    let output = common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(110),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn build_and_run_top_level_for() {
    let dir = common::make_temp_dir("tl-for");
    let src = dir.join("for_loop.aster");
    std::fs::write(
        &src,
        "\
let nums = [10, 20, 12]
let total = 0
for n in nums
  total = total + n

def main() -> Int
  total
",
    )
    .unwrap();
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(42),
        "{}",
        common::output_text(&output)
    );
}
