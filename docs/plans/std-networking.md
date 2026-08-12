---
status: pending
created: 2026-04-21 21:32
executed: null
---

# Implementation Plan: std/net + std/tls + std/url + std/json + std/http

Build out Aster's networking and data-interchange stdlib so the package manager (and anything else) can make real HTTPS requests, parse JSON, and handle URLs. Five new stdlib modules, one vendored C dependency (BoringSSL), one vendored data file (Mozilla CA bundle).

## Prerequisites

- `workingfiles/docs/plans/os-primitives.md` is executed — it establishes the pattern for all virtual-stdlib modules: Rust `extern "C"` runtime function + `runtime_functions!` macro + `builtin_std_submodule_exports` registration + `FirExpr::RuntimeCall` lowering.
- Green-thread scheduler is in place: `codegen/src/green/{scheduler,poller,blocking,thread}.rs`. The `Poller` trait (kqueue/epoll), `Interest::{Read, Write}`, and `BlockingPool` exist. The "yield current green thread until fd ready" primitive is available to runtime code.
- `async foo()` syntax already desugars to a green-thread spawn returning `Task[T]` (per `workingfiles/STATUS<dot>md`).
- `cc`-based link step at `src/main<dot>rs:315` — this plan keeps that as-is. The bundled-linker (`workingfiles/plans/bundled-linker<dot>md`) is explicitly out of scope.
- User decisions captured in this conversation (all 15 summarized above) are the design source of truth.
- Follow-ups file at `workingfiles/plans/std-networking-followups<dot>md` captures deferred work.

## Codebase Analysis

### Existing stdlib pattern (from `os-primitives.md`, now executed)

1. **Runtime Rust function** — `codegen/src/runtime/<module><dot>rs`. `#[unsafe(no_mangle)] pub extern "C" fn aster_<module>_<name>(...) -> ...`. Uses `aster_string_to_rust`, `aster_string_new_from_rust`, `aster_list_*`, `aster_error_set`, `aster_class_alloc` helpers (already in `codegen/src/runtime/{string,list,error,alloc}<dot>rs`).
2. **Signature** — add to `runtime_functions!` macro in `codegen/src/runtime_sigs<dot>rs` (declares the C ABI for JIT and AOT linking).
3. **Re-export** — add `pub use <module>::*;` in `codegen/src/runtime/mod<dot>rs`.
4. **Typechecker registration** — extend `builtin_std_submodule_exports` in `typecheck/src/typechecker<dot>rs` with the new module name; `builtin_function_exports` helper already populates `ModuleExports.variables` with `Type::Function` values.
5. **FIR lowering** — map known stdlib function calls to `FirExpr::RuntimeCall { name: "aster_<module>_<name>", ... }` in `fir/src/lower/expr<dot>rs` (or a dedicated `fir/src/lower/stdlib<dot>rs`).
6. **Class exposure** (for `TcpStream`, `TlsStream`, `HttpClient`, etc.) — built-in class registration alongside built-in error classes (`register_builtins` path, same mechanism `ProcessResult`/`ProcessError` use).

### Files that will change on the compiler side
- `codegen/Cargo<dot>toml` — new `build-dependencies` entry for `cmake` to drive BoringSSL build in `build.rs`.
- `codegen/build<dot>rs` — new or extended (one exists for asm shim compilation). Invokes CMake on vendored BoringSSL, copies `libssl.a`/`libcrypto.a` into `OUT_DIR`, emits an `include_bytes!`-ready Rust file. Also runs the NSS → PEM conversion script and embeds `ca-bundle.pem`.
- `codegen/src/runtime_sigs<dot>rs` — new `aster_*` entries for net/tls/url/json/http runtime calls (url/json/http have very few — most code is in Aster).
- `codegen/src/runtime/mod<dot>rs` — new `pub mod net; pub mod tls;` entries.
- `codegen/src/runtime/net<dot>rs` — new: TCP socket runtime functions.
- `codegen/src/runtime/tls<dot>rs` — new: BoringSSL-backed TLS runtime functions.
- `codegen/src/runtime/embedded<dot>rs` — new: `pub static LIBCRYPTO_A: &[u8] = include_bytes!(...)`, `pub static LIBSSL_A: &[u8] = include_bytes!(...)`, `pub static CA_BUNDLE_PEM: &[u8] = include_bytes!(...)`.
- `typecheck/src/typechecker<dot>rs` — `builtin_std_submodule_exports` extended for five new module names; built-in classes (`TcpStream`, `TcpListener`, `TlsStream`, `HttpClient`, `HttpResponse`, `Url`) registered.
- `fir/src/lower/expr<dot>rs` (or `stdlib<dot>rs`) — mapping entries for the new stdlib functions.
- `fir/src/builtins<dot>rs` — module/function name constants.
- `src/main<dot>rs` around line 315 — the link step now extracts embedded `libssl.a`/`libcrypto.a` into a cached directory (see best-practice cache design) and passes them to `cc` before `-lc`. Only done when producing an AOT binary; JIT path uses Cranelift JIT, which links against the runtime staticlib at process start.

