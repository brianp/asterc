mod build_dir;
mod manifest;

use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use ariadne::{Color, Label, Report, ReportKind, Source};

use ast::{Diagnostic, Severity};
use codegen::config::{BuildConfig, OptLevel, Profile};
use fir::lower::LowerError;
use lexer::lex;
use parser::Parser;
use typecheck::module_loader::{FsResolver, ModuleLoader};
use typecheck::typechecker::TypeChecker;

use crate::build_dir::resolve_build_paths;
use crate::manifest::{BuildManifest, sha256_hex};

fn cc_compiler() -> String {
    env::var("CC").unwrap_or_else(|_| "cc".into())
}

fn render_diagnostic(source: &str, filename: &str, diag: &Diagnostic) {
    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Hint => ReportKind::Advice,
    };

    let offset = diag.labels.first().map(|l| l.span.start).unwrap_or(0);

    let mut report = Report::build(kind, filename, offset);

    if let Some(ref code) = diag.code {
        report = report.with_code(code);
    }

    report = report.with_message(&diag.message);

    for (i, label) in diag.labels.iter().enumerate() {
        let color = if i == 0 { Color::Red } else { Color::Blue };
        report = report.with_label(
            Label::new((filename, label.span.start..label.span.end))
                .with_message(&label.message)
                .with_color(color),
        );
    }

    for note in &diag.notes {
        report = report.with_note(note);
    }

    let _ = report.finish().eprint((filename, Source::from(source)));
}

fn print_usage() {
    eprintln!("Usage: asterc <command> <file.aster>");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  check <file>                Type-check a source file");
    eprintln!("  run <file>                  Compile and execute via JIT");
    eprintln!("  build <file> [options]      Compile to a native executable");
    eprintln!("  fmt [file] [options]        Format source files");
    eprintln!("    --check                   Check if files are formatted (exit 1 if not)");
    eprintln!("    --diff                    Show what would change");
    eprintln!("    --stdin                   Read from stdin, write to stdout");
    eprintln!("    --output-format json      Machine-readable JSON output (with --check/--diff)");
    eprintln!("  clean                       Remove build artifacts");
    eprintln!();
    eprintln!("Build options:");
    eprintln!("  -o <path>                   Output binary path");
    eprintln!("  --release, -r               Use release profile (optimized)");
    eprintln!("  --opt <none|speed|size>     Override optimization level");
    eprintln!("  --build-dir <path>          Override build directory");
    eprintln!("  --verbose, -v               Print compilation steps");
    eprintln!();
    eprintln!("If no command is given, defaults to 'check'.");
}

/// Lex + parse + typecheck. Returns (module AST, typechecker) on success.
fn frontend(source: &str, filename: &str) -> Result<(ast::Module, TypeChecker), ()> {
    // 1. Tokenize
    let tokens = match lex(source) {
        Ok(t) => t,
        Err(diag) => {
            render_diagnostic(source, filename, &diag);
            return Err(());
        }
    };

    // 2. Parse
    let mut parser = Parser::new(tokens);
    let module_ast = match parser.parse_module(filename) {
        Ok(m) => m,
        Err(diag) => {
            render_diagnostic(source, filename, &diag);
            return Err(());
        }
    };

    // 3. Typecheck
    let root = Path::new(filename)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let resolver = FsResolver { root };
    let loader = Rc::new(RefCell::new(ModuleLoader::new(Box::new(resolver))));
    let mut checker = TypeChecker::with_loader(loader);
    let errors = checker.check_module_all(&module_ast);

    // Surface warnings (stored in checker.diagnostics by check_module_all)
    let warnings: Vec<_> = checker.diagnostics.drain(..).collect();
    for w in &warnings {
        render_diagnostic(source, filename, w);
    }

    if !errors.is_empty() {
        for diag in &errors {
            render_diagnostic(source, filename, diag);
        }
        let error_count = errors.len();
        eprintln!(
            "\n{} error{} emitted",
            error_count,
            if error_count == 1 { "" } else { "s" }
        );
        return Err(());
    }

    Ok((module_ast, checker))
}

fn cmd_check(filename: &str) {
    let source = read_source(filename);
    match frontend(&source, filename) {
        Ok(_) => println!("Type checking passed for {}", filename),
        Err(()) => std::process::exit(1),
    }
}

