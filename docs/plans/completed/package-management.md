---
status: superseded
created: 2026-03-12 00:00
executed: null
superseded_by: ../docs/plans/package-manager.md
---

# Implementation Plan: Aster Package Management (SUPERSEDED)

**This plan is outdated.** The current package manager plan is at `workingfiles/docs/plans/package-manager.md`. Key difference: the Seedfile uses an Aster DSL (powered by DynamicReceiver), not TOML. The package manager itself will be written in Aster, not as Rust tooling in the compiler.

## Context (historical)

Aster needs a package ecosystem. The MCP server is the first project that will live outside the compiler repo and depend on stdlib modules. Before that can happen, we need: a package format, a manifest, dependency resolution, and source fetching.

**Inspirations:** Cargo (Rust), Bundler (Ruby), Pub (Dart). All three share the same core model — a manifest declares dependencies with version constraints, a lock file pins exact versions, and a resolver finds a compatible set. We replicate this model.

**Naming:** Packages are **seeds**. The registry is **Asterfoundry** (asterfoundry.dev). The CLI tool is `asterc` itself (subcommands: `asterc seed init`, `asterc seed add`, `asterc seed install`, etc.).

The metaphor: Aster is a flower. Seeds grow into projects. Asterfoundry is where seeds are forged and shared.

## Design Principles

1. **One manifest, one lock.** `seed.toml` declares intent. `seed.lock` records reality. Both live in the project root.
2. **Minimal viable resolution.** Start with simple version resolution (compatible-with / `^` semantics by default). Graduate to SAT solving only if the ecosystem grows large enough to need it.
3. **Git-first, registry-ready.** Seeds can come from git repos today. The registry is a future source that plugs into the same resolution pipeline.
4. **No magic.** `asterc seed install` fetches and resolves. There is no implicit fetch on build. The developer controls when network access happens.
5. **Convention over configuration.** Standard directory layout. No `src` vs `lib` debate — it's just `src/`.

## Package Format

### Directory Structure

```
my-seed/
  seed.toml              -- manifest (name, version, deps, metadata)
  seed.lock              -- pinned dependency graph (generated, committed)
  src/
    main.aster           -- entry point (if binary)
    lib.aster            -- entry point (if library)
    json/
      parser.aster       -- submodules follow directory structure
      serializer.aster
  tests/
    test_parser.aster    -- test files
  examples/
    basic.aster          -- example programs
  README.md              -- optional
  LICENSE                -- optional
```

**Convention:** If `src/main.aster` exists, it's a binary seed. If `src/lib.aster` exists, it's a library seed. Both can coexist (like Cargo). The entry point determines what gets compiled and exported.

### seed.toml

```toml
[seed]
name = "aster-mcp"
version = "0.1.0"
description = "MCP server for the Aster compiler"
authors = ["Tari <tari@example.com>"]
license = "MIT"
aster = ">=0.1.0"            # minimum compiler version

[dependencies]
json = { git = "https://github.com/aster-lang/json", tag = "v0.2.0" }
toon = { git = "https://github.com/aster-lang/toon", branch = "main" }

[dev-dependencies]
assert = { git = "https://github.com/aster-lang/assert", tag = "v0.1.0" }

# Future: registry dependencies
# http = "1.2.0"
# http = { version = "1.2.0", registry = "asterfoundry" }

[seed.bin]
name = "aster-mcp"           # binary name (defaults to seed name)
```

### Field Reference

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Seed name. Lowercase, hyphens allowed. Must be unique on registry. |
| `version` | Yes | SemVer (MAJOR.MINOR.PATCH). |
| `description` | No | One-line description for registry listing. |
| `authors` | No | List of author strings. |
| `license` | No | SPDX identifier. |
| `aster` | No | Minimum compiler version constraint. |
| `dependencies` | No | Map of dependency name → source specifier. |
| `dev-dependencies` | No | Dependencies only needed for tests/examples. |

### Dependency Source Specifiers

