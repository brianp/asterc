---
status: pending
created: 2026-03-29
executed: null
---

# Implementation Plan: Aster Toolchain Manager

Manage multiple `asterc` compiler versions. Install, switch between, pin, and remove versions. Integrated into the `aster` CLI and the Seedfile DSL so projects can declare which compiler they need.

## Design Decisions

- **Shim-based switching**: The `aster` binary is always the shim. It reads the active toolchain, then delegates to the right `asterc` binary. No symlink juggling, no PATH manipulation. Similar to rustup's proxy approach.
- **Prebuilt binaries**: Toolchains are downloaded as prebuilt tarballs from a release URL. No compiling from source (unless explicitly requested later).
- **One global location**: `~/.aster/toolchains/` stores all installed versions. A single `~/.aster/default` file names the default toolchain. Per-project overrides come from the Seedfile.
- **Seedfile integration**: `compiler(version: "0.2.0")` in a Seedfile pins the project's compiler version. If the required toolchain isn't installed, `aster` offers to install it.
- **Version identifiers**: Semver strings (`0.1.0`, `0.2.0`). No channels (nightly/stable/beta) yet. That's a future concern when there's a release cadence. The special identifier `local` points to a user-specified path (for compiler developers).
- **Offline-friendly**: Once a toolchain is installed, it works without a network. Version discovery requires a network check, but `aster` caches the available-versions list.

## Directory Layout

```
~/.aster/
    default                         # plain text: "0.2.0" or "local"
    toolchains/
        0.1.0/
            bin/
                asterc              # the compiler binary
            lib/
                libaster_runtime.a  # AOT runtime staticlib
        0.2.0/
            bin/
                asterc
            lib/
                libaster_runtime.a
        local -> /path/to/dev/build # symlink for compiler developers
    cache/
        versions.json               # cached list of available versions
        downloads/                  # downloaded tarballs before extraction
```

The `aster` shim itself lives outside this tree (installed via a bootstrap script or package manager like brew/apt). It's the one binary that doesn't get version-managed, it manages everything else.

## Seedfile Integration

### The `compiler` function

Added to the `Seedfile` class alongside `package`, `dev_dependency`, and `script`:

```
class Seedfile includes DynamicReceiver
    name: String = ""
    version_str: String = ""
    compiler_version: String = ""
    deps: List[Dependency]
    dev_deps: List[Dependency]
    scripts: Map[String, String]

    def compiler(version: String) -> Void
        self.compiler_version = version

    # ... existing methods ...
```

### Usage in a Seedfile

```
package(name: "my-app", version: "0.1.0")
compiler(version: "0.2.0")

http(version: "1.2.0")
json(version: "0.4.0")
```

### Enforcement

When `aster build` (or any compile-triggering command) evaluates the Seedfile:

1. Read `compiler_version` from the evaluated Seedfile object
2. If empty, use the global default toolchain
3. If set, check if that version is installed in `~/.aster/toolchains/`
4. If installed, delegate to that version's `asterc`
5. If not installed, install it automatically. Print what's happening:
   ```
   asterc 0.2.0 not installed, downloading...
   Downloaded asterc 0.2.0 (14.2 MB)
   Installed to ~/.aster/toolchains/0.2.0/
   ```
   Then continue with the build using the newly installed toolchain.
6. If the version doesn't exist at all (not in the available versions list), error with a helpful message listing available versions

The right behavior is to get out of the user's way. If the Seedfile says they need 0.2.0, they need 0.2.0. Don't make them stop what they're doing to run a separate command. Just get it.

### Version constraint syntax

Start simple: exact version only. `compiler(version: "0.2.0")` means exactly 0.2.0.

Compatible ranges (`compiler(version: "~> 0.2")`) are a later addition. Exact pinning is the right default for compiler versions because compiler behavior is the one thing you want fully reproducible.

## CLI Subcommands

All under `aster toolchain`:

| Command | Description |
|---------|-------------|
| `aster toolchain list` | Show installed toolchains, mark the default and active |
| `aster toolchain install <version>` | Download and install a specific version |
| `aster toolchain remove <version>` | Delete an installed toolchain |
| `aster toolchain use <version>` | Set the default toolchain |
| `aster toolchain which` | Print the path to the active `asterc` binary |
| `aster toolchain available` | Fetch and display all available versions from the release server |
| `aster toolchain link <name> <path>` | Create a custom toolchain pointing to a local build |

### `aster toolchain list`

```
installed toolchains:

  0.1.0
* 0.2.0 (default)
  local -> /Users/tari/Projects/asterc/target/release

active toolchain (from Seedfile): 0.2.0
```

### `aster toolchain install <version>`

