# Implementation Plans

Working design/implementation plans for the Aster compiler. Executed plans live in
[`completed/`](completed/); the audited source of truth for what is actually
implemented is the docs site's
[Implementation Status](../src/content/docs/reference/status.mdx) page — plan
frontmatter drifts, the status page is verified against code (last full audit:
2026-08-12).

## Active

| Plan | State |
|---|---|
| [arity-1-args.md](arity-1-args.md) | Pending — decided rule ([#52](https://github.com/brianp/asterc/issues/52)), not yet implemented |
| [testing-story.md](testing-story.md) | Pending — grounded design ([#2](https://github.com/brianp/asterc/issues/2)); asterc test + std/test, open formatter/harness seams |
| [stacktraces.md](stacktraces.md) | Pending — native FP-walk capture at throw ([#15](https://github.com/brianp/asterc/issues/15)); prerequisite-adjacent to testing |
| [package-manager.md](package-manager.md) | Phases 0–2 shipped (Seedfile DSL); Phase 3 resolver not started |
| [std-networking.md](std-networking.md) | Not started; poller/blocking-pool prerequisites exist |
| [std-networking-followups.md](std-networking-followups.md) | Deferred behind std-networking v1 |
| [lsp.md](lsp.md) | Server not started; SymbolIndex prerequisite done, editor clients pre-staged |
| [mcp-server.md](mcp-server.md) | Server not started; Phase 0 language readiness mostly done (needs stdin + JSON) |
| [repl.md](repl.md) | Not started; eval_pipeline/ContextSnapshot core exists |
| [toolchain-manager.md](toolchain-manager.md) | Not started; Seedfile `compiler(ver:)` hook exists, unenforced |
| [bundled-linker.md](bundled-linker.md) | Parked |
| [future-syntax.md](future-syntax.md) | Deferred-ideas list; ranges shipped, the rest open (see Roadmap) |

## Completed

Executed or formally superseded. Remaining loose ends from these plans are tracked
on the docs Roadmap page or as GitHub issues — not in the plan files.

| Plan | Outcome |
|---|---|
| [syntax-buildout.md](completed/syntax-buildout.md) | Complete — original bootstrap plan, long surpassed |
| [protocols.md](completed/protocols.md) | Complete except Hash trait (→ Roadmap) |
| [random-and-ranges.md](completed/random-and-ranges.md) | Complete |
| [codegen.md](completed/codegen.md) | M1–M11 complete; async superseded upward by green threads |
| [close-execution-path-gaps.md](completed/close-execution-path-gaps.md) | All 9 tasks complete |
| [full-stack-parity.md](completed/full-stack-parity.md) | Complete; parity enforced by meta-tests |
| [green-threads.md](completed/green-threads.md) | Phases 1–5, 9 complete; cleanup/mutex/channel gaps → issue #51 |
| [async-runtime-straightshot.md](completed/async-runtime-straightshot.md) | Complete via staticlib runtime design |
| [close-async-gaps.md](completed/close-async-gaps.md) | Executed; remaining correctness gaps → issue #51 |
| [diagnostics.md](completed/diagnostics.md) | Phases 1–4 complete; Phase 5 TOONS → Roadmap |
| [formatter.md](completed/formatter.md) | Complete |
| [build-system.md](completed/build-system.md) | Complete; minor leftovers → Roadmap |
| [os-primitives.md](completed/os-primitives.md) | Complete |
| [entropy-benchmark.md](completed/entropy-benchmark.md) | Measurement done (70/76 token budget); no CI enforcement yet |
| [package-management.md](completed/package-management.md) | Superseded by package-manager.md |
| [runtime-jit-eval-plan.md](completed/runtime-jit-eval-plan.md) | Complete — shipped as `std/runtime` (`evaluate`, `jit_run`), `--jit`-gated |