```toml
# Git sources (available now)
json = { git = "https://github.com/aster-lang/json" }                    # HEAD of default branch
json = { git = "https://github.com/aster-lang/json", branch = "main" }   # specific branch
json = { git = "https://github.com/aster-lang/json", tag = "v0.2.0" }    # specific tag
json = { git = "https://github.com/aster-lang/json", rev = "abc123" }    # specific commit

# Path sources (for local development / monorepos)
json = { path = "../json" }

# Registry sources (future — when Asterfoundry exists)
json = "0.2.0"                                                            # shorthand: ^0.2.0
json = { version = ">=0.2.0, <0.4.0" }                                   # explicit range
json = { version = "0.2.0", registry = "asterfoundry" }                  # explicit registry
```

### Version Constraint Syntax

Follow Cargo's conventions — these are well-understood:

| Syntax | Meaning | Example |
|--------|---------|---------|
| `"1.2.3"` | `^1.2.3` (compatible) | `>=1.2.3, <2.0.0` |
| `"^1.2.3"` | Compatible with | `>=1.2.3, <2.0.0` |
| `"~1.2.3"` | Patch-level only | `>=1.2.3, <1.3.0` |
| `">=1.0.0"` | Minimum | `>=1.0.0` |
| `">=1.0.0, <2.0.0"` | Range | `>=1.0.0, <2.0.0` |
| `"=1.2.3"` | Exact | `=1.2.3` |

Default (bare version string) is `^` (caret/compatible), matching Cargo.

## seed.lock

Generated by `asterc seed install`. Committed to version control. Records the exact resolved version and source for every dependency (transitive included).

```toml
# This file is auto-generated by asterc. Do not edit.

[[seed]]
name = "json"
version = "0.2.1"
source = "git+https://github.com/aster-lang/json?tag=v0.2.0#abc123def456"

[[seed]]
name = "toon"
version = "0.1.0"
source = "git+https://github.com/aster-lang/toon?branch=main#789abc012def"
dependencies = ["json"]

[[seed]]
name = "assert"
version = "0.1.0"
source = "git+https://github.com/aster-lang/assert?tag=v0.1.0#456789abcdef"
dev = true
```

**Lock file semantics:**
- `asterc seed install` reads `seed.toml`, resolves, writes `seed.lock`, fetches sources
- `asterc seed install` with existing `seed.lock` uses locked versions (fast path)
- `asterc seed update` re-resolves from `seed.toml`, updates `seed.lock`
- `asterc seed update json` re-resolves only `json` and its dependents

## Dependency Resolution

### Algorithm

Phase 1 (simple, sufficient for early ecosystem):

1. Parse `seed.toml` for all direct dependencies
2. For each dependency, fetch its `seed.toml` to discover transitive deps
3. Build a dependency graph
4. For version conflicts: pick the highest version that satisfies all constraints
5. If no compatible version exists, report the conflict with both constraint sources
6. Write resolved graph to `seed.lock`

Phase 2 (when ecosystem needs it):

- Implement PubGrub-style version solving (same algorithm Cargo and Pub use)
- Handles complex constraint satisfaction and produces clear error messages on conflicts

### Conflict Example

```
my-project depends on json ^0.2.0
toon depends on json ^0.3.0

Resolution: json 0.3.x (satisfies both ^0.2.0 and ^0.3.0)
            → 0.3.x is >=0.2.0,<1.0.0 ✓ and >=0.3.0,<1.0.0 ✓

my-project depends on json ^0.2.0
toon depends on json ^1.0.0

Conflict: no version satisfies both ^0.2.0 (<1.0.0) and ^1.0.0 (>=1.0.0)
          → error with explanation
```

## Source Fetching & Cache

### Global Cache

Seeds are fetched once and cached globally:

```
~/.aster/
  cache/
    git/
      github.com-aster-lang-json-abc123/     -- git checkout at specific rev
      github.com-aster-lang-toon-789abc/
    registry/
      asterfoundry/
        json-0.2.1.tar.gz                    -- future: downloaded tarballs
        json-0.2.1/                           -- extracted
```

### Project-Local Link

After resolution, dependencies are available to the compiler via a local `.aster/seeds/` directory:

```
my-project/
  .aster/
    seeds/
      json/          -- symlink or copy from global cache
      toon/
```

The compiler's module loader resolves `use json { Parser }` by looking in `.aster/seeds/json/src/lib.aster`.

## CLI Commands

### `asterc seed init`

Create a new seed project:

```
$ asterc seed init my-project
Created seed 'my-project' at ./my-project/
  seed.toml
  src/main.aster
```

