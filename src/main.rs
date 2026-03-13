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
use lexer::lex;
use parser::Parser;
use typecheck::module_loader::{FsResolver, ModuleLoader};
use typecheck::typechecker::TypeChecker;

use crate::build_dir::resolve_build_paths;
use crate::manifest::{BuildManifest, sha256_hex};

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

fn cmd_run(filename: &str) {
    let source = read_source(filename);
    let (module_ast, checker) = match frontend(&source, filename) {
        Ok(v) => v,
        Err(()) => std::process::exit(1),
    };

    // Lower AST → FIR
    let mut lowerer = fir::Lowerer::new(checker.env, checker.type_table);
    if let Err(e) = lowerer.lower_module(&module_ast) {
        eprintln!("Lowering error: {}", e);
        std::process::exit(2);
    }
    let fir_module = lowerer.finish();

    // Check for entry point
    let entry = match fir_module.entry {
        Some(id) => id,
        None => {
            render_diagnostic(
                &source,
                filename,
                &Diagnostic::error("no main() function found")
                    .with_code("E026")
                    .with_note("add a `def main() -> Int` function as the program entry point"),
            );
            std::process::exit(1);
        }
    };

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
    let (module_ast, checker) = match frontend(&source, &opts.filename) {
        Ok(v) => v,
        Err(()) => std::process::exit(1),
    };

    // Lower AST → FIR
    let mut lowerer = fir::Lowerer::new(checker.env, checker.type_table);
    if let Err(e) = lowerer.lower_module(&module_ast) {
        eprintln!("Lowering error: {}", e);
        std::process::exit(2);
    }
    let fir_module = lowerer.finish();

    if fir_module.entry.is_none() {
        render_diagnostic(
            &source,
            &opts.filename,
            &Diagnostic::error("no main() function found")
                .with_code("E026")
                .with_note("add a `def main() -> Int` function as the program entry point"),
        );
        std::process::exit(1);
    }

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
    let runtime_source = c_runtime_source();
    let runtime_hash = sha256_hex(runtime_source.as_bytes());
    let runtime_o = paths.runtime_o();

    let runtime_fresh = manifest_data
        .as_ref()
        .is_some_and(|m| m.is_runtime_fresh(&runtime_hash) && runtime_o.exists());

    if runtime_fresh {
        if opts.config.verbose {
            eprintln!("[2/3] Runtime (cached)");
        }
    } else {
        if opts.config.verbose {
            eprintln!("[2/3] Compiling runtime → {}", runtime_o.display());
        }

        let runtime_c = paths.runtime_c();
        fs::write(&runtime_c, runtime_source).unwrap_or_else(|e| {
            eprintln!("Failed to write runtime: {}", e);
            std::process::exit(2);
        });

        // Compile runtime.c → runtime.o
        let cc_flags: &[&str] = match opts.config.profile {
            Profile::Debug => &["-c", "-g"],
            Profile::Release => &["-c", "-O2"],
        };
        let status = std::process::Command::new("cc")
            .args(cc_flags)
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
    }

    // Step 3: Link
    let final_output = if let Some(ref out) = opts.output {
        out.clone()
    } else {
        paths.binary_for(&source_name).to_string_lossy().to_string()
    };

    if opts.config.verbose {
        eprintln!("[3/3] Linking → {}", final_output);
    }

    let status = std::process::Command::new("cc")
        .arg(obj_path.to_string_lossy().as_ref())
        .arg(runtime_o.to_string_lossy().as_ref())
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
            manifest.runtime_hash = runtime_hash;
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

fn c_runtime_source() -> &'static str {
    r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

void* aster_alloc(int64_t size) {
    if (size == 0) return (void*)8; /* aligned dangling */
    if (size < 0) { fprintf(stderr, "aster_alloc: negative size\n"); abort(); }
    void* p = malloc((size_t)size);
    if (!p) { fprintf(stderr, "out of memory\n"); abort(); }
    return p;
}

void* aster_class_alloc(int64_t size) { return aster_alloc(size); }

void aster_print_str(void* ptr) {
    if (!ptr) { printf("nil\n"); return; }
    int64_t len = *(int64_t*)ptr;
    if (len < 0) { printf("<invalid string>\n"); return; }
    char* data = (char*)ptr + 8;
    printf("%.*s\n", (int)len, data);
}

void aster_print_int(int64_t val) { printf("%lld\n", (long long)val); }
void aster_print_float(double val) { printf("%g\n", val); }
void aster_print_bool(int8_t val) { printf("%s\n", val ? "true" : "false"); }

void* aster_string_new(void* data, int64_t len) {
    void* p = aster_alloc(8 + len);
    *(int64_t*)p = len;
    if (len > 0) memcpy((char*)p + 8, data, (size_t)len);
    return p;
}

void* aster_string_concat(void* a, void* b) {
    int64_t a_len = a ? *(int64_t*)a : 0;
    int64_t b_len = b ? *(int64_t*)b : 0;
    if (a_len < 0) a_len = 0;
    if (b_len < 0) b_len = 0;
    void* r = aster_alloc(8 + a_len + b_len);
    *(int64_t*)r = a_len + b_len;
    if (a_len > 0) memcpy((char*)r + 8, (char*)a + 8, (size_t)a_len);
    if (b_len > 0) memcpy((char*)r + 8 + a_len, (char*)b + 8, (size_t)b_len);
    return r;
}

int64_t aster_string_len(void* ptr) {
    if (!ptr) return 0;
    int64_t len = *(int64_t*)ptr;
    return len < 0 ? 0 : len;
}

int64_t aster_pow_int(int64_t base, int64_t exp) {
    if (exp < 0) return 0;
    int64_t result = 1;
    while (exp > 0) {
        if (exp & 1) result *= base;
        base *= base;
        exp >>= 1;
    }
    return result;
}

void* aster_int_to_string(int64_t val) {
    char buf[32];
    int len = snprintf(buf, sizeof(buf), "%lld", (long long)val);
    if (len < 0) len = 0;
    return aster_string_new(buf, (int64_t)len);
}

void* aster_float_to_string(double val) {
    char buf[64];
    int len = snprintf(buf, sizeof(buf), "%g", val);
    if (len < 0) len = 0;
    return aster_string_new(buf, (int64_t)len);
}

void* aster_bool_to_string(int8_t val) {
    const char* s = val ? "true" : "false";
    return aster_string_new((void*)s, val ? 4 : 5);
}

/* List handle indirection: list value = handle (ptr to ptr to data block).
   Data block: [len: i64][cap: i64][data: i64...] */

void* aster_list_new(int64_t cap) {
    if (cap < 4) cap = 4;
    void* block = aster_alloc(16 + cap * 8);
    *(int64_t*)block = 0;              /* len */
    *((int64_t*)block + 1) = cap;     /* cap */
    void* handle = aster_alloc(8);
    *(void**)handle = block;
    return handle;
}

int64_t aster_list_get(void* handle, int64_t index) {
    if (!handle) { fprintf(stderr, "aster_list_get: null list\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    if (index < 0 || index >= len) {
        fprintf(stderr, "list index out of bounds: %lld (len %lld)\n", (long long)index, (long long)len);
        abort();
    }
    return *((int64_t*)block + 2 + index);
}

void aster_list_set(void* handle, int64_t index, int64_t value) {
    if (!handle) { fprintf(stderr, "aster_list_set: null list\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    if (index < 0 || index >= len) {
        fprintf(stderr, "list index out of bounds: %lld (len %lld)\n", (long long)index, (long long)len);
        abort();
    }
    *((int64_t*)block + 2 + index) = value;
}

void* aster_list_push(void* handle, int64_t value) {
    if (!handle) { fprintf(stderr, "aster_list_push: null list\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    int64_t cap = *((int64_t*)block + 1);
    if (len >= cap) {
        int64_t new_cap = cap * 2;
        if (new_cap < 4) new_cap = 4;
        void* new_block = aster_alloc(16 + new_cap * 8);
        memcpy(new_block, block, (size_t)(16 + len * 8));
        *((int64_t*)new_block + 1) = new_cap;
        free(block);
        *(void**)handle = new_block;
        block = new_block;
    }
    *((int64_t*)block + 2 + len) = value;
    *(int64_t*)block = len + 1;
    return handle;
}

int64_t aster_list_len(void* handle) {
    if (!handle) return 0;
    void* block = *(void**)handle;
    return *(int64_t*)block;
}

static int aster_string_eq(void* a, void* b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    int64_t a_len = *(int64_t*)a;
    int64_t b_len = *(int64_t*)b;
    if (a_len != b_len || a_len < 0) return 0;
    return memcmp((char*)a + 8, (char*)b + 8, (size_t)a_len) == 0;
}

/* Map handle indirection: map value = handle (ptr to ptr to data block).
   Data block: [len: i64][cap: i64][entries: [key: i64, val: i64]...] */

void* aster_map_new(int64_t cap) {
    if (cap < 4) cap = 4;
    void* block = aster_alloc(16 + cap * 16);
    *(int64_t*)block = 0;              /* len */
    *((int64_t*)block + 1) = cap;     /* cap */
    void* handle = aster_alloc(8);
    *(void**)handle = block;
    return handle;
}

void* aster_map_set(void* handle, int64_t key, int64_t value) {
    if (!handle) { fprintf(stderr, "aster_map_set: null map\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    int64_t cap = *((int64_t*)block + 1);
    int64_t* entries = ((int64_t*)block) + 2;
    for (int64_t i = 0; i < len; i++) {
        if (aster_string_eq((void*)entries[i * 2], (void*)key)) {
            entries[i * 2 + 1] = value;
            return handle;
        }
    }
    if (len >= cap) {
        int64_t new_cap = cap * 2;
        if (new_cap < 4) new_cap = 4;
        void* new_block = aster_alloc(16 + new_cap * 16);
        memcpy(new_block, block, (size_t)(16 + len * 16));
        *((int64_t*)new_block + 1) = new_cap;
        free(block);
        *(void**)handle = new_block;
        block = new_block;
        entries = ((int64_t*)block) + 2;
    }
    entries[len * 2] = key;
    entries[len * 2 + 1] = value;
    *(int64_t*)block = len + 1;
    return handle;
}

int64_t aster_map_get(void* handle, int64_t key) {
    if (!handle) { fprintf(stderr, "aster_map_get: null map\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    int64_t* entries = ((int64_t*)block) + 2;
    for (int64_t i = 0; i < len; i++) {
        if (aster_string_eq((void*)entries[i * 2], (void*)key)) {
            return entries[i * 2 + 1];
        }
    }
    return 0;
}

static int aster_error_flag = 0;

void aster_error_set(void) { aster_error_flag = 1; }

int8_t aster_error_check(void) {
    int8_t was_set = aster_error_flag ? 1 : 0;
    aster_error_flag = 0;
    return was_set;
}

void aster_panic(void) {
    fprintf(stderr, "aster: uncaught error\n");
    abort();
}

int main(int argc, char** argv) {
    (void)argc; (void)argv;
    extern int64_t aster_main(void);
    int64_t result = aster_main();
    return (int)result;
}
"#
}

fn cmd_fmt(args: &[String]) {
    let config = aster_fmt::config::FormatConfig::default();
    let mut check_only = false;
    let mut files = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--check" => check_only = true,
            _ => files.push(arg.clone()),
        }
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
    for file in &files {
        let source = read_source(file);
        match aster_fmt::format_source(&source, &config) {
            Ok(formatted) => {
                if check_only {
                    if formatted != source {
                        eprintln!("{}: needs formatting", file);
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

    if check_only && any_changed {
        std::process::exit(1);
    }
}

fn read_source(filename: &str) -> String {
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
    use super::c_runtime_source;

    #[test]
    fn c_runtime_exports_every_declared_runtime_symbol() {
        let runtime = c_runtime_source();
        for (name, _, _) in codegen::runtime_sigs::RUNTIME_SIGS {
            assert!(
                runtime.contains(name),
                "embedded C runtime is missing symbol declaration for {name}"
            );
        }
    }
}
