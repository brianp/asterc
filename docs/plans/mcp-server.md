# Plan: Aster MCP Server (aster-mcp)

## Context

The Aster MCP server is the bridge between the compiler and AI agents (Claude Code, Cursor, other MCP clients). It watches for compiler output, exposes it as MCP resources and tools, and sends notifications to connected agents when things change.

**Core principle:** The LLM is not the source of correctness. The compiler is. The LLM is the adaptive explanation layer.

**Communication format:** The MCP server uses [TOON (Token-Oriented Object Notation)](https://github.com/toon-format/spec) as its wire format for agent communication. TOON is a compact, human-readable encoding of the JSON data model optimized for LLM consumption -- it reduces token usage by 30-60% compared to JSON while remaining lossless. It is not Aster-specific. The compiler produces structured artifacts (diagnostics, AST, type info); the MCP server encodes these as TOON for consumption by AI agents.

**Primary goal:** Build the MCP server in Aster itself. This is the ultimate dogfood -- Aster's AI tooling layer, written in Aster, powering AI agents that write Aster. The Rust implementation exists as a fallback and bootstrap path.

## Architecture

```
Human writes .aster files
         |
         v
   asterc / asterd
   (compiler / daemon)
         |
         v
   .aster/last-run/
   (compiler artifacts: diagnostics, AST, types)
         |
         v
   aster-mcp
   (stdio MCP server, written in Aster)
         |
     TOON protocol
         |
         v
   Claude Code / Cursor / agents
   (MCP clients)
         |
         v
   Human sees explanations + fixes
```

Three separate processes, clean boundaries:
- **asterc** = one-shot compiler, produces structured artifacts
- **asterd** = incremental daemon (future), watches workspace
- **aster-mcp** = agent bridge, encodes compiler artifacts as TOON, exposes via MCP

## Prerequisites

### Phase 0: Aster Language Readiness

Before the MCP server can be written in Aster, the language must support the capabilities required to build a stdio server that reads files, parses JSON, and manages state. This is the prerequisite phase -- each capability listed here must be implemented through the full compiler stack (parse → typecheck → FIR → codegen).

#### 0A. Standard I/O

The MCP server reads JSON-RPC from stdin and writes responses to stdout. Aster needs:

- **`stdin_read_line() -> String`** — blocking read of one line from stdin
- **`stdout_write(s: String)`** — write string to stdout (no trailing newline)
- **`stdout_writeln(s: String)`** — write string to stdout with newline
- **`stderr_write(s: String)`** — write to stderr (for logging, separate from protocol)

These can start as runtime builtins (like `aster_print_str` today) and graduate to a `std/io` module.

#### 0B. File I/O

The server reads `.aster/last-run/` artifacts from disk:

- **`file_read(path: String) -> String`** — read entire file contents
- **`file_write(path: String, content: String)`** — write string to file
- **`file_exists(path: String) -> Bool`** — check if file exists
- **`path_join(base: String, child: String) -> String`** — join path segments

#### 0C. JSON Parsing and Serialization

The MCP protocol is JSON-RPC 2.0. This is the largest single capability needed:

- **`json_parse(s: String) -> JsonValue`** — parse JSON string into a value
- **`json_stringify(v: JsonValue) -> String`** — serialize value to JSON string
- **`JsonValue` type** — enum with variants: Null, Bool, Int, Float, Str, Array, Object
- **Object access** — `json.get("key")`, `json.set("key", value)`
- **Array access** — `json[i]`, `json.push(value)`, `json.len()`

This requires: Map type working end-to-end, recursive enum support, string manipulation.

#### 0D. String Operations

JSON handling and protocol work require string manipulation beyond what exists today:

- **`string_contains(haystack: String, needle: String) -> Bool`**
- **`string_split(s: String, delim: String) -> List[String]`**
- **`string_starts_with(s: String, prefix: String) -> Bool`**
- **`string_trim(s: String) -> String`**
- **`string_slice(s: String, start: Int, end: Int) -> String`**
- **`int_parse(s: String) -> Int`** — parse integer from string

#### 0E. Map Type (End-to-End)

Maps are needed for JSON objects, request routing, and state management:

- Map literals: `let m = {"key": value}`
- Map access: `m["key"]`, `m.get("key")`
- Map mutation: `m["key"] = value`, `m.set("key", value)`
- Map iteration: `for key in m.keys()`
- Map size: `m.len()`

This is currently a hard gap -- `Expr::Map` is parsed but not lowered to FIR.

#### 0F. Error Handling (End-to-End)

The server must handle malformed input, missing files, parse failures gracefully:

- `throw` creates a tagged error value
- `!` propagates errors up the call stack (early return)
- `.or(default)` provides fallback values
- `.catch` dispatches on error type

Currently stub/identity in FIR and codegen. Must be fully wired.

#### 0G. Process Spawning

The server invokes `asterc` as a subprocess:

- **`process_run(cmd: String, args: List[String]) -> ProcessResult`** — run a command, capture stdout/stderr/exit code
- **`ProcessResult`** — class with `stdout: String`, `stderr: String`, `exit_code: Int`

#### 0H. Event Loop / Blocking I/O

The MCP server sits in a read-dispatch-respond loop. At minimum this is a synchronous `while true` loop reading stdin. For the file watcher (Phase 5), async or polling-based file watching is needed:

- **Synchronous first:** `while true { let line = stdin_read_line(); dispatch(line) }` — this works for Phase 1-4
- **File polling (later):** `file_modified_time(path: String) -> Int` — poll-based watcher without async runtime
- **True async (future):** needed for `asterd` daemon, not blocking for MCP v1

#### Language Readiness Summary

| Capability | Depends On | Required For |
|-----------|-----------|-------------|
| Stdio I/O | Runtime builtins | Phase 1 (core server) |
| File I/O | Runtime builtins | Phase 1 (read artifacts) |
| String ops | Runtime builtins | Phase 1 (JSON, protocol) |
| Map type | FIR + codegen gap close | Phase 1 (JSON objects, routing) |
| Error handling | FIR + codegen gap close | Phase 1 (graceful failures) |
| JSON parse/serialize | Map, String ops, Enums | Phase 1 (MCP protocol) |
| Process spawning | Runtime builtin | Phase 3 (invoke asterc) |
| File polling | Runtime builtin | Phase 5 (file watcher) |
| Async I/O | Async runtime | Phase 6 (asterd) |

**Critical path:** Map type → JSON → MCP server. Everything else can be added as runtime builtins incrementally.

## Design

### Phase 1: Core MCP Server

**1A. Server skeleton (stdio transport)**

The MCP server runs as a local process, communicating via JSON-RPC 2.0 over stdio. Claude Code and Cursor both support this transport.

```
# User adds to their MCP config:
{
  "mcpServers": {
    "aster": {
      "command": "aster-mcp",
      "args": ["--workspace", "/path/to/project"]
    }
  }
}
```

Written in Aster:

```
def main()
  let workspace = args()[1]
  let state = ServerState(workspace: workspace)

  while true
    let line = stdin_read_line()
    let request = json_parse(line)
    let response = dispatch(state, request)
    stdout_writeln(json_stringify(response))
```

**1B. Workspace artifact reader**

On startup or on-demand, aster-mcp reads compiler artifacts:

```
def read_artifacts(workspace: String) -> CompilerOutput
  let base = path_join(workspace, ".aster/last-run")
  let envelope = json_parse(file_read(path_join(base, "envelope.json")))
  let diagnostics = json_parse(file_read(path_join(base, "diagnostics.json")))
  CompilerOutput(envelope: envelope, diagnostics: diagnostics)
```

**1C. On-demand compilation**

The MCP server can trigger compilation directly:

```
def compile(file: String) -> CompilerOutput
  let result = process_run("asterc", [file, "--emit", "all"])
  read_artifacts(workspace)
```

### Phase 2: MCP Resources

Resources are read-only data the agent can pull on demand.

| Resource URI | Description |
|-------------|-------------|
| `aster://diagnostics/latest` | Latest diagnostics (structured, encoded as TOON) |
| `aster://ast/latest` | Current AST with node IDs |
| `aster://ast/node/{id}` | Single AST node and its children |
| `aster://types/latest` | All resolved types and bindings |
| `aster://symbols/latest` | Symbol index (definitions + references) |
| `aster://repairs/latest` | Candidate fixes from latest compilation |
| `aster://envelope/latest` | Compilation result summary |
| `aster://source/{file}` | Source file contents |

Example interaction:

```json
// Agent asks for diagnostics
{"jsonrpc": "2.0", "id": 1, "method": "resources/read",
 "params": {"uri": "aster://diagnostics/latest"}}

// Server returns structured diagnostics encoded as TOON
{"jsonrpc": "2.0", "id": 1, "result": {
  "contents": [{
    "uri": "aster://diagnostics/latest",
    "mimeType": "application/json",
    "text": "[{\"diagnostic_id\":\"E0412\",\"severity\":\"error\",...}]"
  }]
}}
```

### Phase 3: MCP Tools

Tools are actions the agent can invoke.

| Tool | Parameters | Returns |
|------|-----------|---------|
| `aster.compile` | `{file: string}` | Compilation envelope + diagnostics |
| `aster.check` | `{file: string}` | Typecheck only, no codegen |
| `aster.explain_error` | `{diagnostic_id: string}` | Template explanation + parameters + context |
| `aster.get_node` | `{node_id: string}` | AST node, its type, its scope, source text |
| `aster.get_type_at` | `{file: string, line: int, col: int}` | Type of expression at position |
| `aster.suggest_fix` | `{diagnostic_id: string}` | Ranked candidate fixes with confidence |
| `aster.apply_fix` | `{diagnostic_id: string, fix_index: int}` | Apply fix, return new source |
| `aster.format` | `{file: string}` | Format file, return canonical source |
| `aster.format_check` | `{file: string}` | Check if file is formatted |
| `aster.get_scope` | `{node_id: string}` | All bindings visible at node |
| `aster.search_symbols` | `{query: string}` | Find symbols by name/type pattern |

**Key tool: `aster.explain_error`**

The tool returns the diagnostic object with everything the LLM needs:

```json
{
  "diagnostic_id": "E0412",
  "template": "type_mismatch_assignment",
  "template_params": {
    "expected": "Int",
    "actual": "String",
    "binding_name": "x"
  },
  "primary_node": {
    "id": "node_184",
    "source": "\"hello\" + 1",
    "span": {"line": 5, "col": 12}
  },
  "related_nodes": [...],
  "constraint_violated": "Assignable(String, Int)",
  "scope": "function main, line 3-10",
  "candidate_fixes": [
    {"description": "convert String to Int via parse()", "confidence": 0.61, "preview": "parse(\"hello\") + 1"},
    {"description": "change binding type to String", "confidence": 0.48, "preview": "let x: String = ..."}
  ]
}
```

The LLM receives this and can explain it to:
- A beginner: "You're trying to add a word to a number. You need to convert 'hello' to a number first."
- An expert: "Type mismatch on line 5: `+` requires homogeneous operands. `parse()` or change the binding type."
- A Rust dev: "Similar to Rust's type system -- no implicit coercion. Use explicit conversion."

**Same truth, different presentation.**

**Key tool: `aster.apply_fix`**

The agent can apply compiler-suggested fixes directly:

```json
// Agent calls
{"method": "tools/call", "params": {
  "name": "aster.apply_fix",
  "arguments": {"diagnostic_id": "E0412", "fix_index": 0}
}}

// Server applies the edit, re-compiles, returns result
{"result": {
  "applied": true,
  "fix_description": "convert String to Int via parse()",
  "new_source": "...",
  "recompile_result": {"success": true, "diagnostics": []}
}}
```

### Phase 4: Notifications

When the compiler runs (triggered by file save, manual build, or agent request), the MCP server sends notifications.

**Build changed:**
```json
{"jsonrpc": "2.0", "method": "notifications/resources/list_changed"}
```

The client refreshes its resource list and can pull the latest diagnostics.

**Custom notification (for richer clients):**
```json
{"jsonrpc": "2.0", "method": "aster/buildResult", "params": {
  "success": false,
  "file": "main.aster",
  "error_count": 2,
  "warning_count": 0,
  "top_diagnostic": "E0412: type mismatch at line 5"
}}
```

**Important rule:** Notifications are tiny. Don't push giant payloads. Let the client pull on demand. This keeps tokens under control.

### Phase 5: File Watcher Integration

For seamless development, aster-mcp watches `.aster` files for changes:

```
File saved (main.aster)
    |
    v
aster-mcp detects change (polling or OS events)
    |
    v
Runs: asterc main.aster --emit all
    |
    v
Reads .aster/last-run/
    |
    v
Sends notification to connected agents
    |
    v
Agent pulls diagnostics, suggests fix if errors exist
```

This creates the loop:
1. Developer saves file
2. Compiler produces artifacts
3. MCP server encodes as TOON, notifies agents
4. Agent explains errors + suggests fixes
5. Developer applies fix
6. Repeat

### Phase 6: Future -- `asterd` Incremental Daemon

Eventually replace the compile-on-save pattern with an always-running daemon:

```
asterd  (incremental compiler daemon)
    |
    | event stream
    |
aster-mcp  (subscribes to events)
    |
    | MCP protocol (TOON)
    |
Claude / Cursor
```

The daemon would:
- Watch all workspace files
- Run incremental lex/parse/typecheck on change
- Keep TypeEnv in memory
- Emit artifact updates in real-time
- Support the REPL as a sub-mode

This is the "fast path" -- no filesystem roundtrip, direct event stream from compiler to MCP server.

## Implementation

### v1: Rust Bootstrap (Fallback)

A Rust implementation exists as a bootstrap path and fallback while the Aster language matures. This can be built immediately without waiting for Phase 0.

```
aster-mcp/
  Cargo.toml
  src/
    main.rs           -- entry point, stdio transport
    server.rs         -- MCP server implementation
    state.rs          -- workspace state, artifact cache
    watcher.rs        -- file system watcher
    resources.rs      -- MCP resource handlers
    tools.rs          -- MCP tool handlers
    compiler.rs       -- subprocess invocation of asterc
```

Dependencies:
- `serde`, `serde_json` -- JSON handling
- `notify` -- file system watching
- `tokio` -- async runtime

The Rust implementation serves two purposes:
1. Unblocks MCP server development while Aster gains the needed capabilities
2. Acts as a reference implementation / test oracle for the Aster version

### v2: Built in Aster (Primary Goal)

The Aster implementation is the target. It requires all Phase 0 capabilities to be complete.

```
mcp-server/
  main.aster          -- entry point, stdio read loop
  server.aster        -- JSON-RPC dispatch, request routing
  state.aster         -- workspace state, artifact cache
  resources.aster     -- MCP resource handlers
  tools.aster         -- MCP tool handlers
  compiler.aster      -- subprocess invocation of asterc
  json.aster          -- JSON parser and serializer
  toon.aster          -- TOON encoder (Token-Oriented Object Notation)
```

No external dependencies. JSON parsing, TOON encoding, and the MCP protocol are all implemented in Aster.

**Migration path:** Build the Rust version first for each phase, then port to Aster as Phase 0 capabilities land. The Rust version remains as a test oracle -- both implementations should produce identical MCP responses for the same compiler artifacts.

## Dependency Chain

```
Phase 0: Aster Language Readiness
    |
    |-- 0E. Map type (FIR + codegen)
    |-- 0F. Error handling (FIR + codegen)
    |-- 0A. Stdio I/O (runtime builtins)
    |-- 0B. File I/O (runtime builtins)
    |-- 0D. String ops (runtime builtins)
    |     |
    |     v
    |-- 0C. JSON parse/serialize (needs Map, String, Enum)
    |     |
    |     v
    |-- 0G. Process spawning (runtime builtin)
    |
    v
Phase 1: Core MCP server (stdio, read artifacts, dispatch)
    |
Phase 2: Resources (diagnostics, AST, types)
    |
Phase 3: Tools (compile, explain, apply_fix)
    |
Phase 4: Notifications (build changed)
    |
    |-- 0H. File polling (runtime builtin)
    |
    v
Phase 5: File watcher (auto-compile on save)
    |
Phase 6: asterd daemon (event stream)  -- future, needs async
```

The Rust fallback can start at Phase 1 immediately, in parallel with Phase 0 language work.

## The Big Picture

```
"Human-written, compiler-proven, machine-explained."

human writes .aster
    |
asterc determines truth (types, constraints, violations)
    |
compiler artifacts carry truth structurally (AST, semantics, diagnostics, repairs)
    |
aster-mcp encodes truth as TOON, exposes via MCP (resources, tools, notifications)
    |
LLM explains truth conversationally (for any audience)
    |
human understands and fixes
```

The LLM never guesses. It reads compiler facts and translates them.
The compiler never explains. It produces structured truth.
The MCP server never interprets. It encodes artifacts as TOON and passes them faithfully.
TOON is the agent communication protocol. It is not Aster-specific.

Clean separation. Each layer does one thing.