Options:
- `--lib` — create library seed (`src/lib.aster` instead of `src/main.aster`)
- `--name <name>` — override inferred name

### `asterc seed add <name> <source>`

Add a dependency:

```
$ asterc seed add json --git https://github.com/aster-lang/json --tag v0.2.0
Added json (git: https://github.com/aster-lang/json, tag: v0.2.0)
```

```
$ asterc seed add json 0.2.0    # future: registry shorthand
Added json ^0.2.0
```

- `--dev` — add to `[dev-dependencies]`
- `--path <path>` — use local path source

### `asterc seed install`

Resolve dependencies and fetch sources:

```
$ asterc seed install
Resolving dependencies...
  json v0.2.1 (git+https://github.com/aster-lang/json#abc123)
  toon v0.1.0 (git+https://github.com/aster-lang/toon#789abc)
Fetched 2 seeds.
```

If `seed.lock` exists and is up to date, this is a no-op (fast).

### `asterc seed update [name]`

Re-resolve and update lock file:

```
$ asterc seed update json
Updating json...
  json v0.2.1 -> v0.2.3
Updated seed.lock.
```

Without a name, updates all dependencies.

### `asterc seed remove <name>`

Remove a dependency from `seed.toml` and `seed.lock`.

### `asterc seed list`

Show resolved dependency tree:

```
$ asterc seed list
my-project v0.1.0
├── json v0.2.1 (git+https://github.com/aster-lang/json#abc123)
└── toon v0.1.0 (git+https://github.com/aster-lang/json#789abc)
    └── json v0.2.1 (*)
```

### `asterc seed publish` (future)

Publish to Asterfoundry registry. Validates `seed.toml`, creates tarball, uploads.

## Integration with Compiler

### Module Resolution Order

When the compiler encounters `use json { Parser }`:

1. **Stdlib** — check `std/` built-in modules first
2. **Project local** — check project `src/` directory
3. **Seeds** — check `.aster/seeds/<name>/src/lib.aster`
4. **Error** — "module 'json' not found. Did you forget to add it to seed.toml?"

This extends the existing module loader (`typecheck/src/module_loader.rs`).

### Build Integration

`asterc build` and `asterc run` should:

1. Check that `seed.lock` exists and matches `seed.toml` (warn if stale, error if missing deps)
2. Include seed source directories in the compilation unit
3. Compile seeds before the project (dependency order)

**No implicit fetch.** If seeds aren't installed, error with: "Run `asterc seed install` first."

## Asterfoundry Registry (Future — Separate RFC)

The registry is out of scope for this plan but the package format is designed to support it. Key decisions to lock now:

- **Domain:** asterfoundry.dev (or .com — whichever is available)
- **API:** REST, JSON responses. `GET /api/v1/seeds/<name>` returns versions and metadata. `GET /api/v1/seeds/<name>/<version>/download` returns tarball.
- **Auth:** API tokens for publishing. Read is public and unauthenticated.
- **Namespace:** Flat (no scopes/orgs initially). First-come-first-served on names.
- **Immutability:** Published versions cannot be modified or deleted (only yanked).
- **Yanking:** Yanked versions are excluded from resolution but remain downloadable for existing lock files.
- **Index:** Git-based index (like crates.io) or API-based (like pub.dev). Decision deferred.

The registry needs its own RFC covering: hosting infrastructure, moderation policy, name squatting rules, security (signing, checksums), and mirroring.

## Task Breakdown

### Phase 1: Package Format & Manifest

#### 1.1 TOML parsing
- **Approach:** Add `toml` crate to the compiler's Rust dependencies. Parse `seed.toml` into Rust structs.
- **Files to create:** `src/seed/manifest.rs` (SeedManifest, Dependency, SourceSpec structs)
- **Files to modify:** `Cargo.toml` (add `toml` dep), `src/main.rs` (register `seed` subcommand)

#### 1.2 `asterc seed init`
- **Approach:** Create directory, write template `seed.toml` and `src/main.aster` (or `lib.aster`).
- **Files to create:** `src/seed/init.rs`

#### 1.3 `asterc seed add` / `asterc seed remove`
- **Approach:** Parse existing `seed.toml`, add/remove dependency entry, write back.
- **Files to create:** `src/seed/edit.rs`

### Phase 2: Git Source Fetching

