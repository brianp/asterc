# Plan: The Testing Story

status: pending
issue: https://github.com/brianp/asterc/issues/2

`asterc test` plus `std/test`. Convention-core, no DSL, no runtime eval. The
harness and the formatter are both extension points from day one, because
Rust locking libtest in is not a good look and we're not repeating it.

## Grounding (read 2026-08-12: minitest 5.25.4 source, spectacular source, RSpec from use)

- **minitest**: discovery is `methods_matching(/^test_/)`. The spec DSL is a
  veneer: `it "desc"` executes `define_method("test_%04d_desc")`, `before`/
  `after` become `setup`/`teardown` chained via `super()` (setup parent-first,
  teardown reverse). Default run order is a seeded shuffle (`methods.sort.shuffle`
  under a printed seed) so order-coupled tests surface early and reproduce exactly.
- **spectacular**: two front-ends (`spec!` describe/it and `#[test_suite]` +
  `#[test]`) generating the same standard test functions. Its real contributions:
  three stacking hook layers with explicit opt-in, and typed context injection
  (`before -> T` gives shared `&T`, `before_each -> T` gives owned `T` that the
  test receives and `after_each` consumes). The runner wraps `cargo test
  --format json` and feeds pluggable formatters (default/boring/pride) — a
  workaround for libtest being closed; we build the seam natively instead.
- **RSpec**: the `let`/`subject`/shared-examples/metadata layer is deliberately
  rejected — lazy memoized magic is the aliasing entropy the philosophy RFC bans.
- **The cross-framework law**: every framework converges on a boring convention
  core with the DSL as optional sugar that lowers to it. We build only the core.
  A BDD layer comes later as its own library (see Deferred).

## Design

### Conventions (decided 2026-08-13)

Two test locations, one purpose each — not two ways to do the same thing:

- **Integration tests**: `*_test.aster` under `tests/` from the project root
  (Seedfile root, then `.aster/`, then `.git/` — same discovery as the build
  system). These exercise the project's `pub` API only. Colocation
  (`foo_test.aster` beside `foo.aster`) is rejected.
