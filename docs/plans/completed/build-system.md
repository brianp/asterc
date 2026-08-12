---
status: executed
created: 2026-03-12 09:30
executed: 2026-03-12 10:00
---

# Implementation Plan: Build Directory, Optimization Levels, and Artifact Caching

## Prerequisites

- Codegen pipeline is complete (JIT + AOT both working, 518 tests passing)
- `asterc build` produces working native binaries via `cc` linking
- `asterc run` works via Cranelift JIT
- RFC: `docs/design/compilation-rfc.md` defines the full design

## Codebase Analysis

- **`src/main.rs`** — CLI entry point. `cmd_build()` writes `.o` and `_runtime.c` next to output, links, deletes intermediates. `cmd_run()` uses JIT in-memory. Manual arg parsing (no clap).
- **`codegen/src/aot.rs`** — `CraneliftAOT::new()` hardcodes `opt_level = "speed"` and `is_pic = "true"`. No config parameter.
- **`codegen/src/jit.rs`** — `CraneliftJIT::new()` hardcodes `opt_level = "speed"` and `is_pic = "false"`. No config parameter.
- **`write_c_runtime()`** in `src/main.rs` — embeds the entire C runtime as a string literal, writes to disk each build.
- No existing build directory, manifest, or caching logic anywhere.

## Research Findings

- **Cranelift optimization levels**: `none`, `speed`, `speed_and_size` — maps cleanly to our debug/release/size model
- **Industry convention**: hidden build dir (`.build/`, `zig-cache/`, `target/`) is standard; `.aster/build/` fits the pattern
- **Incremental strategy**: file-content hashing with a JSON manifest is the minimal correct approach (Go uses action IDs, Cargo uses fingerprints — both are content-hash at core)
- **Runtime caching**: compiling the C runtime once and caching `runtime.o` is a significant win — `cc` invocation is the slowest part of `asterc build`

## Task Breakdown

### 1. Build Configuration Types

- **Files to create:** `codegen/src/config.rs`
- **Files to modify:** `codegen/src/lib.rs`, `codegen/src/aot.rs`, `codegen/src/jit.rs`
- **Dependencies:** None
- **Approach:** Define `BuildConfig`, `OptLevel`, and `Profile` enums. Add `CraneliftAOT::with_config(config)` and `CraneliftJIT::with_config(config)` constructors that map `OptLevel` to Cranelift's `opt_level` setting. Keep existing `::new()` as shorthand for default config. Export from `codegen/src/lib.rs`.
- **Key decisions:**
  - Three opt levels only (`None`, `Speed`, `SpeedAndSize`) — direct Cranelift mapping, no fake `-O1`/`-O3` distinction
  - `debug_info: bool` field in config even though Cranelift DWARF support is limited — forward-compatible
- **Data structures:**
  ```
  OptLevel: { None, Speed, SpeedAndSize }
  Profile: { Debug, Release }
  BuildConfig: { opt_level, debug_info, profile, verbose }
  ```

### 2. Build Directory Management