### Files that will be newly created on the Aster side
- `std/net<dot>aster` (virtual stdlib file — lives alongside other std/* sources once the stdlib-files-in-Aster direction lands; until then, the module is synthetic inside the typechecker). TCP wrappers: the runtime functions return raw handles; a thin Aster layer over them provides `TcpStream`, `TcpListener`, `Stream` trait impls.
- `std/tls<dot>aster` — same pattern, thin Aster wrapper exposing `TlsStream` + `Stream` impl + trust-source params + the deliberately-ugly `connect_insecure_skip_verify` function.
- `std/url<dot>aster` — pure Aster: URL parsing, no runtime functions at all.
- `std/json<dot>aster` — pure Aster: parser + serializer + pretty-printer + `Value` enum + `validate(value, schema)` function. No runtime functions.
- `std/http<dot>aster` — pure Aster: `HttpClient` class, `HttpRequest`/`HttpResponse` classes, `get/post/put/delete/head/patch` free functions, connection pool, chunked transfer, redirects.
- Stdlib error class definitions (`NetError`, `ConnectError`, `ReadError`, `WriteError`, `TimeoutError`, `TlsError`, `HandshakeError`, `CertVerifyError`, `HttpError`, `JsonError`, `ParseError`, `SchemaError`, `UrlError`). Each hierarchy lives in the same `.aster` file as its module.

### Existing infrastructure we build on (do not reinvent)
- `codegen/src/green/poller<dot>rs` — `Poller::register(fd, interest, token)` already parks a green thread until fd is ready. TCP and TLS runtime functions call this directly.
- `codegen/src/green/blocking<dot>rs` — `BlockingPool::submit(task, closure)` offloads truly-blocking work to an OS thread pool. DNS (`getaddrinfo`) goes here.
- `codegen/src/runtime/error<dot>rs` — `aster_error_set()` sets the throws flag; typechecker + codegen already handle the `!` propagation.
- `codegen/src/runtime/alloc<dot>rs` — `aster_class_alloc(size)` allocates a GC'd class instance. All new classes (TcpStream, TlsStream, HttpResponse, Url, etc.) use this.
- `codegen/src/runtime/list<dot>rs`, `string<dot>rs`, `map<dot>rs` — Aster collection handles. Runtime functions read args from these and return results as these.
- `codegen/src/runtime/mutex<dot>rs`, `channel<dot>rs` — concurrency primitives for the connection pool in `std/http`.

## Research Findings

### BoringSSL integration
- Canonical pattern (from the provider research): vendor BoringSSL as a git submodule pinned to a known commit, drive its CMake build in `build.rs` via the `cmake` crate (`cfg.define("BUILD_SHARED_LIBS", "OFF")`, `cfg.define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")`, `cfg.profile("Release")`), copy the resulting `libssl.a`/`libcrypto.a` into `OUT_DIR`, then `include_bytes!` them into a Rust static.
- BoringSSL requires CMake + Go (+ Perl on some targets) **on the build host**, not on user machines. Release CI builds asterc per target triple with Go available; users download a prebuilt asterc and see none of this.
- **Symbol prefixing** is recommended (BoringSSL provides a `BORINGSSL_PREFIX` build flag on current revisions) — prevents collision if a user ever links another TLS stack into the final binary. We use prefix `ASTERC_`.
- Reference crate: `boring-sys` — we don't use it as a dep (violates "no wrapping crates" rule), but we read its build script to see what it does.
- At link time, asterc extracts `libssl.a`/`libcrypto.a` from the embedded bytes into `$XDG_CACHE_HOME/asterc/boringssl/<content-hash>/` (first time only, reused thereafter via file lock) and passes them to `cc`.

### Non-blocking TLS with green-thread scheduler
- Use `SSL_set_fd(ssl, fd)` with a non-blocking TCP fd. BoringSSL's built-in socket BIO is the right choice — custom BIOs are only necessary for non-fd transports.
- On `SSL_do_handshake` / `SSL_read` / `SSL_write` returning < 0, call `SSL_get_error`: `SSL_ERROR_WANT_READ` → `poller.register(fd, Interest::Read, token)` + yield; `SSL_ERROR_WANT_WRITE` → same with Write. After resume, retry the operation.
- Reads can return `WANT_WRITE` (during handshake/alert/KeyUpdate) and writes can return `WANT_READ`. Handle both directions in each driver.
- Do **not** attempt to yield inside a custom BIO callback — yielding from BoringSSL's call stack is fragile. The WANT_* boundary is the correct yield point.
- `SSL_ERROR_ZERO_RETURN` = clean `close_notify` = `Ok(0)`.

### CA trust store
- **Source:** Mozilla NSS `certdata.txt`. Vendor a specific NSS release tarball (pinned by git tag), not a rolling checkout.
- **Conversion:** a small script (`scripts/build-ca-bundle.py` or `.sh`) parses `certdata.txt`, extracts only certs with `CKA_TRUST_SERVER_AUTH = TRUSTED_DELEGATOR`, excludes distrusted, emits a concatenated PEM. The `certifi` project's approach is the reference.
- **Shape:** single concatenated PEM file. BoringSSL loads it via `SSL_CTX_load_verify_locations(ctx, path, NULL)` when we write it to disk at first use, OR we parse it from embedded memory via `PEM_read_bio_X509` in a loop and `X509_STORE_add_cert`. Prefer the memory path — avoids filesystem I/O on every connect.
- **System cert opt-in:** on Linux, read `/etc/ssl/certs/ca-certificates.crt` (or platform-specific path via `openssl-probe`-like logic); on macOS, call `SecTrustCopyAnchorCertificates`; on Windows, call `CertOpenSystemStore("ROOT")` + enumerate. Each adds certs to the `X509_STORE`.

### HTTP/1.1 implementation in Aster
- Pure Aster code on top of `Stream` (trait that both TcpStream and TlsStream implement).
- Parser: hand-written byte state machine. HTTP/1.1 is ABNF-simple; no parser generator required. RFC 9112 is the current spec.
- Must support: status line parsing, header folding (tolerant of old-school wrap-by-whitespace but don't emit it), `Content-Length` bodies, `Transfer-Encoding: chunked` bodies (can't be replaced by Content-Length — the spec requires chunked support to be HTTP/1.1-compliant), connection-header interpretation (`close` vs `keep-alive`).
- Keep-alive pool: a `Map[String, List[TlsStream/TcpStream]]` keyed by `<scheme>://<host>:<port>`. Concurrent access gated by a `Mutex[...]` (both primitives exist in the runtime). When a request finishes and the response's `Connection` doesn't say close, the stream goes back into the pool.
- Redirects: follow up to 10 hops; only follow 3xx with a `Location` header; rewrite the method on 301/302/303 per RFC (POST → GET on 303 always; conservative on 301/302). Controllable via `HttpClient(follow_redirects: false, ...)`.

### JSON parser design
- Hand-written recursive descent parser over a `Chars` iterator — well-trodden territory, <500 lines of Aster.
- `Value` enum: `Null | Bool(Bool) | Number(Float) | Integer(Int) | Text(String) | Array(List[Value]) | Object(Map[String, Value])`. Split numeric types at parse time: any literal without `.`/`e`/`E` parses to `Integer`; anything else parses to `Number`. Consumers decide which representation they need via pattern match.
- Errors report `line`, `column`, `expected`, `found`. Every token consumed advances the position counter.
- Schema validation: v1 supports a small but real subset of JSON Schema draft-2020-12: `type`, `properties`, `required`, `items`, `minimum`/`maximum`, `minLength`/`maxLength`, `enum`, `pattern` (regex). More complex keywords (`allOf`, `$ref`, etc.) deferred to follow-ups.

### URL parser design
- RFC 3986 ABNF. Hand-written, <200 lines of Aster.
- `parse(url: String) -> Url throws UrlError`. `Url` is a class with public fields: `scheme`, `userinfo`, `host`, `port: Int?`, `path`, `query`, `fragment`.
- Helpers: `join(base: Url, relative: String) -> Url`, `encode(s: String) -> String` (percent-encode), `decode(s: String) -> String` (percent-decode).

## Task Breakdown

Ordered by dependencies. Roughly: compiler-side infrastructure first (BoringSSL, CA bundle, runtime functions), then stdlib Aster code (built on top), then tests.

### 1. Vendor BoringSSL and the NSS CA bundle

- **Files to modify:** `codegen/build<dot>rs`, `codegen/Cargo<dot>toml` (add `cmake = "0.1"` build-dep).
- **Files to create:** `third_party/boringssl/` (git submodule at a pinned commit), `third_party/nss/certdata<dot>txt` (pinned copy from a specific NSS release), `scripts/build-ca-bundle<dot>py` (NSS → PEM converter).
- **Dependencies:** none (first task).
- **Approach:** Add BoringSSL as a git submodule. Extend `codegen/build<dot>rs` to drive its CMake build (Release, PIC, BUILD_SHARED_LIBS=OFF, `BORINGSSL_PREFIX=ASTERC_`), copy `libssl<dot>a` + `libcrypto<dot>a` into `OUT_DIR`. Run `build-ca-bundle<dot>py` on `certdata<dot>txt` to produce `ca-bundle<dot>pem`, also to `OUT_DIR`. Emit a generated Rust file containing `include_bytes!` for all three artifacts.
- **Integration points:** BoringSSL archives and CA bundle are consumed by:
  - `codegen/src/runtime/embedded<dot>rs` (new) re-exports the bytes
  - `src/main<dot>rs` link step extracts archives to cache dir + passes to `cc`
  - `codegen/src/runtime/tls<dot>rs` parses PEM from `CA_BUNDLE_PEM` into `X509_STORE` at startup
- **Key decisions:**
  - Build BoringSSL in CI, not on user machines: asterc release binaries ship the prebuilt archives already embedded.
  - Symbol prefix `ASTERC_` — avoid collisions if a user ever links another crypto stack.
  - NSS vendored as a specific release tarball, not a rolling checkout (reproducibility).
- **Potential issues:**
  - Host machines need Go + Perl + CMake to build asterc. Document in `README<dot>md`. Release CI images preinstall all three.
  - CMake build is slow (~30-60s fresh). Use `cargo:rerun-if-changed=third_party/boringssl` so it rebuilds only when the submodule pin moves.
  - Per-target matrix — Linux x86_64, Linux aarch64, macOS x86_64, macOS arm64, Windows x86_64. Each binary embeds its own per-target archives.

### 2. Link-step extraction and caching

- **Files to modify:** `src/main<dot>rs` (around line 315, `cmd_build`).
- **Files to create:** `src/link_artifacts<dot>rs` (new helper module).
- **Dependencies:** Task 1.
- **Approach:** Before invoking `cc`, compute a SHA-256 hash of the embedded BoringSSL bytes. Build cache path `$XDG_CACHE_HOME/asterc/boringssl/<hash>/`. If cache miss, acquire a file lock on `<hash>.lock`, write `libssl<dot>a`/`libcrypto<dot>a` into the dir, release lock. Append both archive paths to the `cc` command line in the right order (`libssl<dot>a` before `libcrypto<dot>a` — ssl depends on crypto).
- **Integration points:** Only triggered on AOT path; JIT uses the runtime staticlib directly (Cranelift's `JITModule` resolves symbols from the process, which already has BoringSSL linked into asterc itself).
- **Key decisions:**
  - Content-hashed cache dir — never stale, never re-extract, zero re-entry cost after first build.
  - File lock on first-time extraction to handle concurrent `asterc build` invocations.
  - Don't conditionally link BoringSSL — always pass the archives to `cc`; linker DCE strips unused symbols if the program doesn't reference TLS.
- **Implementation notes:** `fd2::flock`-style lock via `std::fs::File` + libc `flock` on Unix / `LockFileEx` on Windows. The `fs2` crate pattern (but we don't need it as a dep — direct libc call).

### 3. std/net runtime functions

- **Files to create:** `codegen/src/runtime/net<dot>rs`.
- **Files to modify:** `codegen/src/runtime/mod<dot>rs`, `codegen/src/runtime_sigs<dot>rs`, `typecheck/src/typechecker<dot>rs`, `fir/src/lower/expr<dot>rs`, `fir/src/builtins<dot>rs`.
- **Dependencies:** Task 1 (not TLS-dependent, but sequencing here for convenience).
- **Runtime functions (extern "C"):**
  - `aster_net_tcp_connect(host: *mut u8, port: i64, connect_timeout_ms: i64) -> *mut u8` — resolves host via blocking pool (getaddrinfo), connects non-blocking, waits for writable via poller, returns TcpStream handle. Sets error on timeout / refused.
  - `aster_net_tcp_read(stream: *mut u8, buf: *mut u8, len: i64, read_timeout_ms: i64) -> i64` — non-blocking read with poller yield on `EAGAIN`. Returns bytes read or -1 on error.
  - `aster_net_tcp_write(stream: *mut u8, buf: *const u8, len: i64, write_timeout_ms: i64) -> i64` — non-blocking write with poller yield.
  - `aster_net_tcp_close(stream: *mut u8)` — deregisters fd from poller, closes fd.
  - `aster_net_tcp_listen(host: *mut u8, port: i64) -> *mut u8` — binds, listens. Returns TcpListener handle.
  - `aster_net_tcp_accept(listener: *mut u8) -> *mut u8` — non-blocking accept with poller yield. Returns new TcpStream handle.
- **Stdlib surface (Aster):** `use std/net { tcp_connect, tcp_listen, TcpStream, TcpListener, NetError, ConnectError, ReadError, WriteError, TimeoutError }`.
- **Key decisions:**
  - DNS goes through `BlockingPool::submit(|| getaddrinfo(...))`. The runtime function submits the job and yields; pool wakes it with result.
  - Timeouts implemented via a timer registered with the poller alongside the fd interest. `Poller::poll(timeout)` already supports a top-level timeout; we add a per-registration deadline.
  - TcpStream handle layout: `{ fd: i64, read_timeout_ms: i64, write_timeout_ms: i64 }`. Simple GC'd struct, same shape as `ProcessResult`.
- **Implementation notes:** Use `libc::socket`, `libc::connect`, `libc::fcntl(F_SETFL, O_NONBLOCK)`, `libc::read`, `libc::write`, `libc::shutdown`, `libc::close`. Wrap each in the WANT_*-equivalent pattern (EAGAIN → register + yield). Windows uses WSA equivalents — separate `#[cfg(windows)]` impl.

### 4. std/tls runtime functions

- **Files to create:** `codegen/src/runtime/tls<dot>rs`, `codegen/src/runtime/embedded<dot>rs`.
- **Files to modify:** `codegen/src/runtime/mod<dot>rs`, `codegen/src/runtime_sigs<dot>rs`, `typecheck/src/typechecker<dot>rs`, `fir/src/lower/expr<dot>rs`, `fir/src/builtins<dot>rs`.
- **Dependencies:** Tasks 1, 3.
- **Runtime functions:**
  - `aster_tls_init()` — one-time init: build a global `SSL_CTX`, set `TLS_method`, set minimum version to TLS 1.2, load vendored CA bundle by parsing `CA_BUNDLE_PEM` into `SSL_CTX`'s `X509_STORE` via `PEM_read_bio_X509` loop.
  - `aster_tls_connect(host: *mut u8, port: i64, use_system_certs: i8, trust_path: *mut u8, connect_timeout_ms: i64) -> *mut u8` — creates TCP stream via `aster_net_tcp_connect` internally, wraps via `SSL_new` + `SSL_set_fd` + `SSL_set_tlsext_host_name` (SNI), drives `SSL_do_handshake` with WANT_* → register + yield loop. Returns TlsStream handle.
  - `aster_tls_connect_insecure_skip_verify(host, port, connect_timeout_ms) -> *mut u8` — same as connect but `SSL_CTX_set_verify(SSL_VERIFY_NONE)` on a fresh SSL_CTX. Deliberately separate function with long name.
  - `aster_tls_read(stream, buf, len, timeout_ms) -> i64` — `SSL_read` driver per research sketch.
  - `aster_tls_write(stream, buf, len, timeout_ms) -> i64` — `SSL_write` driver, handles partial writes.
  - `aster_tls_close(stream)` — `SSL_shutdown` (one-directional, don't wait for peer close_notify — that's a common hang bug), `SSL_free`, close underlying fd.
  - `aster_tls_peer_cert_subject(stream) -> *mut u8` — returns subject DN as String.
  - `aster_tls_peer_cert_issuer(stream) -> *mut u8` — returns issuer DN as String.
- **Stdlib surface:** `use std/tls { tls_connect, tls_connect_insecure_skip_verify, TlsStream, TlsError, HandshakeError, CertVerifyError }`. `tls_connect` takes optional `use_system_certs: Bool = false` and `trust_path: String = ""` params.
- **Key decisions:**
  - One global `SSL_CTX` seeded with the vendored bundle, used for all default connections. Connections with `trust_path` or `use_system_certs` build their own `SSL_CTX` (cheaper to rebuild than to mutate).
  - SNI always set from the hostname passed to `tls_connect` — never a user knob.
  - TLS 1.2 minimum (`SSL_CTX_set_min_proto_version(TLS1_2_VERSION)`), disabled SSLv3/TLS1.0/TLS1.1.
  - Hostname verification via `SSL_set_verify` + `X509_VERIFY_PARAM_set1_host(param, hostname, 0)` — never optional on the normal path.
- **Potential issues:**
  - `SSL_CTX` thread safety: BoringSSL post-1.1 is thread-safe for read-only operations (our case). Use `OnceLock<*mut SSL_CTX>` (as raw pointer — wrap in a Send-Sized-pointer newtype).
  - Memory BIO parsing of `CA_BUNDLE_PEM`: `BIO_new_mem_buf`, loop `PEM_read_bio_X509` until NULL, `X509_STORE_add_cert` for each, `X509_free`. Straightforward but error-prone in FFI — write once, test with known-good bundle.
- **Implementation notes:** Raw FFI declarations for the BoringSSL functions we call — ~25 functions. Declare them directly in `tls<dot>rs` with `extern "C"` blocks (matching BoringSSL's C signatures). No `bindgen`, no crate. Symbol names include the `ASTERC_` prefix.

### 5. Error class hierarchy (built-in registration)

- **Files to modify:** `typecheck/src/typechecker<dot>rs` (built-in class registration path that currently registers `ProcessError`, `ProcessResult`, etc.), `codegen/src/runtime/alloc<dot>rs` (if new class-size helpers needed).
- **Dependencies:** Tasks 3, 4.
- **Approach:** Register each error class as a built-in extending `Error`. Fields: `message: String` (inherited), plus category-specific fields (`ConnectError.host: String`, `ConnectError.port: Int`; `TimeoutError.timeout_ms: Int`; `CertVerifyError.reason: String`; etc.). Runtime allocates via `aster_class_alloc` and initializes fields before `aster_error_set()`.
- **Classes registered:** `NetError`, `ConnectError extends NetError`, `ReadError extends NetError`, `WriteError extends NetError`, `TimeoutError extends NetError`. `TlsError`, `HandshakeError extends TlsError`, `CertVerifyError extends TlsError`. (`HttpError`, `UrlError`, `JsonError`, `ParseError`, `SchemaError` declared in Aster .aster source since they don't need special runtime construction.)
- **Integration points:** When a runtime function calls `aster_error_set()`, it first allocates the appropriate subclass instance and stores it in the thread-local current-error slot (existing mechanism, see `codegen/src/runtime/error<dot>rs`). The caller's `!` propagation picks up whatever subclass was stored.

### 6. std/net Aster wrapper

- **Files to create:** `std/net<dot>aster` (virtual stdlib source).
- **Dependencies:** Tasks 3, 5.
- **Approach:** Thin Aster layer. `tcp_connect(host: String, port: Int, connect_timeout: Int = 30, read_timeout: Int = 30, write_timeout: Int = 30) -> TcpStream throws NetError` — calls `aster_net_tcp_connect`, constructs `TcpStream` class. Same for `tcp_listen`. `TcpStream` class has instance methods `read`, `write`, `close` that delegate to the corresponding runtime functions.
- **Trait:** declare `Stream` trait (in `std/io<dot>aster` or alongside — decide during implementation) with `def read(buf: Bytes, len: Int) -> Int throws NetError`, `def write(data: Bytes) -> Int throws NetError`, `def close() -> Void throws NetError`. `TcpStream includes Stream`.
- **Key decisions:**
  - Timeout values as named params with defaults on construction function. No builder (per decision 14 correction).
  - `Stream` trait is the shared interface consumed by `std/http`.

### 7. std/tls Aster wrapper

- **Files to create:** `std/tls<dot>aster`.
- **Dependencies:** Tasks 4, 5, 6.
- **Approach:** `tls_connect(host: String, port: Int, connect_timeout: Int = 30, read_timeout: Int = 30, write_timeout: Int = 30, use_system_certs: Bool = false, trust_path: String = "") -> TlsStream throws TlsError`. `TlsStream includes Stream`. Separate `tls_connect_insecure_skip_verify(host, port, ...)` for the escape hatch. `peer_cert_subject` / `peer_cert_issuer` as instance methods on `TlsStream`.

### 8. std/url

- **Files to create:** `std/url<dot>aster`.
- **Dependencies:** none (pure Aster).
- **Approach:** `parse(url: String) -> Url throws UrlError`, `join(base: Url, relative: String) -> Url throws UrlError`, `encode(s: String) -> String`, `decode(s: String) -> String throws UrlError`. `Url` class with public `scheme`, `userinfo`, `host`, `port: Int?`, `path`, `query`, `fragment` fields plus `to_string() -> String` method. Hand-written byte-level RFC 3986 parser.
- **Key decisions:**
  - `Url.port` is `Int?` (nullable) — absent means "use scheme default" (80 for http, 443 for https).
  - Percent-decoding surfaces `UrlError` on invalid `%XX` sequences rather than silently producing garbage bytes.

### 9. std/json

- **Files to create:** `std/json<dot>aster`.
- **Dependencies:** none (pure Aster).
- **Approach:** Recursive descent parser over a byte-position cursor. Free functions: `parse(text: String) -> Value throws JsonError`, `serialize(value: Value) -> String`, `pretty_print(value: Value, indent: Int = 2) -> String`, `validate(value: Value, schema: Value) -> Void throws SchemaError`. `Value` enum with variants `Null`, `Bool(Bool)`, `Integer(Int)`, `Number(Float)`, `Text(String)`, `Array(List[Value])`, `Object(Map[String, Value])`.
- **Key decisions:**
  - Numeric bifurcation at parse time: literals without fractional/exponent parts → `Integer`. Consumers pattern-match.
  - Error type: `JsonError extends Error` with `line: Int`, `column: Int`, `expected: String`, `found: String` fields.
  - Schema subset: `type`, `properties`, `required`, `items`, `minimum`/`maximum`, `minLength`/`maxLength`, `enum`, `pattern`. `$ref`/`allOf`/`anyOf`/`oneOf` deferred.
- **Potential issues:**
  - Regex support for `pattern` keyword — Aster may not have a stdlib regex yet. If not, implement a minimal subset (anchors, char classes, repetition) or defer `pattern` to follow-ups. Verify during implementation; add to followups file if deferred.

### 10. std/http (the big one)

- **Files to create:** `std/http<dot>aster`.
- **Dependencies:** Tasks 6, 7, 8.
- **Approach:** Pure Aster on top of `Stream`. Public surface: free functions `get(url: String) -> HttpResponse`, `post(url: String, body: Bytes, headers: Map[String, String] = {}) -> HttpResponse`, plus `put`, `delete`, `head`, `patch`. For shared configuration, `HttpClient` class with constructor params (timeouts, follow_redirects, max_redirects, trust_path, etc.) and instance methods matching the free functions. The free functions internally use a default `HttpClient` constructed with module defaults.
- **Classes:** `HttpClient`, `HttpRequest` (method, url, headers, body), `HttpResponse` (status, headers, body, url — post-redirect).
- **Error classes:** `HttpError` (base), subclasses `InvalidResponseError`, `RedirectError`, `StatusError` (optional — only if user opts into "throw on non-2xx" mode; default is return 4xx/5xx as normal responses).
- **Internal architecture:**
  - URL → scheme dispatch: parse URL via `std/url`, pick `tcp_connect` for `http://` and `tls_connect` for `https://`, get a `Stream`.
  - Connection pool: `Map[String, List[Stream]]` keyed by `<scheme>://<host>:<port>`. Wrapped in a `Mutex`. On request, `pool.pop()` or `connect()`. On response complete with keep-alive, `pool.push()`.
  - Request serialization: write status line + headers + `\r\n\r\n` + body to the `Stream`. Handle `Transfer-Encoding: chunked` if body size is unknown (rare for clients, common for streaming — v1 always knows size, so uses `Content-Length`).
  - Response parsing: read status line, read headers until `\r\n\r\n`, decode body per `Content-Length` or `Transfer-Encoding: chunked`. Return `HttpResponse`.
  - Redirects: if 3xx + `Location` header, construct new URL (possibly relative — use `url.join`), recurse up to `max_redirects`. Method rewrite per RFC on 303 (always GET) and conservatively on 301/302.
- **Key decisions:**
  - Buffered bodies, not streaming (v1). Body is `Bytes` (List[Byte] or a dedicated type — follow Aster convention).
  - No cookies, no multipart, no gzip in v1 (deferred — already in followups file).
  - Default timeouts inherited from `std/net` defaults unless overridden on `HttpClient`.
- **Implementation notes:**
  - Chunked transfer decoder is the trickiest piece. State machine: read chunk-size line (hex), read that many bytes, consume `\r\n`, repeat until chunk-size 0, read trailer headers, done.
  - Header case-insensitivity: normalize to lowercase on parse, compare lowercase.

### 11. FIR lowering for new stdlib calls

- **Files to modify:** `fir/src/lower/expr<dot>rs` (or `fir/src/lower/stdlib<dot>rs` if extracted), `fir/src/builtins<dot>rs`.
- **Dependencies:** Tasks 3, 4 (runtime function names must exist).
- **Approach:** Extend the existing known-stdlib-function mapping from `os-primitives` work. Each imported function from `std/net`, `std/tls` (only the ones that correspond to runtime functions, not the pure-Aster helpers) is lowered to the matching `FirExpr::RuntimeCall`. `std/url`, `std/json`, `std/http` have no runtime-function calls — their code is pure Aster and lowers like any user-written Aster.
- **Integration points:** `lower_call` in `fir/src/lower/expr<dot>rs` already branches on stdlib function names; add new entries.

### 12. Tests

- **Files to create:**
  - `tests/integration/std_net<dot>rs` — TCP unit + integration (loopback server in Rust, connect+read+write from Aster)
  - `tests/integration/std_tls<dot>rs` — TLS handshake against an in-test loopback TLS server (use `rustls` or a self-signed cert served by a minimal Rust server). These are compiler-internal tests; we can use `rustls` in tests without violating the "no crate wrapping" rule — it's test-only, not shipped.
  - `tests/integration/std_url<dot>rs` — URL parsing/joining/encoding edge cases from RFC 3986 examples.
  - `tests/integration/std_json<dot>rs` — JSON parser (all valid constructs, malformed inputs), serializer (round-trip), pretty printer, schema validator.
  - `tests/integration/std_http<dot>rs` — HTTP client against in-test Rust HTTP server (chunked, keep-alive, redirects, error codes, headers).
- **Files to modify:** `tests/integration/main<dot>rs` (register modules).
- **Approach:** Per decision 15 — both strategies. Mock/unit for parsers (`std/url`, `std/json`, `std/http` parser pieces). Real-server integration for `std/net` and `std/tls` (using ephemeral ports, 127.0.0.1-bound Rust servers spun up in test fixtures).

### 13. Documentation

- **Files to create:** `workingfiles/docs/src/content/docs/stdlib/net<dot>mdx`, `tls<dot>mdx`, `url<dot>mdx`, `json<dot>mdx`, `http<dot>mdx`.
- **Approach:** Mirror the existing docs pattern. Short overview, function signatures, examples, error class list per module.

## Potential Challenges & Mitigations

1. **Challenge:** BoringSSL build fails on a target (missing Go/Perl on a CI runner, CMake version too old, etc.).
   **Mitigation:** Document prerequisites in `README<dot>md`. CI matrix explicitly installs Go ≥ 1.18, Perl ≥ 5.20, CMake ≥ 3.15. Build.rs prints a clear error when any tool is missing. Developers building asterc locally hit the same prereq check.

2. **Challenge:** Symbol collisions when a user's final binary links another TLS stack (e.g. a future Aster user writes C-FFI to OpenSSL).
   **Mitigation:** Build BoringSSL with `BORINGSSL_PREFIX=ASTERC_`. All symbols live under that namespace. Even if the user links OpenSSL, no collision.

3. **Challenge:** TLS handshake hangs when peer never replies (dead-server scenario).
   **Mitigation:** Connect and handshake respect `connect_timeout`. Poller `Poller::poll(timeout)` returns on deadline, TLS driver reports `TimeoutError`, green thread unwinds cleanly.

4. **Challenge:** `SSL_shutdown` can block waiting for peer `close_notify` — classic cause of hangs on connection tear-down.
   **Mitigation:** Call `SSL_shutdown` once (one-directional), don't re-call for the peer response. Then close fd. We tolerate half-closes; dropping ciphertext in flight is acceptable at close.

5. **Challenge:** BlockingPool saturation under high DNS load — if thousands of green threads all block on `getaddrinfo` concurrently, the pool fills up.
   **Mitigation:** Current blocking pool is unbounded (spawns threads as needed). Acceptable for v1. Pool tuning / DNS cache is a follow-up if it bites. Note this in followups file.

6. **Challenge:** HTTP/1.1 parser ambiguity around `Content-Length` vs `Transfer-Encoding: chunked` (RFC specifies chunked wins but some broken servers send both).
   **Mitigation:** Spec-compliant behavior: if both present, prefer chunked. If chunked is present and we can't parse it, return `InvalidResponseError`. Tolerate-but-warn path deferred to followups (not needed for fetching from sane registries).

7. **Challenge:** JSON schema `pattern` keyword needs regex; Aster may not have a regex stdlib.
   **Mitigation:** Check during implementation. If no regex exists, either implement a minimal regex engine in Aster (anchors + char classes + `*`/`+`/`?` — enough for most schemas) or defer `pattern` to followups. Prefer the latter — regex is a whole separate project.

8. **Challenge:** First-time link-step extraction of BoringSSL archives is slow (~50 MB writes across two files).
   **Mitigation:** Cache by content hash, file-locked first-time extraction, reuse for all subsequent builds. First `asterc build` after install is slow; every subsequent build pays zero cost.

9. **Challenge:** System cert opt-in (`use_system_certs: true`) means three different platform code paths.
   **Mitigation:** Build per-platform Rust modules: `tls_certs_linux<dot>rs`, `tls_certs_macos<dot>rs`, `tls_certs_windows<dot>rs`. Each exposes `fn load_system_certs(store: *mut X509_STORE)`. `cfg` gates pick the right one.

10. **Challenge:** `std/http` connection pool under concurrent access (multiple green threads making requests at once).
    **Mitigation:** Pool is a `Mutex[Map[...]]`. Mutex lock is held only for the pop/push — actual I/O happens on the extracted stream without holding the lock.

## File Description Updates

After implementation, these files gain meaningful descriptions:
- `codegen/src/runtime/net<dot>rs`
- `codegen/src/runtime/tls<dot>rs`
- `codegen/src/runtime/embedded<dot>rs`
- `codegen/build<dot>rs` (updated — now builds BoringSSL and CA bundle)
- `src/link_artifacts<dot>rs`
- `std/net<dot>aster`, `std/tls<dot>aster`, `std/url<dot>aster`, `std/json<dot>aster`, `std/http<dot>aster`
- `tests/integration/std_{net,tls,url,json,http}<dot>rs`
- `scripts/build-ca-bundle<dot>py`

## Codebase Overview Updates

- "Core Components" / "Rust Compiler Framework" section gets: `codegen/src/runtime/{net,tls,embedded}<dot>rs`, the CA bundle + BoringSSL vendoring, the link-time extraction.
- "Technology Stack" adds: BoringSSL (vendored, statically embedded), Mozilla NSS CA bundle (vendored).
- "Entry Points" unchanged.
- "Data Flow" gets a note about HTTPS requests flowing Aster → std/http → std/url (parse) → std/net or std/tls → runtime FFI → BoringSSL + poller + green-thread scheduler.

## Unwired Code Audit

- [ ] BoringSSL archive bytes (producer: `build<dot>rs` writes `embedded<dot>rs`) consumed by both: (a) `codegen/src/runtime/tls<dot>rs` for initializing `SSL_CTX` at runtime-start; (b) `src/main<dot>rs` link step for passing `libssl<dot>a`/`libcrypto<dot>a` to `cc`.
- [ ] CA bundle bytes (producer: `build<dot>rs` embedding) consumed by `aster_tls_init()` which parses PEM into `X509_STORE`. Every `aster_tls_connect` call uses the global `SSL_CTX` that references this store.
- [ ] Each `aster_net_*` / `aster_tls_*` runtime function (producer: new `runtime_functions!` entries) is consumed by FIR lowering (task 11) which emits `FirExpr::RuntimeCall` with the matching name.
- [ ] Each Aster-side public function in `std/{net,tls}<dot>aster` (consumer of runtime functions) must correspond to a registered runtime function (producer). Import-only helpers don't need runtime-function backing; they lower like user Aster code.
- [ ] `Stream` trait (producer: declared in `std/io<dot>aster` or similar) is consumed by `TcpStream`, `TlsStream` (which `include` it) and by `std/http` code that takes `Stream` as parameter type.
- [ ] Connection pool (producer: `HttpClient` constructor creates empty `Mutex[Map[...]]`) consumed by every request path (pops/pushes) and by `HttpClient.close()` (must close all pooled streams — TODO confirm this method exists or the pool leaks on client drop).
- [ ] Redirect follow counter (producer: `HttpClient(max_redirects: N)` field read on every request) consumed by the request loop to terminate.
- [ ] Error-throwing runtime functions (producer: call `aster_error_set()`) consumed by `!` propagation in Aster calling code. Ensure every runtime function that can fail calls `aster_error_set` with the right subclass instance.
- [ ] `tls_connect_insecure_skip_verify` (producer: registered as separate runtime function + stdlib export) consumed by user code that explicitly imports it. It is NOT aliased or hidden behind `tls_connect` with a flag — separation is the safety mechanism.

## Validation Steps

- [ ] `asterc build` produces a binary against a test Aster program that does `use std/net { tcp_connect }; let s = tcp_connect(host: "127.0.0.1", port: 8080)!; s.write(data: "hello"); s.read(buf: ..., len: 5)`
- [ ] `asterc run` of the above works too (JIT path)
- [ ] TLS: `use std/tls { tls_connect }; let s = tls_connect(host: "example.com", port: 443)!; s.write(...); s.read(...)` completes a real HTTPS handshake against a public server (in release testing, not CI, to avoid flakiness).
- [ ] TLS against a known-bad cert fails with `CertVerifyError`, not a panic or generic failure.
- [ ] `tls_connect_insecure_skip_verify` succeeds against a self-signed local server.
- [ ] `use std/url { parse }; let u = parse(url: "https://user:pass@host.com:8080/path?q=1#frag")!` produces correct field values.
- [ ] `use std/json { parse, serialize, validate }` — round-trips a complex JSON document; schema validation catches a missing required field.
- [ ] `use std/http { get }; let r = get(url: "https://example.com")!` returns a 200 response with body containing expected marker.
- [ ] `HttpClient(follow_redirects: false)` does not follow a 301.
- [ ] `HttpClient(follow_redirects: true)` follows up to `max_redirects` and then errors with `RedirectError`.
- [ ] Chunked transfer encoding parses correctly against a test server that sends chunks deliberately.
- [ ] Keep-alive pool reuses a connection: two sequential `get()` calls against the same host use one TCP+TLS handshake (verified via test server hit count).
- [ ] Concurrent requests: 100 `async http.get(...)` calls complete without exhausting file descriptors or deadlocking the blocking pool.
- [ ] Timeout: `tcp_connect(host: "10.255.255.1", port: 80, connect_timeout: 1)` returns `TimeoutError` within ~1s (unroutable IP, black-holes SYN).
- [ ] Linker DCE: a `hello world` `asterc build` output binary does not contain BoringSSL symbols (verified with `nm` / `objdump`).
- [ ] An `asterc build` output binary that uses `std/tls` contains `ASTERC_SSL_CTX_new` (prefixed) and does NOT contain unprefixed `SSL_CTX_new`.
- [ ] All new module imports fail gracefully when an unknown function is requested (e.g. `use std/net { nonexistent }` → typechecker error).
- [ ] Test suite passes: `cargo test` green across all new integration files + existing suite.
