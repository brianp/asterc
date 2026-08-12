# Plan: Bundled Linker (eliminate system C toolchain requirement)

## Status

Parked. Placeholder for a future initiative. Not scoped for immediate work.

## Context

Aster's dev environment goal is "small and un-scary" — users shouldn't need to manage C compilers, linkers, or system libraries to use Aster. Today, asterc shells out to the system C compiler driver (`cc`) at `src/main.rs:315` to invoke the system linker; Cranelift handles codegen but linking is delegated. Users must therefore have gcc/clang installed.

This plan is about closing that gap: bundle a linker (and as much of the libc/SDK story as is feasible per-platform) so that asterc is a one-download, self-contained toolchain for end users.

Zig is the canonical reference — `zig cc` and `zig build-exe` bundle LLD + Clang + platform libc stubs to act as a full drop-in toolchain, including for cross-compilation. Go takes a different route: its linker is written in Go and is part of the standard distribution. Either model eliminates the system-toolchain dependency.

## Why this is its own project (not coupled to std/tls or any feature)

Bundled-linker work benefits every Aster program equally — it's infrastructure for the compiler's distribution shape, not any particular stdlib module. Coupling it to a specific feature (like std/tls) would either slow that feature down or rush the toolchain work. Ship std/tls on the existing `cc`-based link step first; tackle this separately.

## Scope (three separable pieces)

### 1. Bundle a linker

Options:

- **Ship `ld.lld` binary alongside asterc** (or embed via `include_bytes!` and write to tempdir at invocation time, matching how libboring.a will be handled). ~15-20 MB per platform. Invoke it directly instead of shelling out to `cc`.
- **Use LLD as a library** via `lld` / `llvm-sys` Rust crates. Links into asterc's binary directly. More integrated, but adds LLVM as a build-time dep for asterc itself (heavy).
- **Write our own linker.** Not happening at this stage.

Likely pick: embed `ld.lld` binary, same packaging model as libboring.a (once that lands). The Zig approach.

### 2. Bundle libc (or platform equivalent)

Every linked executable needs libc for syscalls. This is the harder half and is platform-specific:

- **Linux:** static-link against **musl libc**. Fully bundleable, no system dependency, works on any Linux kernel new enough. glibc cannot be easily static-linked (version-specific symbols, licensing). musl is the standard answer — Zig ships it.
- **macOS:** cannot static-link libSystem. Apple requires dynamic linking against the system libSystem.dylib, and linking against any Mac library requires the Xcode SDK. **Users will still need Xcode Command Line Tools installed on macOS** — this is Apple's constraint, not something we can engineer around. (Zig has the same limitation. It's common enough that macOS devs expect it.)
- **Windows:** bundle **mingw libc** (MinGW-w64 CRT) for fully self-contained binaries, or require the MSVC runtime. Zig bundles mingw.

### 3. Bundle C headers (for FFI / `extern "C"`)

Only relevant if/when Aster supports FFI to arbitrary C libraries. Zig ships glibc headers for cross-compilation support. Not on the near-term roadmap, but worth noting as the third shoe to drop.

## Open questions

- Which platforms are first-class vs best-effort? (Linux is easiest, macOS is hobbled by Apple, Windows needs mingw bundling work — prioritize based on user base.)
- Distribution size budget: bundling lld adds ~15-20 MB per platform. Bundling musl adds another few MB. Bundling mingw for Windows adds significant size. What's the acceptable ceiling for the asterc distribution?
- Do we want cross-compilation support (build Linux binaries from macOS, etc.)? Zig makes a big deal of this; it's a nice feature but adds scope (need all target libc/headers bundled).
- How does this interact with `codegen/src/asm_source.rs` (the asm shims currently compiled via the `cc` crate at asterc build time — that's a `cc` requirement for **building asterc itself**, which is a separate problem from the user-facing `cc` requirement at runtime).
- Conditional stdlib linking (linker DCE) already works with the current `cc` setup; confirm lld behaves the same (it should — `--gc-sections` / `-dead_strip`).

## Notes carried over from discussion (2026-04-16)

- Current link step: asterc invokes `cc` via `Command::new(&cc)` in `src/main.rs:315`. `cc` is the bottleneck for "no system deps." Replacing it with bundled lld is the first concrete change.
- Cranelift generates the object files (via `cranelift-object` crate, `codegen/src/aot.rs`). Link step is separate; this plan only touches the link side.
- Runtime staticlib (`libcodegen.a`) is already a separate artifact from asterc. libboring.a will be added alongside it (or embedded in asterc — decision pending on that plan). Those don't require a bundled linker per se, but the packaging stories are adjacent.
- This plan's success criterion: on Linux and Windows, a user downloads `asterc`, runs `asterc build hello.aster`, gets a working native binary — without installing gcc, clang, ld, lld, or any system toolchain. On macOS, same experience with the caveat that Xcode Command Line Tools must be present (Apple's requirement; document clearly in install docs).

## Not in scope

- Replacing `cc` for the **asterc build** itself (asterc's own compilation uses the `cc` crate as a build-dep for asm shims). That's a separate problem — only affects asterc developers, not users.
- Cross-compilation as a marketed feature. Nice side-effect if it falls out, but not a design goal for v1.
- Bundling a full C compiler (clang). Unless/until Aster supports C FFI, there's no need. Zig bundles clang because `zig cc` is a selling point; Aster doesn't need to be a C compiler.