- **Files to create:** `src/build_dir.rs`
- **Files to modify:** `src/main.rs`
- **Dependencies:** None
- **Approach:** Implement `resolve_build_dir(source_path, override)` that walks up from the source file to find project root (`.aster/` or `.git/` marker), then returns `<root>/.aster/build/<profile>/`. Create `obj/`, `gen/`, `bin/` subdirectories lazily. Support `ASTER_BUILD_DIR` env var and `--build-dir` flag override.
- **Key decisions:**
  - Lazy directory creation (don't create until first write)
  - Project root detection: `.aster/` > `.git/` > source parent
- **Implementation notes:** Use `std::fs::create_dir_all` for lazy creation. Return a `BuildPaths` struct with resolved paths for obj, gen, bin subdirs.
- **Data structures:**
  ```
  BuildPaths: { root, obj_dir, gen_dir, bin_dir, profile }
  ```

### 3. Build Manifest

- **Files to create:** `src/manifest.rs`
- **Dependencies:** Task 2 (needs build dir paths)
- **Approach:** Define `BuildManifest` struct serialized as JSON to `.aster/build/<profile>/manifest.json`. Tracks compiler version, profile, opt_level, target triple, per-file source hashes, runtime hash, and timestamps. On build start, load existing manifest. After each compilation step, check if artifact is stale (hash mismatch or missing). After build completes, write updated manifest.
- **Key decisions:**
  - SHA-256 for content hashing (via `sha2` crate) — add to root `Cargo.toml`
  - Hash source content only, not file paths (content-addressed)
  - Compiler version change invalidates entire manifest
- **Data structures:**
  ```
  BuildManifest: { compiler_version, profile, opt_level, target, files: HashMap<path, FileEntry>, runtime_hash }
  FileEntry: { source_hash, object_path, compiled_at }
  ```
- **Potential issues:**
  - First build always has no manifest → full compile (correct behavior)
  - Corrupt manifest → treat as missing, full rebuild

### 4. Runtime Caching

- **Files to modify:** `src/main.rs` (extract `write_c_runtime` logic), `src/build_dir.rs`
- **Dependencies:** Task 2 (build dir), Task 3 (manifest for hash tracking)
- **Approach:** On build, check manifest for `runtime_hash`. Hash the embedded runtime template string at compile time (or lazily). If hash matches and `gen/runtime.o` exists → skip. Otherwise, write `gen/runtime.c`, compile to `gen/runtime.o` via `cc -c` (with profile-appropriate flags: `-g` for debug, `-O2` for release). Store `runtime.o` path for linker step.
- **Key decisions:**
  - Compile runtime separately (`cc -c`) then link, instead of compiling and linking in one step
  - Debug runtime gets `-g`, release gets `-O2`
- **Implementation notes:** The runtime C source is ~100 lines and changes only when the compiler changes. Caching saves ~200ms per build on typical systems.

### 5. Refactor `cmd_build` to Use Build Directory

- **Files to modify:** `src/main.rs`
- **Dependencies:** Tasks 1-4
- **Approach:** Rewrite `cmd_build()` to:
  1. Parse flags (`--release`, `--opt`, `--build-dir`, `-o`, `-v`)
  2. Resolve build paths via `resolve_build_dir()`
  3. Load manifest
  4. Check if source `.o` is stale → compile to `<build>/obj/<name>.o`
  5. Check if runtime is stale → compile to `<build>/gen/runtime.o`
  6. Check if binary is stale → link `obj/*.o` + `gen/runtime.o` → `<build>/bin/<name>`
  7. If `-o` specified, copy final binary to that path
  8. Update and write manifest
  9. Print verbose output if `-v`
- **Integration points:** `BuildConfig` feeds into `CraneliftAOT::with_config()`. `BuildPaths` provides all file paths. `BuildManifest` drives skip/recompile decisions.
- **Key decisions:**
  - Default profile is `debug` (matches Cargo convention)
  - Default output goes to `<build>/bin/<name>`, not next to source

### 6. CLI Flag Parsing

- **Files to modify:** `src/main.rs`
- **Dependencies:** Task 5
- **Approach:** Extend the manual arg parser in `main()` to handle new flags: `--release`/`-r`, `--opt <level>`, `--build-dir <path>`, `--verbose`/`-v`. Add `clean` and `clean --all` subcommands. Keep manual parsing (no clap dependency) — the flag set is small and well-defined.
- **Key decisions:**
  - No clap — too heavy for 6 flags. Manual parsing keeps deps minimal.
  - `clean` is a subcommand, not a flag
- **Implementation notes:** Parse into a `BuildOptions` struct before calling `cmd_build`. Validate flag combinations (e.g., `--opt` with `run` is ignored with a warning).

### 7. `clean` Subcommand

- **Files to modify:** `src/main.rs`
- **Dependencies:** Task 2 (build dir resolution)
- **Approach:** `asterc clean` removes `.aster/build/`. `asterc clean --all` removes `.aster/` entirely. Resolve project root the same way as build. Print what was removed and total size freed.
- **Potential issues:**
  - No `.aster/build/` exists → "Nothing to clean" (not an error)
  - Permission errors → report and continue

### 8. Verbose Output

- **Files to modify:** `src/main.rs`
- **Dependencies:** Tasks 5-6
- **Approach:** When `--verbose` is set, print step-by-step progress: `[1/N] Compiling ...`, `[2/N] Runtime (cached)`, `[3/N] Linking ...`, `[4/N] Done: path (size)`. Without verbose, just print the final "Compiled to ..." line (current behavior).
- **Key decisions:**
  - Verbose goes to stderr (so stdout is clean for piping)
  - Non-verbose keeps current single-line output

## Potential Challenges & Mitigations

1. **Challenge:** `cc` might not be available on all systems (Windows especially)
   **Mitigation:** Already a requirement today. Document it. Windows support is future work.

2. **Challenge:** Runtime `.o` compiled with different `cc` version than user objects
   **Mitigation:** Include `cc --version` hash in manifest (or just recompile runtime when compiler version changes, which is simpler and nearly as correct).

3. **Challenge:** Concurrent builds writing to same build directory
   **Mitigation:** Defer file locking to future work. Single-user single-build is the expected use case now.

4. **Challenge:** Build dir detection might pick wrong root for nested projects
   **Mitigation:** Prefer `.aster/` marker over `.git/`. Users can override with `--build-dir` or env var.

## Unwired Code Audit

- [x] `BuildConfig` is created by CLI flag parsing (Task 6) AND consumed by `CraneliftAOT::with_config` (Task 1)
- [x] `BuildPaths` is created by `resolve_build_dir` (Task 2) AND consumed by `cmd_build` (Task 5)
- [x] `BuildManifest` is loaded at build start (Task 3) AND written at build end (Task 5)
- [x] `runtime_hash` is checked in manifest (Task 3) AND set after runtime compilation (Task 4)
- [x] `source_hash` per file is checked (Task 3) AND set after each `.o` compilation (Task 5)
- [x] `clean` subcommand (Task 7) deletes what `resolve_build_dir` creates (Task 2)
- [x] `--verbose` flag is parsed (Task 6) AND consumed by build steps (Task 8)
- [x] `--release` flag is parsed (Task 6) AND maps to `Profile::Release` in `BuildConfig` (Task 1)

## Validation Steps

- `cargo build` — compiles with new types and modules
- `cargo test` — all 518+ existing tests pass (no behavior change for tests)
- `cargo clippy -- -D warnings` — clean
- `asterc build examples/hello.aster` — produces `.aster/build/debug/bin/hello`
- `asterc build examples/hello.aster --release` — produces `.aster/build/release/bin/hello`
- `asterc build examples/hello.aster -o /tmp/hello` — produces `/tmp/hello`, intermediates in `.aster/build/`
- `asterc build examples/hello.aster` (twice) — second build says "cached" / "up to date"
- `asterc build examples/hello.aster --release -v` — verbose output shows steps
- `asterc clean` — removes `.aster/build/`, confirms
- `asterc run examples/hello.aster` — unchanged behavior (JIT, no disk artifacts)
- Modify `examples/hello.aster`, rebuild — only changed file recompiles