- **Unit tests**: top-level `def test_*` functions inside ordinary source
  files, Rust-inline-module style. They see the file's private items. No
  `#[cfg(test)]` machinery: the `test_` naming convention IS the conditional —
  the lowerer skips `test_*` defs outside test mode (a special case of
  dead-code elimination; relates to #45). `test_` is a reserved-by-convention
  prefix on top-level defs, documented.
- Tests: zero args, or one arg for context injection (below). May declare
  `throws`.
- Classification per test, via typed catch in the harness: clean return = PASS,
  `AssertionError` = FAIL (message shown), any other `Error` = ERROR.

### `std/test` (free functions, per API style)

- `assert(that: Bool)` / `assert(that: Bool, message: String)`
- `assert_eq(actual: T, expected: T)` where `T includes Eq`
- `assert_ne(actual: T, expected: T)` where `T includes Eq`
- `assert_throws(f: Fn() -> Void)` — passes if `f` throws, with a typed
  variant to be designed once we see real usage
- All throw `AssertionError extends Error` carrying the failure message.
- Failure rendering (decided): `Eq` is the only requirement. When the type
  includes `Printable`, failures render values via `debug()` (which defaults
  to `to_string()`); without it, the message degrades to "values differ".
  No second function, no Printable requirement.
- Arity-1 (#52) applies: `assert(x > 3)` reads clean.
- These are deliberately simple enough to become the first tenants of an
  Aster-source stdlib layer later.

### Hooks and context injection (the spectacular idea, as plain conventions)

- Optional top-level `def before_each()` / `def after_each()` in a test file.
- If `before_each` returns a value (`def before_each() -> Ctx`), any `test_*`
  def declaring a `Ctx` parameter receives it, and `after_each(ctx: Ctx)` may
  consume it for teardown. Type-driven wiring, synthesized by the harness, no
  registration.
- `Drop`/`Close` already run on scope exit, so RAII covers most teardown;
  `after_each` is for the rest.
- Suite-level (cross-file) hooks: deferred until wanted. Explicit opt-in like
  spectacular's `suite;` if it happens.

### The default harness (compiler-synthesized, like `__top_main`)

For each test file, `asterc test` synthesizes an entry that:
1. Shuffles the discovered `test_*` defs under a seed (printed; `--seed N`
   reproduces — minitest's discipline, adopted day one).
2. Threads hooks/context around each test.
3. Wraps each call in typed catch and reports an event per outcome.
4. Aggregates counts; nonzero exit on any FAIL/ERROR.

### The formatter seam (open by default)

The harness does not print. It emits typed events (`run started`, `test
started`, `passed`, `failed(message)`, `errored(error)`, `run finished
(counts, seed, duration)`) to a `Formatter` trait defined in `std/test`.
Everything that renders is a formatter, including future machine output:

- **`pride`** — the default. Rainbow output in the spectacular/minitest
  lineage. The default should have personality.
- **`plain`** — no color, boring, for CI logs and greppers.
- **`json`** — NDJSON events, later; this is where the TOONS thread (#22)
  plugs in rather than a second design.

Selection: `--formatter name`, with a Seedfile default
(`test(formatter: "plain")`) once the pkg CLI grows test config. No TTY
auto-detection (decided): pride is the default everywhere, including CI —
you get what you ask for; want something else, ask for it. Custom
formatters are Aster classes including `Formatter`, compiled in from the
project's tests tree by convention (exact registration convention to be
settled in implementation — likely a `pub def formatter() -> F` in
`tests/formatter.aster`). Dispatch is monomorphized at harness synthesis;
no vtables needed. Community formatters become distributable packages once
the package manager lands — a deliberate dogfood target.

### The harness seam (the anti-libtest clause)

If a project defines the convention entry `def test_main(tests: List[TestCase])`
(where `TestCase` carries `name: String` and a callable), the compiler hands it
the discovered test list instead of synthesizing the default loop. Discovery
stays free; execution policy becomes yours. This is Rust's `harness = false`
except you keep discovery, hooks metadata, and the event/formatter contract.

### CLI

`asterc test [filter] [--seed N] [--formatter name]` — filter is a substring
match on test names, minitest-style. Runs all discovered test files when no
path given.

## Sequencing prerequisites (decided 2026-08-13)

1. **#43 — root-relative module resolution** is a hard prerequisite: a test
   in `tests/` must `use geometry { Point }` against the project's modules.
   The chain is #43 → testing story → hand-written resolver.
2. **#15 — the stacktrace story** gets worked before (or with) the
   failure-location task below. Assertion call-site spans injected at lowering
   (`#[track_caller]`-style) cover the common case; traces cover the rest.
   The event format reserves a trace field either way.

## Tasks

1. **`AssertionError` + `std/test` assertions** — builtin error class with
   typed tag; register `std/test` exports; FIR lowering for the assert
   functions (throw-on-false with message formatting).
2. **Discovery + default harness synthesis** — project-root test-file walk,
   `test_*` enumeration in the typechecker, harness entry synthesis in the
   lowerer (seeded shuffle, typed catch classification), `asterc test`
   subcommand with exit codes.
3. **Formatter trait + events + pride/plain** — trait in `std/test`, event
   dispatch from the harness, the two built-in formatters, `--formatter`.
4. **Hooks + context injection** — `before_each`/`after_each` detection,
   typed context threading, teardown-runs-on-failure semantics.
5. **Filter + `--seed`** — substring filtering, seed print/replay.
6. **Custom harness hook** — `test_main(tests:)` convention, `TestCase` type.
7. **Dogfood + docs** — tests for aster-pkg's Seedfile code written with the
   new framework; docs pages (tooling/testing + status/roadmap updates); this
   becomes the proving ground before the resolver is hand-written.

## Deferred (seams designed, work not started)

- **BDD layer** — describe/it with a DSL, as its own library/package, lowering
  to the `test_*` convention exactly as minitest/spec does. Not in the
  compiler. Not now.
- **Suite-level hooks** — spectacular's third layer, opt-in only.
- **`--format json` / TOONS integration** (#22) — the Formatter trait is the
  socket; the JSON formatter is the plug.
- **Parallel execution** — blocked on #49 (cross-worker GC). Do not
  parallelize the harness before that is fixed and tested.
- **Stack traces on failures** (#15) — failure output improves when traces
  exist; the event format should leave room for a trace field.