/// Run the full frontend pipeline: parse, typecheck, lower to FIR, validate,
/// and verify that a main() entry point exists.
fn frontend_and_lower(source: &str, filename: &str) -> fir::FirModule {
    let (module_ast, checker) = match frontend(source, filename) {
        Ok(v) => v,
        Err(()) => std::process::exit(1),
    };

    // Lower AST → FIR
    let mut lowerer = fir::Lowerer::new(checker.env, checker.type_table);
    if let Err(e) = lowerer.lower_module(&module_ast) {
        render_execution_error(source, filename, &e);
        std::process::exit(2);
    }
    let fir_module = lowerer.finish();

    // Validate FIR invariants (debug builds only)
    #[cfg(debug_assertions)]
    {
        let fir_errors = fir::validate::validate(&fir_module);
        for e in &fir_errors {
            eprintln!("{}", e);
        }
        if !fir_errors.is_empty() {
            eprintln!("FIR validation failed with {} errors", fir_errors.len());
            std::process::exit(2);
        }
    }

    // Check for entry point
    if fir_module.entry.is_none() {
        render_diagnostic(
            source,
            filename,
            &Diagnostic::error("no main() function found")
                .with_code("E026")
                .with_note("add a `def main() -> Int` function as the program entry point"),
        );
        std::process::exit(1);
    }

    fir_module
}

fn cmd_run(filename: &str) {
    let source = read_source(filename);
    let fir_module = frontend_and_lower(&source, filename);
    let entry = fir_module.entry.unwrap();

    // JIT compile and run
    let mut jit = codegen::CraneliftJIT::new();
    if let Err(e) = jit.compile_module(&fir_module) {
        eprintln!("JIT compilation error: {}", e);
        std::process::exit(2);
    }

    let exit_code = jit.call_i64(entry);
    std::process::exit(exit_code.clamp(0, 255) as i32);
}

/// Parsed build options from CLI flags.
struct BuildOptions {
    filename: String,
    output: Option<String>,
    config: BuildConfig,
    build_dir_override: Option<String>,
}

