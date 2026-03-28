use std::collections::HashMap;

// ─── std/unstable module + --unstable flag ──────────────────────────
//
// The std/unstable virtual module gates experimental features behind
// the --unstable compiler flag (or ASTER_UNSTABLE=1 env var).
//
// These tests validate the gating mechanism itself, not the contents
// of the unstable module (which is currently empty, awaiting
// FieldAccessible in Feature 3).

// ─── Contract tests: the gate exists and works ──────────────────────

#[test]
fn use_std_unstable_rejected_without_flag() {
    // Importing std/unstable without --unstable must produce a compile error.
    let err = crate::common::check_err_with_files("use std/unstable\n", HashMap::new());
    assert!(
        err.contains("--unstable"),
        "Expected error mentioning --unstable flag, got: {}",
        err
    );
}

#[test]
fn use_std_unstable_selective_rejected_without_flag() {
    // Selective import from std/unstable should also be rejected.
    let err =
        crate::common::check_err_with_files("use std/unstable { SomeFeature }\n", HashMap::new());
    assert!(
        err.contains("--unstable"),
        "Expected error mentioning --unstable flag, got: {}",
        err
    );
}

#[test]
fn use_std_unstable_namespace_rejected_without_flag() {
    // Namespace import from std/unstable should also be rejected.
    let err = crate::common::check_err_with_files("use std/unstable as u\n", HashMap::new());
    assert!(
        err.contains("--unstable"),
        "Expected error mentioning --unstable flag, got: {}",
        err
    );
}

// ─── Happy path: --unstable allows the import ───────────────────────

#[test]
fn use_std_unstable_allowed_with_flag() {
    // With --unstable enabled, `use std/unstable` should succeed.
    crate::common::check_ok_with_files_unstable("use std/unstable\n", HashMap::new());
}

#[test]
fn use_std_unstable_selective_allowed_with_flag() {
    // Selective import from std/unstable with --unstable enabled.
    // Currently the module is empty, so importing a specific name should
    // fail with "not exported", not with the unstable gate error.
    let err = crate::common::check_err_with_files_unstable(
        "use std/unstable { SomeFeature }\n",
        HashMap::new(),
    );
    // Should NOT mention --unstable (the gate is open).
    assert!(
        !err.contains("--unstable"),
        "Should not mention --unstable when flag is enabled, got: {}",
        err
    );
    // Should mention the symbol not being exported.
    assert!(
        err.contains("SomeFeature") && err.contains("not"),
        "Expected 'not exported' error for unknown symbol, got: {}",
        err
    );
}

// ─── Error message quality ──────────────────────────────────────────

#[test]
fn unstable_error_has_m005_code() {
    let diag = crate::common::check_err_diagnostic_with_files("use std/unstable\n", HashMap::new());
    assert_eq!(
        diag.code(),
        Some("M005"),
        "Expected error code M005, got: {:?}",
        diag.code()
    );
}

#[test]
fn unstable_error_mentions_env_var() {
    // The error message should mention ASTER_UNSTABLE as an alternative.
    let err = crate::common::check_err_with_files("use std/unstable\n", HashMap::new());
    assert!(
        err.contains("ASTER_UNSTABLE"),
        "Expected error to mention ASTER_UNSTABLE env var, got: {}",
        err
    );
}

// ─── Propagation: imported modules inherit the flag ─────────────────

#[test]
fn imported_module_can_use_unstable_when_root_has_flag() {
    // If the root module is compiled with --unstable, modules it imports
    // should also be able to use std/unstable.
    let mut files = HashMap::new();
    files.insert(
        "experimental".to_string(),
        "use std/unstable\npub let FEATURE = 1\n".to_string(),
    );
    crate::common::check_ok_with_files_unstable(
        "use experimental { FEATURE }\nlet x = FEATURE\n",
        files,
    );
}