1. Check `~/.aster/cache/versions.json` (refresh if stale or missing)
2. Find the download URL for the requested version and current platform (`aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, etc.)
3. Download the tarball to `~/.aster/cache/downloads/`
4. Verify SHA-256 checksum against the manifest
5. Extract to `~/.aster/toolchains/<version>/`
6. Print the installed path and suggest `aster toolchain use <version>` if not the default

### `aster toolchain link <name> <path>`

For compiler developers who build `asterc` locally:

```
aster toolchain link local /Users/tari/Projects/asterc/target/release
```

Creates a symlink at `~/.aster/toolchains/local` pointing to the given path. The path must contain `bin/asterc` (or the binary directly, in which case the tool creates the expected structure).

## Version Discovery

### Release manifest

A JSON file hosted at a known URL (e.g., `https://releases.aster.dev/versions.json`):

```json
{
  "latest": "0.2.0",
  "versions": [
    {
      "version": "0.2.0",
      "date": "2026-04-15",
      "platforms": {
        "aarch64-apple-darwin": {
          "url": "https://releases.aster.dev/0.2.0/asterc-0.2.0-aarch64-apple-darwin.tar.gz",
          "sha256": "abc123..."
        },
        "x86_64-unknown-linux-gnu": {
          "url": "https://releases.aster.dev/0.2.0/asterc-0.2.0-x86_64-unknown-linux-gnu.tar.gz",
          "sha256": "def456..."
        }
      }
    },
    {
      "version": "0.1.0",
      "date": "2026-03-01",
      "platforms": { ... }
    }
  ]
}
```

### Caching

`versions.json` is cached at `~/.aster/cache/versions.json` with a TTL of 24 hours. `aster toolchain available` always fetches fresh. All other commands use the cache and only fetch if it's missing or expired.

### No HTTP client in Aster yet

The toolchain manager shells out to `curl` or `wget` for downloads (via `std/process { run }`). This is fine. Every system that can run a compiler has curl or wget. A native HTTP client in Aster is a future project.

Platform detection: `uname -m` and `uname -s` via process spawning give us the architecture and OS. Map these to the platform keys in the manifest.

## Shim Dispatch Logic

When `aster` is invoked, before doing anything else:

```
1. Is there a Seedfile in the current directory (or parent directories)?
   YES -> evaluate it, read compiler_version
         Is compiler_version set?
           YES -> use that toolchain
           NO  -> use global default
   NO  -> use global default

2. Resolve the toolchain to a path:
   - Read ~/.aster/default (or "0.1.0" if missing)
   - Look up ~/.aster/toolchains/<version>/bin/asterc
   - If not found, error: "toolchain <version> not installed"

3. For compile commands (build, run, test):
   - Exec the resolved asterc with the appropriate arguments

4. For toolchain commands (toolchain install/list/use/etc):
   - Handle directly in the aster CLI, don't delegate to asterc
```

The `aster` binary itself is always the latest version. Toolchain management commands always use the latest `aster` code. Only compilation is delegated to the pinned `asterc`.

## Tarball Structure

Each release tarball contains:

```
asterc-0.2.0-aarch64-apple-darwin/
    bin/
        asterc
    lib/
        libaster_runtime.a
    CHECKSUMS.sha256
```

On extraction, the outer directory is renamed/moved to `~/.aster/toolchains/0.2.0/`.

## Implementation Order

This phase depends on Phase 4 (the `aster` CLI) being functional. The work breaks down as:

### 5.1 Seedfile `compiler()` function

Add the `compiler_version` field and `compiler` method to the Seedfile class. This is a Phase 2 addition, but it's trivial and can be done whenever the Seedfile class is implemented.

### 5.2 Directory structure and default management

Create `~/.aster/` on first run. Read/write the `default` file. Implement `aster toolchain list`, `aster toolchain use`, `aster toolchain which`.

No network required. This is the foundation that everything else builds on.

### 5.3 Version discovery and download

Implement `aster toolchain available` (fetch versions.json), `aster toolchain install` (download, verify, extract). Shells out to curl/wget for HTTP.

### 5.4 Shim dispatch

Wire the Seedfile's `compiler_version` into the `aster` CLI's dispatch logic. When a Seedfile pins a version, the CLI delegates to the right `asterc`.

### 5.5 Toolchain linking

Implement `aster toolchain link` for local development builds. This is the escape hatch for compiler developers.

### 5.6 Toolchain removal and cleanup

Implement `aster toolchain remove`. Refuse to remove the active default. Clean up downloads cache.

## Potential Challenges

1. **No release infrastructure yet.** The versions.json and tarballs need to be hosted somewhere. For initial development, use GitHub releases on the asterc repo. The URL can be hardcoded and changed later.

2. **Platform detection is imperfect.** `uname` gives us the basics, but edge cases exist (musl vs glibc on Linux, Rosetta on macOS). Start with the four main targets (aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu) and add more as needed.

3. **The aster shim needs to be fast.** Every `aster` invocation goes through the shim. The Seedfile evaluation (JIT compile + execute) adds latency. Mitigate by caching the Seedfile evaluation result (keyed by file mtime + content hash). If the cache is warm, the shim just reads a few bytes from disk and execs the right binary.

4. **Self-update.** The `aster` shim itself is not version-managed by the toolchain manager. Updating the shim is a separate concern (OS package manager, or a future `aster self-update` command). Don't try to solve this in the initial implementation.

## Validation Steps

- `aster toolchain list` shows installed versions
- `aster toolchain install 0.1.0` downloads and installs a toolchain
- `aster toolchain use 0.1.0` sets the default
- `aster toolchain which` prints the path to the active asterc
- A Seedfile with `compiler(version: "0.1.0")` causes `aster build` to use that version
- A Seedfile requiring an uninstalled version auto-installs it and continues the build
- `aster toolchain link local /path/to/build` creates a usable custom toolchain
- `aster toolchain remove 0.1.0` cleans up
- Removing the active default is rejected with a clear message