fn cmd_build(opts: &BuildOptions) {
    let source = read_source(&opts.filename);
    let fir_module = frontend_and_lower(&source, &opts.filename);

    // Resolve build paths
    let source_path = Path::new(&opts.filename)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(&opts.filename).to_path_buf());
    let build_dir_override = opts
        .build_dir_override
        .as_ref()
        .map(|s| Path::new(s.as_str()));
    let paths = resolve_build_paths(&source_path, opts.config.profile, build_dir_override);
    paths.ensure_dirs().unwrap_or_else(|e| {
        eprintln!("Failed to create build directory: {}", e);
        std::process::exit(2);
    });

    // Load manifest for caching
    let mut manifest_data = BuildManifest::load(&paths.manifest())
        .filter(|m| m.is_compatible(opts.config.profile_dir(), opts.config.cranelift_opt_level()));
    let source_hash = sha256_hex(source.as_bytes());
    let source_name = Path::new(&opts.filename)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // Step 1: Compile source to .o (skip if cached)
    let obj_path = paths.object_for(&source_name);
    let obj_path_str = obj_path.to_string_lossy().to_string();

    let obj_fresh = manifest_data
        .as_ref()
        .is_some_and(|m| m.is_file_fresh(&source_name, &source_hash) && obj_path.exists());

    if obj_fresh {
        if opts.config.verbose {
            eprintln!("[1/3] {} (cached)", source_name);
        }
    } else {
        if opts.config.verbose {
            eprintln!("[1/3] Compiling {} → {}", source_name, obj_path.display());
        }

        let mut aot = codegen::CraneliftAOT::with_config(&opts.config);
        if let Err(e) = aot.compile_module(&fir_module) {
            eprintln!("AOT compilation error: {}", e);
            std::process::exit(2);
        }
        if let Err(e) = aot.emit_object_to_file(&obj_path_str) {
            eprintln!("Error writing object file: {}", e);
            std::process::exit(2);
        }
    }

    // Step 2: Compile runtime (skip if cached)
    let runtime_source = codegen::runtime_source::c_runtime_source();
    let asm_source = codegen::asm_source::asm_source_for_target();
    let combined_hash = sha256_hex(format!("{}{}", runtime_source, asm_source).as_bytes());
    let runtime_o = paths.runtime_o();
    let asm_o = paths.gen_dir.join("asm.o");

    let runtime_fresh = manifest_data.as_ref().is_some_and(|m| {
        m.is_runtime_fresh(&combined_hash) && runtime_o.exists() && asm_o.exists()
    });

    if runtime_fresh {
        if opts.config.verbose {
            eprintln!("[2/4] Runtime (cached)");
            eprintln!("[3/4] Assembly (cached)");
        }
    } else {
        // Compile runtime.c → runtime.o
        if opts.config.verbose {
            eprintln!("[2/4] Compiling runtime → {}", runtime_o.display());
        }

        let runtime_c = paths.runtime_c();
        fs::write(&runtime_c, runtime_source).unwrap_or_else(|e| {
            eprintln!("Failed to write runtime: {}", e);
            std::process::exit(2);
        });

        let cc_flags: &[&str] = match opts.config.profile {
            Profile::Debug => &["-c", "-g"],
            Profile::Release => &[
                "-c",
                "-O2",
                "-fstack-protector-strong",
                "-D_FORTIFY_SOURCE=2",
            ],
        };
        let cc = cc_compiler();
        let status = std::process::Command::new(&cc)
            .args(cc_flags)
            .arg("-pthread")
            .arg(runtime_c.to_string_lossy().as_ref())
            .arg("-o")
            .arg(runtime_o.to_string_lossy().as_ref())
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("Runtime compilation failed: {}", s);
                std::process::exit(2);
            }
            Err(e) => {
                eprintln!("Failed to run cc: {}", e);
                std::process::exit(2);
            }
        }

        // Compile assembly → asm.o
        if opts.config.verbose {
            eprintln!("[3/4] Compiling assembly → {}", asm_o.display());
        }

        let asm_s = paths.gen_dir.join("green_asm.S");
        fs::write(&asm_s, asm_source).unwrap_or_else(|e| {
            eprintln!("Failed to write assembly: {}", e);
            std::process::exit(2);
        });

        let status = std::process::Command::new(&cc)
            .arg("-c")
            .arg(asm_s.to_string_lossy().as_ref())
            .arg("-o")
            .arg(asm_o.to_string_lossy().as_ref())
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("Assembly compilation failed: {}", s);
                std::process::exit(2);
            }
            Err(e) => {
                eprintln!("Failed to run cc for assembly: {}", e);
                std::process::exit(2);
            }
        }
    }

    // Step 4: Link
    let final_output = if let Some(ref out) = opts.output {
        out.clone()
    } else {
        paths.binary_for(&source_name).to_string_lossy().to_string()
    };

    if opts.config.verbose {
        eprintln!("[4/4] Linking → {}", final_output);
    }

    let cc = cc_compiler();
    let status = std::process::Command::new(&cc)
        .arg(obj_path.to_string_lossy().as_ref())
        .arg(runtime_o.to_string_lossy().as_ref())
        .arg(asm_o.to_string_lossy().as_ref())
        .arg("-pthread")
        .arg("-lm")
        .arg("-o")
        .arg(&final_output)
        .status();

    match status {
        Ok(s) if s.success() => {
            // Update manifest
            let mut manifest = manifest_data.take().unwrap_or_else(|| {
                BuildManifest::new(opts.config.profile_dir(), opts.config.cranelift_opt_level())
            });
            manifest.record_file(&source_name, &source_hash, &obj_path_str);
            manifest.runtime_hash = combined_hash;
            let _ = manifest.save(&paths.manifest());

            let size = fs::metadata(&final_output).map(|m| m.len()).unwrap_or(0);
            println!("Compiled to {} ({}K)", final_output, size / 1024);
        }
        Ok(s) => {
            eprintln!("Linker failed with exit code: {}", s);
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("Failed to run linker (cc): {}", e);
            std::process::exit(2);
        }
    }
}

fn cmd_clean(all: bool) {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let project_root = build_dir::find_project_root(&cwd);

    if all {
        let aster_dir = project_root.join(".aster");
        if aster_dir.is_dir() {
            let _ = fs::remove_dir_all(&aster_dir);
            println!("Removed {}", aster_dir.display());
        } else {
            println!("Nothing to clean.");
        }
    } else {
        let build_dir = project_root.join(".aster").join("build");
        if build_dir.is_dir() {
            let _ = fs::remove_dir_all(&build_dir);
            println!("Removed {}", build_dir.display());
        } else {
            println!("Nothing to clean.");
        }
    }
}

fn render_execution_error(source: &str, filename: &str, err: &LowerError) {
    let span = err.span();
    match err {
        LowerError::UnsupportedFeature(kind, _) => {
            let detail = kind.detail();
            let diag = Diagnostic::error(format!(
                "execution support for {detail} is not executable yet"
            ))
            .with_code("E028")
            .with_label(span, format!("{detail} cannot be compiled yet"))
            .with_note("this file can still pass `asterc check` while `run` and `build` reject it");
            render_diagnostic(source, filename, &diag);
        }
        LowerError::UnboundVariable(name, _) => {
            let diag = Diagnostic::error(format!("unbound variable '{name}' during lowering"))
                .with_code("E028")
                .with_label(span, "not found in lowered scope")
                .with_note(
                    "this file can still pass `asterc check` while `run` and `build` reject it",
                );
            render_diagnostic(source, filename, &diag);
        }
    }
}