#[test]
fn imported_module_cannot_use_unstable_without_root_flag() {
    // Without --unstable on the root, imported modules should also fail.
    let mut files = HashMap::new();
    files.insert(
        "experimental".to_string(),
        "use std/unstable\npub let FEATURE = 1\n".to_string(),
    );
    let err = crate::common::check_err_with_files(
        "use experimental { FEATURE }\nlet x = FEATURE\n",
        files,
    );
    assert!(
        err.contains("--unstable"),
        "Expected --unstable error from imported module, got: {}",
        err
    );
}

// ─── Composition: stable std imports still work alongside ───────────

#[test]
fn stable_std_imports_unaffected_by_unstable_flag() {
    // Having --unstable enabled should not break normal std imports.
    crate::common::check_ok_with_files_unstable(
        "use std/cmp { Eq }\n\nclass Point includes Eq\n  x: Int\n  y: Int\n",
        HashMap::new(),
    );
}

#[test]
fn stable_std_imports_work_without_unstable() {
    // Normal std imports should continue to work without --unstable.
    crate::common::check_ok_with_files(
        "use std/cmp { Eq }\n\nclass Point includes Eq\n  x: Int\n  y: Int\n",
        HashMap::new(),
    );
}

// ─── Rejection: unknown std submodules still rejected ───────────────

#[test]
fn unknown_std_submodule_still_rejected_with_unstable() {
    // --unstable should not make arbitrary std submodules valid.
    let err = crate::common::check_err_with_files_unstable("use std/nonexistent\n", HashMap::new());
    assert!(
        !err.contains("--unstable"),
        "Unknown submodule error should not mention --unstable, got: {}",
        err
    );
}

// ─── Non-std module named "unstable" is not gated ───────────────────

#[test]
fn user_module_named_unstable_not_gated() {
    // A user module whose path contains "unstable" should NOT be gated
    // by the --unstable flag. Only std/unstable is special.
    let mut files = HashMap::new();
    files.insert("mylib/unstable".to_string(), "pub let X = 42\n".to_string());
    crate::common::check_ok_with_files("use mylib/unstable { X }\nlet y = X\n", files);
}

// ─── CLI integration: --unstable flag on check/run/build ────────────

#[test]
fn cli_check_unstable_flag_accepted() {
    // asterc check --unstable <file> should be a valid invocation.
    // Write a file that uses std/unstable and check it passes with the flag.
    let dir = crate::common::make_temp_dir("unstable-cli");
    let file = dir.join("test.aster");
    std::fs::write(&file, "use std/unstable\ndef main() -> Int\n  0\n").unwrap();

    let output = crate::common::cli(&["check", "--unstable", file.to_str().unwrap()]);
    let text = crate::common::output_text(&output);
    assert!(
        output.status.success(),
        "asterc check --unstable should succeed, got: {}",
        text
    );
}

#[test]
fn cli_check_without_unstable_flag_rejects() {
    // asterc check <file> (no --unstable) should fail for std/unstable imports.
    let dir = crate::common::make_temp_dir("unstable-cli-reject");
    let file = dir.join("test.aster");
    std::fs::write(&file, "use std/unstable\ndef main() -> Int\n  0\n").unwrap();

    let output = crate::common::cli(&["check", file.to_str().unwrap()]);
    let text = crate::common::output_text(&output);
    assert!(
        !output.status.success(),
        "asterc check without --unstable should fail, got: {}",
        text
    );
    assert!(
        text.contains("--unstable"),
        "Error output should mention --unstable, got: {}",
        text
    );
}

#[test]
fn cli_env_var_enables_unstable() {
    // ASTER_UNSTABLE=1 should enable unstable imports without the CLI flag.
    let dir = crate::common::make_temp_dir("unstable-env");
    let file = dir.join("test.aster");
    std::fs::write(&file, "use std/unstable\ndef main() -> Int\n  0\n").unwrap();

    let output = std::process::Command::new(
        std::env::var_os("CARGO_BIN_EXE_asterc")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("target/debug/asterc")),
    )
    .args(["check", file.to_str().unwrap()])
    .env("ASTER_UNSTABLE", "1")
    .output()
    .expect("failed to run asterc");

    let text = crate::common::output_text(&output);
    assert!(
        output.status.success(),
        "ASTER_UNSTABLE=1 should enable unstable imports, got: {}",
        text
    );
}