#### 2.1 Git clone & checkout
- **Approach:** Shell out to `git` (like Cargo does for git deps). Clone to global cache (`~/.aster/cache/git/`). Checkout specific tag/branch/rev.
- **Files to create:** `src/seed/git.rs`
- **Key decision:** Shell out to `git` rather than using libgit2. Keeps dependencies minimal and `git` is universally available.

#### 2.2 Cache management
- **Approach:** Hash the git URL + ref to derive cache directory name. Check if cached checkout exists and matches expected rev. Symlink from `.aster/seeds/<name>` to cached checkout.
- **Files to create:** `src/seed/cache.rs`

### Phase 3: Dependency Resolution

#### 3.1 Graph construction
- **Approach:** Parse each dependency's `seed.toml` (from fetched source) to discover transitive deps. Build a DAG. Detect cycles.
- **Files to create:** `src/seed/resolve.rs`

#### 3.2 Version resolution
- **Approach:** For each dependency, collect all version constraints from all dependents. Find the highest version satisfying all constraints. Report conflicts with both constraint sources.
- **Implementation notes:** Start with greedy resolution. Upgrade to PubGrub if/when needed.

#### 3.3 Lock file generation
- **Approach:** Write resolved graph to `seed.lock` in TOML format. Include source URLs, exact revisions, and dependency relationships.
- **Files to create:** `src/seed/lock.rs`

#### 3.4 `asterc seed install` / `asterc seed update`
- **Approach:** Orchestrate: read manifest → resolve → fetch → link → write lock.
- **Files to create:** `src/seed/install.rs`

### Phase 4: Compiler Integration

#### 4.1 Module loader seed resolution
- **Files to modify:** `typecheck/src/module_loader.rs`
- **Approach:** Add seed directory to module search path. When resolving `use json { ... }`, check `.aster/seeds/json/src/lib.aster`.

#### 4.2 Build order
- **Files to modify:** `src/main.rs`
- **Approach:** Before compiling the project, compile all seeds in dependency order. Cache seed compilation artifacts in `.aster/build/<profile>/seeds/`.

#### 4.3 Stale lock detection
- **Files to modify:** `src/main.rs`
- **Approach:** On `asterc build`/`asterc run`, compare `seed.toml` mtime against `seed.lock` mtime. If toml is newer, warn. If `.aster/seeds/` is missing, error with install instructions.

### Phase 5: Path Dependencies

#### 5.1 Local path resolution
- **Approach:** For `{ path = "../json" }` deps, resolve relative to project root. No caching needed — use the path directly.
- **Files to modify:** `src/seed/resolve.rs`, `src/seed/cache.rs`

This is critical for monorepo development and for developing seeds alongside projects that use them.

## Blockers & Prerequisites

These are things that need to exist or be true before the package manager phases can land cleanly. Some are hard blockers (the phase literally can't work without them), others are soft blockers (technically possible to skip but you'll pay for it later).

### Hard Blockers

1. **`toml` and `semver` Rust crate dependencies.**
   Needed in Phase 1.1 (manifest parsing) and Phase 3.2 (version resolution). Neither is in `Cargo.toml` today. Adding them is trivial but must happen before any seed code compiles.

2. **Module loader support for multiple roots.**
   `FsResolver` currently takes a single `root: PathBuf`. Phase 4.1 needs it to search `.aster/seeds/<name>/src/lib.aster` in addition to the project's own `src/`. This is the compiler integration bottleneck. The abstraction is there (`FileResolver` trait), but `FsResolver` needs a search path list or a fallback chain.

3. **Seed compilation ordering.**
   Phase 4.2 requires compiling seeds before the project, in dependency order. The build system (`.aster/build/`) exists and handles single-project compilation with caching. It doesn't handle multi-project dependency-ordered compilation. The build pipeline in `src/main.rs` runs lex, parse, typecheck, lower, codegen for one file tree. It needs to loop over resolved seeds first, each producing cached artifacts.

### Soft Blockers

4. **No CLI framework.**
   Argument parsing in `src/main.rs` is hand-rolled. Adding `asterc seed init/add/install/update/remove/list/publish` by hand is doable but gets messy. Consider whether to adopt `clap` or keep hand-rolling. Not blocking, just annoying.