fn cmd_fmt(args: &[String]) {
    let config = aster_fmt::config::FormatConfig::default();
    let mut check_only = false;
    let mut diff_mode = false;
    let mut stdin_mode = false;
    let mut json_output = false;
    let mut files = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check_only = true,
            "--diff" => diff_mode = true,
            "--stdin" => stdin_mode = true,
            "--output-format" => {
                i += 1;
                if i < args.len() && args[i] == "json" {
                    json_output = true;
                } else {
                    eprintln!("Unknown output format. Supported: json");
                    std::process::exit(1);
                }
            }
            _ => files.push(args[i].clone()),
        }
        i += 1;
    }

    // --stdin: read from stdin, write to stdout
    if stdin_mode {
        use std::io::Read;
        const MAX_STDIN_SIZE: usize = 10 * 1024 * 1024; // 10 MB, matches lexer limit
        let mut source = String::new();
        std::io::stdin()
            .take(MAX_STDIN_SIZE as u64 + 1)
            .read_to_string(&mut source)
            .unwrap_or_else(|e| {
                eprintln!("Failed to read stdin: {}", e);
                std::process::exit(1);
            });
        if source.len() > MAX_STDIN_SIZE {
            eprintln!("stdin input exceeds 10 MB limit");
            std::process::exit(1);
        }
        match aster_fmt::format_source(&source, &config) {
            Ok(formatted) => print!("{}", formatted),
            Err(e) => {
                eprintln!("Error formatting stdin: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // If no files given, find all .aster files in cwd
    if files.is_empty() {
        let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
        if let Ok(entries) = fs::read_dir(&cwd) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("aster") {
                    files.push(path.to_string_lossy().to_string());
                }
            }
        }
        if files.is_empty() {
            eprintln!("No .aster files found in current directory.");
            std::process::exit(1);
        }
    }

    let mut any_changed = false;
    let mut json_results: Vec<serde_json::Value> = Vec::new();

    for file in &files {
        let source = read_source(file);

        if json_output && (check_only || diff_mode) {
            // JSON diff output (Phase 6A)
            match aster_fmt::format_diff(&source, &config) {
                Ok(diffs) => {
                    let is_formatted = diffs.is_empty();
                    let diff_entries: Vec<serde_json::Value> = diffs
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "line": d.line,
                                "original": d.original,
                                "formatted": d.formatted,
                            })
                        })
                        .collect();
                    json_results.push(serde_json::json!({
                        "file": file,
                        "formatted": is_formatted,
                        "diff": diff_entries,
                    }));
                    if !is_formatted {
                        any_changed = true;
                    }
                }
                Err(e) => {
                    json_results.push(serde_json::json!({
                        "file": file,
                        "error": e.to_string(),
                    }));
                    any_changed = true;
                }
            }
            continue;
        }

        match aster_fmt::format_source(&source, &config) {
            Ok(formatted) => {
                if check_only {
                    if formatted != source {
                        eprintln!("{}: needs formatting", file);
                        any_changed = true;
                    }
                } else if diff_mode {
                    if formatted != source {
                        print_unified_diff(file, &source, &formatted);
                        any_changed = true;
                    }
                } else if formatted != source {
                    fs::write(file, &formatted).unwrap_or_else(|e| {
                        eprintln!("Could not write '{}': {}", file, e);
                        std::process::exit(1);
                    });
                    println!("Formatted {}", file);
                }
            }
            Err(e) => {
                eprintln!("Error formatting '{}': {}", file, e);
                std::process::exit(1);
            }
        }
    }

    if json_output {
        let output = if json_results.len() == 1 {
            serde_json::to_string_pretty(&json_results[0]).unwrap()
        } else {
            serde_json::to_string_pretty(&json_results).unwrap()
        };
        println!("{}", output);
    }

    if (check_only || diff_mode) && any_changed {
        std::process::exit(1);
    }
}

