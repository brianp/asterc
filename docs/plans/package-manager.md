---
status: pending
created: 2026-03-27 22:00
executed: null
---

# Implementation Plan: Aster Package Manager

The `aster` CLI becomes the single entry point for the entire language: compiler, package manager, formatter, test runner, toolchain manager. The Seedfile is an Aster DSL file that declares project metadata, dependencies, and scripts. The package manager is written in Aster itself.

## Design Decisions

- **CLI**: `aster` is the command. `asterc` stays as the raw compiler, but users interact with `aster`.
- **Manifest**: `Seedfile`, an Aster source file evaluated in a DSL context.
- **DSL style**: Functions with named args. Needs `method_missing` or equivalent to allow package names as function calls (e.g., `http(version: "1.2.0")` where `http` isn't a real function).
- **Written in Aster**: The package manager is the first major Aster-in-Aster project. It dogfoods the language and drives feature development.
- **No backwards compatibility concerns**: Aster has no users yet. We can change anything.
- **Registry**: Git-based initially (packages are git repos with a Seedfile). Central registry is a future concern.

## Blockers: Language Features Required

These must be implemented in the compiler (Rust) before the package manager can be written in Aster. Ordered by dependency chain.

### Phase 0: OS Primitives

These are runtime functions exposed to Aster code. Each one is a Rust `extern "C"` function in `codegen/src/runtime/` wired through the existing ABI macro system.

#### 0.1 Command-line arguments (argv)

The package manager needs to know what the user typed (`aster add http`, `aster build`).

- **Runtime**: `aster_argv_count() -> Int`, `aster_argv_get(index: Int) -> String`
- **Stdlib surface**: `Sys.args() -> List[String]`
- **Files to modify**: `codegen/src/runtime/` (new `sys.rs`), `codegen/src/runtime_sigs.rs`, `codegen/src/runtime/mod.rs`
- **Typechecker**: Add `Sys` as a built-in module with static methods
- **Test**: `aster test.aster -- arg1 arg2` passes args through

#### 0.2 Environment variables

Reading `ASTER_HOME`, `PATH`, `HOME`, and setting env for child processes.

- **Runtime**: `aster_env_get(key: String) -> String?`, `aster_env_set(key: String, value: String)`
- **Stdlib surface**: `Sys.env(key: String) -> String?`, `Sys.set_env(key: String, value: String)`
- **Files**: Same `sys.rs` runtime module

#### 0.3 Process spawning

Running `git clone`, `tar`, `aster build`, and reading stdout/stderr/exit code.

- **Runtime**: `aster_process_run(cmd: String, args_handle: *mut u8, env_handle: *mut u8) -> i64` returning a process handle, plus `aster_process_stdout(handle) -> String`, `aster_process_stderr(handle) -> String`, `aster_process_exit_code(handle) -> Int`
- **Stdlib surface**: `Process.run(cmd: String, args: List[String]) -> ProcessResult throws ProcessError`
- **Key decision**: Synchronous initially. Async process spawning is a later concern.
- **This is the critical primitive.** Without it, the package manager can't fetch packages, run builds, or do anything useful.

#### 0.4 Filesystem operations

The package manager needs to create directories, list files, check existence, delete things.

- **Runtime functions needed**:
  - `aster_fs_exists(path: String) -> Bool`
  - `aster_fs_is_dir(path: String) -> Bool`
  - `aster_fs_mkdir(path: String) -> Void` (recursive, like `mkdir -p`)
  - `aster_fs_remove(path: String) -> Void` (recursive for dirs)
  - `aster_fs_list_dir(path: String) -> List[String]`
  - `aster_fs_copy(src: String, dst: String) -> Void`
  - `aster_fs_rename(src: String, dst: String) -> Void`
- **Stdlib surface**: `File.exists(path:)`, `Dir.create(path:)`, `Dir.list(path:)`, `Dir.remove(path:)`, etc.
- **Files**: New `fs.rs` in `codegen/src/runtime/`, or extend existing file I/O

#### 0.5 Hashing (SHA-256)

Package integrity verification. Lock files reference content hashes.

- **Runtime**: `aster_sha256(data: String) -> String` (returns hex digest)
- **Stdlib surface**: `Crypto.sha256(data: String) -> String`
- **Implementation**: Use the `sha2` crate already in Cargo.toml

### Phase 1: Dynamic Dispatch (method_missing) — DONE

Implemented via `DynamicReceiver` trait (Option A from original design). The trait intercepts unknown method calls and rewrites them to `self.method_missing(fn_name, args)`. Three-mode behavior (open, closed, hybrid) based on compiler inspection of the method_missing body.

Also implemented: `FunctionNotFound` error type, `FieldAccessible` trait (unstable), `std/unstable` module gating, and the full introspection API (`class_name`, `fields`, `methods`, `ancestors`, `children`, `is_a`, `responds_to`).

Relevant commits: `dba90cc`, `f84e940`, `62be19d`, `c3193d4`, `8ecaf78`.

### Phase 2: Seedfile DSL

With Phase 0 and 1 complete, we can define the Seedfile format.

#### 2.1 Seedfile stdlib module

A module (`std/seed`) that provides the DSL classes. Source lives at `aster-pkg/src/seedfile.aster`.

```
class Dependency
    name: String
    version: String
    path: String
    git: String
    dev: Bool
    trusted: Bool

class Task
    name: String
    cmd: String

class Seedfile includes DynamicReceiver
    name: String
    version_str: String
    compiler_version: String
    quarantine_days: Int
    deps: List[Dependency]
    tasks: List[Task]

    def package(name: String, version: String = "0.0.0") -> Void
        self.name = name
        self.version_str = version

    def compiler(version: String) -> Void
        self.compiler_version = version

    def quarantine(days: Int) -> Void
        quarantine_days = days

    def task(name: String, cmd: String) -> Void
        tasks.push(item: Task(name: name, cmd: cmd))

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        # If args has a "name" key, use it (for hyphenated package names).
        # Otherwise, the method name IS the package name.
        # Dev dependencies use dev: "true" flag.
        let dep_name = args["name"].or(default: fn_name)
        let dep_version = args["version"].or(default: "*")
        let dep_path = args["path"].or(default: "")
        let dep_git = args["git"].or(default: "")
        let dep_dev = args["dev"].or(default: "false") == "true"
        let dep_trusted = args["trusted"].or(default: "false") == "true"
        deps.push(item: Dependency(
            name: dep_name,
            version: dep_version,
            path: dep_path,
            git: dep_git,
            dev: dep_dev,
            trusted: dep_trusted
        ))
```

#### 2.2 Example Seedfile

```
package(name: "asterc", version: "0.1.0")
compiler(version: "0.1.0")
quarantine(days: 3)

lexer(path: "lexer")
ast(path: "ast")
parser(path: "parser")
typecheck(path: "typecheck")
fir(path: "fir")
codegen(path: "codegen")
aster_fmt(name: "aster-fmt", path: "aster-fmt")

ariadne(version: "0.4.1")
sha2(version: "0.10")
serde(version: "1")
serde_json(version: "1")
serde_json(version: "1", dev: "true")

task(name: "test", cmd: "aster test --release")
task(name: "bench", cmd: "aster run benchmarks/main.aster")
```

`package` and `task` are real methods. Everything else hits `method_missing` and becomes a dependency. Dev dependencies use the `dev: "true"` flag. Hyphenated package names use the `name:` override: `aster_fmt(name: "aster-fmt", path: "aster-fmt")`.

#### 2.3 Seedfile evaluation

The `aster` CLI:
1. Reads `Seedfile` from the current directory
2. Compiles it as an Aster module with `std/seed` imported
3. Evaluates it (JIT) to produce a populated `Seedfile` object
4. Reads the metadata from the object to drive package management

### Phase 3: Dependency Resolution

#### 3.1 Package source

Start simple: packages are git repos with a Seedfile at the root.

- `http(version: "1.2.0")` resolves to a git tag `v1.2.0` on a known registry repo
- `http(version: "1.2.0", git: "https://github.com/user/http.git")` for explicit sources
- `http(path: "../http")` for local development

#### 3.2 Version resolution

Implement semver parsing and comparison in Aster (good dogfooding exercise):
- Exact: `"1.2.0"`
- Compatible: `"~> 1.2"` (>= 1.2.0, < 2.0.0)
- Minimum: `">= 1.0.0"`

#### 3.3 Lock file

`Seedfile.lock` (or `seed.lock`), a simple format listing resolved versions and content hashes. Can be the custom format or even just Aster map literals. The lock file is NOT a DSL file, it's data. A simple parseable format is fine here.

#### 3.4 Resolution algorithm

Start with a greedy resolver (resolve each dependency to its latest compatible version, error on conflicts). A proper SAT-based resolver like Cargo's is a later optimization. The greedy approach works fine for small ecosystems.

#### 3.5 Version quarantine (supply chain protection)

Most malicious package versions are caught and yanked within hours or days of publication. A quarantine period makes the resolver ignore versions that were published too recently, so compromised releases get removed before they ever reach your project.

**Seedfile syntax:**

```
quarantine(days: 3)
```

This is a top-level directive. When set, the resolver will not install any package version published less than 3 days ago. If the only version matching a constraint is too new, the resolver errors with a clear message explaining why and when the version becomes available.

**Per-dependency override:**

Path and local dependencies skip quarantine automatically (they're your own code). For trusted registry or git dependencies that need to skip quarantine, use the `trusted: "true"` flag:

```
quarantine(days: 7)

my_internal_lib(path: "../lib")          # no quarantine (path dep)
http(version: "1.2.0")                   # subject to 7-day quarantine
hot_fix(version: "2.0.1", trusted: "true")  # skips quarantine
```

**How it works:**

1. The resolver fetches available versions for each dependency (from git tags or a registry API)
2. Each version has a publish timestamp (git tag date, or registry metadata)
3. Versions younger than the quarantine period are excluded from resolution
4. The lock file records the publish timestamp for each resolved version
5. `aster update` respects the quarantine. `aster install` with an existing lock file uses already-locked versions regardless of age (they were quarantined at lock time)

**Lock file integration:**

```
[[package]]
name = "http"
version = "1.2.0"
published = "2026-03-15T10:00:00Z"
source = "git+https://github.com/aster-lang/http?tag=v1.2.0#abc123"
```

The `published` timestamp is recorded so the quarantine check doesn't need to re-fetch metadata on every install.

**Prior art:**
- [pnpm `minimumReleaseAge`](https://pnpm.io/supply-chain-security): configurable in minutes, with an exclude list for trusted packages
- [npm `--before` flag](https://www.pcloadletter.dev/blog/npm-min-release-age/): time-based version filtering, no exclude mechanism
- [uv `--exclude-newer`](https://pydevtools.com/handbook/how-to/how-to-protect-against-python-supply-chain-attacks-with-uv/) and pip `--uploaded-prior-to`: same concept for Python

**Default:** No quarantine unless explicitly set. This is a security-conscious opt-in, not a surprise behavior. Projects that care about supply chain security add one line to their Seedfile. Projects that don't aren't affected.

#### 3.6 Dependency overrides

Force a transitive dependency to a specific version, regardless of what the intermediate dependency declared. This is a power tool for emergencies: a transitive dep has a CVE, the maintainer hasn't released a fix, and you can't wait.

**Seedfile syntax:**

```
override(name: "json", version: "0.3.0")
override(name: "crypto", git: "https://github.com/my-fork/crypto", branch: "fix-cve")
```

`override` is a real method on Seedfile (like `package`, `task`, `quarantine`). Each override replaces the source for a named dependency wherever it appears in the dependency graph.

**Semantics:**

- Overrides apply globally. If `json` appears anywhere in the transitive graph, it gets replaced.
- The override does NOT need to satisfy the original constraint. That's the point. You're saying "I know this breaks the contract, I'm doing it anyway."
- The resolver logs a warning for each constraint violation: `warning: override 'json 0.3.0' violates constraint '= 0.2.0' from 'http'`
- Overrides are recorded in the lock file so `aster install` reproduces the same result.
- Path overrides work too: `override(name: "json", path: "../json-patched")` for local fixes.

**Seedfile class addition:**

```
class Override
    name: String
    version: String
    path: String
    git: String
    branch: String
```

With a real method on Seedfile:

```
pub def override(name: String, version: String = "", path: String = "", git: String = "", branch: String = "") -> Void
    overrides.push(item: Override(name: name, version: version, path: path, git: git, branch: branch))
```

#### 3.7 Vendoring

`aster vendor` copies all resolved dependencies into a `vendor/` directory inside the project. After vendoring, builds use the local copies with no network access required.

**How it works:**

1. `aster vendor` reads `Seedfile.lock`
2. For each dependency, copies the source into `vendor/<name>/`
3. Writes a `vendor/manifest` file listing what was vendored and from where
4. Subsequent `aster build` detects `vendor/` and uses it as the package source instead of `.aster/packages/`

**Use cases:**

- CI/CD environments where network access is restricted or unreliable
- Air-gapped deployments
- Auditing: "I want to read every line of code my project ships with"
- Reproducibility: the vendor directory is committed, so every checkout is identical

**CLI:**

| Command | Description |
|---------|-------------|
| `aster vendor` | Copy all deps into `vendor/` |
| `aster vendor --check` | Verify `vendor/` matches `Seedfile.lock` (for CI) |

**Project structure with vendoring:**

```
my-project/
    Seedfile
    Seedfile.lock
    src/
        main.aster
    vendor/
        http/
            src/lib.aster
        json/
            src/lib.aster
        manifest
```

Vendored deps are plain source. No special format. The resolver just reads from `vendor/` instead of the global cache.

### Phase 4: The `aster` CLI

#### 4.1 Subcommands

**Passthrough commands** (delegate to `asterc` with the same args):

| Command | Description |
|---------|-------------|
| `aster build` | Compile the project (`asterc build`) |
| `aster run` | Build and execute (`asterc run`) |
| `aster test` | Run test files (`asterc test`) |
| `aster check` | Type-check without compiling (`asterc check`) |
| `aster fmt` | Format source files (`asterc fmt`) |

**Package management commands** (handled by `aster` directly):

| Command | Description |
|---------|-------------|
| `aster init` | Create a new project with a Seedfile |
| `aster add <pkg>` | Add a dependency to the Seedfile |
| `aster remove <pkg>` | Remove a dependency |
| `aster install` | Fetch and install all dependencies |
| `aster update` | Update dependencies to latest compatible versions |
| `aster lock` | Generate/update the lock file |
| `aster vendor` | Copy all deps into `vendor/` for offline builds |
| `aster task <name>` | Run a named task from the Seedfile |

#### 4.2 Bootstrap problem

The `aster` CLI needs to be written in Aster, but it also needs to run Aster. The bootstrap sequence:

1. `asterc` (Rust) remains the raw compiler
2. The `aster` CLI is written in Aster and compiled with `asterc`
3. The `aster` binary ships as a pre-compiled native binary (AOT compiled)
4. `aster build` internally invokes `asterc` for compilation

This is similar to how `cargo` is written in Rust and compiled with `rustc`.

#### 4.3 Project structure

```
my-project/
    Seedfile
    Seedfile.lock
    src/
        main.aster
        lib/
    test/
        main_test.aster
    .aster/
        packages/          # fetched dependencies
        cache/             # build cache
```

### Phase 5: Toolchain Manager

Manage multiple `asterc` compiler versions. Install, switch, pin per-project via `compiler(version: "0.2.0")` in the Seedfile. Shim-based dispatch from the `aster` binary to the correct `asterc`. Prebuilt binary downloads, checksum verification, local toolchain linking for compiler developers.

Full plan: [toolchain-manager.md](toolchain-manager.md)

### Phase 6: Package Registry (Future)

Not blocking for v1. Git-based fetching works. A central registry (`registry.aster.dev` or similar) is a nice-to-have.

## Execution Order

The phases have a strict dependency chain:

```
Phase 0 (OS primitives) — DONE
    0.1 argv
    0.2 env vars
    0.3 process spawning
    0.4 filesystem ops
    0.5 hashing
        |
Phase 1 (method_missing / dynamic dispatch) — DONE
    1.1 design
    1.2 implementation
        |
Phase 2 (Seedfile DSL)
    2.1 stdlib module
    2.2 format design (includes compiler() function)
    2.3 evaluation pipeline
        |
Phase 3 (dependency resolution)
    3.1 package source
    3.2 semver
    3.3 lock file
    3.4 resolver
    3.5 quarantine
    3.6 overrides
    3.7 vendoring
        |
Phase 4 (aster CLI)
    4.1 subcommands
    4.2 bootstrap
    4.3 project structure
        |
Phase 5 (toolchain manager)
    5.1 directory structure + defaults
    5.2 version discovery + download
    5.3 shim dispatch (Seedfile compiler_version -> right asterc)
    5.4 toolchain linking (local dev builds)
    5.5 cleanup + removal
```

Phase 0 items (0.1-0.5) can be done in parallel. Phase 1 can start as soon as the design is settled (doesn't depend on Phase 0). Phases 2-5 are sequential. Phase 5 depends on the aster CLI (Phase 4) being functional, since toolchain commands are subcommands of `aster`.

## Potential Challenges & Mitigations

1. **Challenge**: method_missing in a statically typed language is inherently at odds with compile-time safety.
   **Mitigation**: Restrict it to classes that explicitly opt in via `includes DynamicReceiver`. The typechecker only relaxes rules in that specific context. Regular code is unaffected.

2. **Challenge**: Seedfile evaluation requires JIT-compiling Aster to read project config, adding latency to every `aster` command.
   **Mitigation**: Cache the parsed Seedfile. Only re-evaluate when the file's mtime changes. The JIT is already fast for small programs.

3. **Challenge**: Writing a dependency resolver in Aster means the language needs solid Map/String/List operations under real workload.
   **Mitigation**: This is the point. If the resolver is painful to write, that's feedback about the language. Fix the language, not the plan.

4. **Challenge**: No HTTP client means the registry can't be web-based initially.
   **Mitigation**: Shell out to `git` for package fetching. Git is installed everywhere. HTTP registry comes after networking is implemented (or via the Rust FFI RFC using `reqwest`).

5. **Challenge**: The bootstrap problem. Building the `aster` CLI requires a working Aster compiler, but the CLI is how users interact with Aster.
   **Mitigation**: Ship `asterc` as the initial binary. The `aster` CLI is an upgrade that wraps `asterc`. Users can always fall back to `asterc` directly.

## Unwired Code Audit

- [x] Seedfile DSL functions (`package`, `method_missing`) are defined (producer) and called by Seedfile evaluation (consumer)
- [x] argv runtime functions (producer: OS) are consumed by CLI argument parsing
- [x] Process spawning (producer: runtime) is consumed by `aster build`, `aster test`, package fetching
- [x] Filesystem ops (producer: runtime) are consumed by package installation, project init, cache management
- [x] Lock file is written by `aster lock`/`aster install` (producer) and read by `aster build` (consumer)
- [x] method_missing rewrite in typechecker (producer) generates FIR that codegen consumes
- [ ] `quarantine(days:)` (producer: Seedfile) is consumed by the resolver to filter versions by publish date
- [ ] `override(name:, ...)` (producer: Seedfile) is consumed by the resolver to replace transitive deps
- [ ] `aster vendor` (producer: lock file + fetched packages) writes to `vendor/` consumed by `aster build`
- [ ] `trusted: "true"` on Dependency (producer: Seedfile) is consumed by quarantine check to skip filtering

## Validation Steps

- Seedfile can declare a package with name, version, and dependencies
- `aster init` creates a valid project structure
- `aster add http` modifies the Seedfile
- `aster install` fetches git-based dependencies into `.aster/packages/`
- `aster build` compiles a project with dependencies
- `aster run` builds and executes
- Lock file is generated and respected on subsequent installs
- method_missing works in a general-purpose context (not just Seedfiles)
- Round-trip: create project, add deps, build, run, all through `aster` CLI
- `quarantine(days: 3)` causes resolver to reject versions published < 3 days ago
- `trusted: "true"` on a dep skips quarantine for that dep
- `override(name: "json", version: "0.3.0")` replaces json everywhere in the graph
- Override that violates a constraint logs a warning but succeeds
- `aster vendor` copies all deps to `vendor/`, subsequent builds use them
- `aster vendor --check` fails if `vendor/` doesn't match `Seedfile.lock`

## Future Considerations

Features that are worth building but not blocking for v1. These should each get their own RFC when the time comes.

- **`aster audit`**: Check all dependencies against a known-vulnerabilities database. Requires a vulnerability database service, which doesn't exist for Aster yet. Pairs well with quarantine for defense-in-depth.
- **Checksum verification**: Store content hashes (SHA-256) per dependency in the lock file. Verify fetched content matches on every `aster install`. Catches tampering between lock time and install time.
- **`aster outdated`**: Show which dependencies have newer versions available (respecting quarantine). Quick view of what's drifting.
- **`aster why <pkg>`**: Explain why a transitive dependency exists. "json 0.2.0 is required by http 1.2.0, which is a direct dependency." Useful for debugging resolution conflicts.
- **Offline mode**: `aster install --offline` uses only the local cache, never hits the network. Fails if anything is missing. Lighter than full vendoring.
- **License checking**: Verify all dependencies use licenses compatible with the project's license. Configurable allow/deny lists in the Seedfile.
- **Platform-specific dependencies**: Different deps per OS/architecture. Relevant when the ecosystem grows to include native extensions.