5. **No multi-module compilation in a single invocation.**
   `asterc build` currently takes a single file. A seed project has `src/lib.aster` which may `use` submodules, and those submodules are resolved via the module loader. This already works for the project itself. But for seeds that depend on other seeds, the module loader needs to see all resolved seeds in the search path during typechecking. This is related to blocker #2 but distinct: it's about the full compilation pipeline seeing the whole resolved dependency graph, not just one seed at a time.

6. **No `pub` exports enforcement across seed boundaries.**
   The visibility system (`pub` keyword) works within a single project's module tree. When a seed exports types via `src/lib.aster`, the consuming project should only see `pub` items. The module loader currently imports all public names from a resolved module. This probably works already, but needs validation since seed boundaries are a new trust boundary.

7. **Unstable flag transitivity.**
   There's a TODO in `module_loader.rs` about propagating `--unstable` through the dependency chain. The design doc mentions `unstable(enabled: true)` in seed.toml. This isn't blocking Phase 1-3 (no seeds use unstable features yet) but will bite once any seed includes FieldAccessible or future unstable traits.

### Not Blockers (Ready to Go)

- **Build directory infrastructure** (`.aster/build/`, manifest caching, profiles) is implemented and working.
- **Module resolution abstraction** (`FileResolver` trait, `VirtualResolver` for tests) is clean and extensible.
- **CLI subcommand pattern** is established (`check`, `run`, `build`, `fmt`, `clean`). Adding `seed` follows the same shape.
- **Serde** is already a dependency (used for build manifests). Parsing TOML into serde-compatible structs will work.

## Potential Challenges & Mitigations

1. **Challenge:** TOML parsing adds a Rust dependency to the compiler
   **Mitigation:** `toml` crate is small and well-maintained. This is the compiler's tooling, not the language runtime — Rust deps are fine here.

2. **Challenge:** Git operations are slow for large repos
   **Mitigation:** Use `--depth 1` shallow clones by default. Full clone only when `rev` specifies a non-tip commit.

3. **Challenge:** Circular dependencies between seeds
   **Mitigation:** Detect cycles during graph construction. Error with the cycle path.

4. **Challenge:** Aster's module system currently doesn't support external module roots
   **Mitigation:** Phase 4.1 extends the module loader. This is a straightforward search path addition.

5. **Challenge:** No SemVer parsing in Aster or the compiler today
   **Mitigation:** Use the `semver` Rust crate in the compiler tooling. Aster programs don't need to parse SemVer — only the package manager does.

## Unwired Code Audit

- [ ] `seed.toml` is written by `seed init` (1.2) AND read by `seed install` (3.4) AND read by `seed add/remove` (1.3)
- [ ] `seed.lock` is written by `seed install` (3.4) AND read by build integration (4.3) AND read by `seed install` fast path (3.4)
- [ ] Global cache is written by git fetch (2.1) AND read by cache check (2.2) AND symlinked by install (3.4)
- [ ] `.aster/seeds/` symlinks are created by install (3.4) AND consumed by module loader (4.1)
- [ ] Version constraints in `seed.toml` are parsed by resolver (3.2) AND validated against available versions (3.2)
- [ ] Stale lock detection (4.3) checks relationship between `seed.toml` (1.1) and `seed.lock` (3.3)

## Validation Steps

- [ ] `asterc seed init my-project` creates valid directory structure with `seed.toml`
- [ ] `asterc seed init --lib my-lib` creates library layout with `src/lib.aster`
- [ ] `asterc seed add json --git <url> --tag v0.1.0` adds dependency to `seed.toml`
- [ ] `asterc seed install` clones git dep, resolves, writes `seed.lock`, creates `.aster/seeds/` symlinks
- [ ] `asterc seed install` (second run) is a fast no-op
- [ ] `asterc seed update` re-resolves and updates lock
- [ ] `asterc seed list` shows dependency tree
- [ ] `asterc seed remove json` removes from `seed.toml` and `seed.lock`
- [ ] `asterc build` in a seed project compiles deps in order, then project
- [ ] `use json { Parser }` resolves to `.aster/seeds/json/src/lib.aster`
- [ ] Circular dependency detected and reported clearly
- [ ] Missing `seed.lock` on build produces helpful error
- [ ] Path dependencies work for local development
- [ ] Two seeds depending on different-but-compatible versions of a third resolve correctly
- [ ] Two seeds depending on incompatible versions produce a clear conflict error