fn print_unified_diff(filename: &str, original: &str, formatted: &str) {
    let orig_lines: Vec<&str> = original.lines().collect();
    let fmt_lines: Vec<&str> = formatted.lines().collect();

    println!("--- {}", filename);
    println!("+++ {}", filename);

    let max = orig_lines.len().max(fmt_lines.len());
    let mut i = 0;
    while i < max {
        let orig = orig_lines.get(i).copied().unwrap_or("");
        let fmt = fmt_lines.get(i).copied().unwrap_or("");
        if orig != fmt {
            // Find the extent of this change hunk
            let start = i;
            while i < max {
                let o = orig_lines.get(i).copied().unwrap_or("");
                let f = fmt_lines.get(i).copied().unwrap_or("");
                if o == f {
                    break;
                }
                i += 1;
            }
            println!(
                "@@ -{},{} +{},{} @@",
                start + 1,
                i - start,
                start + 1,
                i - start
            );
            for j in start..i {
                if let Some(&line) = orig_lines.get(j) {
                    println!("-{}", line);
                }
            }
            for j in start..i {
                if let Some(&line) = fmt_lines.get(j) {
                    println!("+{}", line);
                }
            }
        } else {
            i += 1;
        }
    }
}

/// Maximum source file size (10 MB), matching the lexer's MAX_INPUT_SIZE.
const MAX_SOURCE_SIZE: u64 = 10 * 1024 * 1024;

fn read_source(filename: &str) -> String {
    // Pre-check file size to avoid reading huge files into memory.
    match fs::metadata(filename) {
        Ok(meta) => {
            if meta.len() > MAX_SOURCE_SIZE {
                eprintln!(
                    "Source file '{}' is too large ({} bytes, max {} bytes)",
                    filename,
                    meta.len(),
                    MAX_SOURCE_SIZE
                );
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Could not read file '{}': {}", filename, e);
            std::process::exit(1);
        }
    }
    match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Could not read file '{}': {}", filename, e);
            std::process::exit(1);
        }
    }
}

/// Parse build flags from args starting at `start_idx`.
fn parse_build_options(args: &[String], start_idx: usize) -> BuildOptions {
    let filename = args[start_idx].clone();
    let mut output = None;
    let mut release = false;
    let mut opt_override = None;
    let mut build_dir_override = None;
    let mut verbose = false;

    let mut i = start_idx + 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output = Some(args[i].clone());
                }
            }
            "--release" | "-r" => release = true,
            "--verbose" | "-v" => verbose = true,
            "--opt" => {
                i += 1;
                if i < args.len() {
                    opt_override = Some(args[i].clone());
                }
            }
            "--build-dir" => {
                i += 1;
                if i < args.len() {
                    build_dir_override = Some(args[i].clone());
                }
            }
            other => {
                eprintln!("Unknown flag: {}", other);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let mut config = if release {
        BuildConfig::release()
    } else {
        BuildConfig::debug()
    };
    config.verbose = verbose;

    // Apply opt level override
    if let Some(ref opt) = opt_override {
        config.opt_level = match opt.as_str() {
            "none" => OptLevel::None,
            "speed" => OptLevel::Speed,
            "size" => OptLevel::SpeedAndSize,
            other => {
                eprintln!(
                    "Unknown optimization level '{}'. Use: none, speed, size",
                    other
                );
                std::process::exit(1);
            }
        };
    }

    // Check for env var override
    if build_dir_override.is_none()
        && let Ok(dir) = env::var("ASTER_BUILD_DIR")
    {
        build_dir_override = Some(dir);
    }

    BuildOptions {
        filename,
        output,
        config,
        build_dir_override,
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "check" => {
            if args.len() < 3 {
                eprintln!("Usage: asterc check <file.aster>");
                std::process::exit(1);
            }
            cmd_check(&args[2]);
        }
        "run" => {
            if args.len() < 3 {
                eprintln!("Usage: asterc run <file.aster>");
                std::process::exit(1);
            }
            cmd_run(&args[2]);
        }
        "build" => {
            if args.len() < 3 {
                eprintln!("Usage: asterc build <file.aster> [options]");
                std::process::exit(1);
            }
            let opts = parse_build_options(&args, 2);
            cmd_build(&opts);
        }
        "fmt" => {
            cmd_fmt(&args[2..]);
        }
        "clean" => {
            let all = args.iter().any(|a| a == "--all");
            cmd_clean(all);
        }
        // Default: treat first arg as a file, default to check
        other => {
            if other.ends_with(".aster") || std::path::Path::new(other).exists() {
                cmd_check(other);
            } else {
                eprintln!("Unknown command: {}", other);
                print_usage();
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod runtime_tests {
    #[test]
    fn c_runtime_exports_every_declared_runtime_symbol() {
        let runtime = codegen::runtime_source::c_runtime_source();
        for (name, _, _) in codegen::runtime_sigs::RUNTIME_SIGS {
            assert!(
                runtime.contains(name),
                "embedded C runtime is missing symbol declaration for {name}"
            );
        }
    }
}
