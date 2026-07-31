# RunboxJS Architecture

This document describes the internal architecture of RunboxJS, its module structure, data flow, and key design decisions.

---

## Table of Contents

- [High-Level Overview](#high-level-overview)
- [Module Map](#module-map)
- [Data Flow](#data-flow)
- [Virtual Filesystem (VFS)](#virtual-filesystem-vfs)
- [Shell and Command Routing](#shell-and-command-routing)
- [Runtime System](#runtime-system)
  - [Bun/Node Runtime](#bunnode-runtime)
  - [Package Manager Runtime](#package-manager-runtime)
  - [Git Runtime](#git-runtime)
  - [Python Runtime](#python-runtime)
  - [Shell Builtins](#shell-builtins)
  - [JavaScript Engine](#javascript-engine)
- [Process Manager](#process-manager)
- [Console System](#console-system)
- [Terminal System](#terminal-system)
- [Hot Reload Engine](#hot-reload-engine)
- [DOM Inspector](#dom-inspector)
- [Network Layer](#network-layer)
- [Sandbox Bridge](#sandbox-bridge)
- [MCP Server](#mcp-server)
- [AI Tooling](#ai-tooling)
- [WASM Bindings Layer](#wasm-bindings-layer)
- [Native Binary (CLI)](#native-binary-cli)
- [Dual-Target Architecture](#dual-target-architecture)
- [Error Handling](#error-handling)
- [Dependencies](#dependencies)

---

## High-Level Overview

RunboxJS is a Rust crate compiled to two targets:

1. **WebAssembly (`wasm32`)** -- runs in the browser via `wasm-bindgen`, exposing `RunboxInstance` as a JavaScript class with 40+ methods.
2. **Native binary** -- runs as an MCP server over stdio, serving the same VFS, shell, and console capabilities to AI clients like Claude Desktop or Cursor.

The architecture is layered:

```
┌──────────────────────────────────────────────────────┐
│                    Consumer Layer                     │
│       (Browser JS / MCP Client / AI Assistant)       │
├──────────────────────────────────────────────────────┤
│                   Bindings Layer                      │
│           wasm.rs (WASM) / main.rs (Native)          │
├──────────────────────────────────────────────────────┤
│                  Orchestration Layer                  │
│        shell.rs  ·  sandbox.rs  ·  ai/skills.rs      │
├──────────────────────────────────────────────────────┤
│                    Runtime Layer                      │
│  bun.rs · npm.rs · git.rs · python.rs · builtins.rs  │
│                   js_engine.rs                        │
├──────────────────────────────────────────────────────┤
│                 Infrastructure Layer                  │
│  vfs.rs · process.rs · console.rs · terminal.rs      │
│  hotreload.rs · inspector.rs · network.rs · error.rs │
├──────────────────────────────────────────────────────┤
│                     MCP Layer                         │
│  server.rs · client.rs · registry.rs · protocol.rs   │
│                  transport.rs                         │
└──────────────────────────────────────────────────────┘
```

---

## Module Map

The crate is organized into the following modules (defined in `src/lib.rs`):

| Module | File(s) | Purpose |
|---|---|---|
| `vfs` | `src/vfs.rs` | In-memory virtual filesystem with change tracking |
| `shell` | `src/shell.rs` | Command parsing and runtime target detection |
| `runtime` | `src/runtime/mod.rs` | Runtime trait definition and `ExecOutput` type |
| `runtime::bun` | `src/runtime/bun.rs` | Bun/Node.js runtime implementation |
| `runtime::npm` | `src/runtime/npm.rs` | Package manager runtime (npm/pnpm/yarn/bun) |
| `runtime::git` | `src/runtime/git.rs` | In-memory git implementation |
| `runtime::python` | `src/runtime/python.rs` | Python/pip runtime |
| `runtime::shell_builtins` | `src/runtime/shell_builtins.rs` | Shell built-in commands (echo, ls, cat, etc.) |
| `runtime::js_engine` | `src/runtime/js_engine.rs` | JavaScript/TypeScript execution engine |
| `process` | `src/process.rs` | Process registry and lifecycle management |
| `console` | `src/console.rs` | Structured log collection with levels and filtering |
| `terminal` | `src/terminal.rs` | Terminal I/O streams for xterm.js integration |
| `hotreload` | `src/hotreload.rs` | Hot reload engine with debounced change detection |
| `inspector` | `src/inspector.rs` | DOM inspector session management |
| `network` | `src/network.rs` | HTTP request/response handling and SW bridge |
| `sandbox` | `src/sandbox.rs` | Sandbox command/event bridge |
| `ai` | `src/ai/mod.rs` | AI module entry point |
| `ai::tools` | `src/ai/tools.rs` | AI tool schema definitions |
| `ai::skills` | `src/ai/skills.rs` | AI skill dispatch and execution |
| `mcp` | `src/mcp/mod.rs` | MCP module entry point |
| `mcp::server` | `src/mcp/server.rs` | MCP server implementation |
| `mcp::client` | `src/mcp/client.rs` | MCP client for connecting to external servers |
| `mcp::registry` | `src/mcp/registry.rs` | Multi-server MCP registry with namespacing |
| `mcp::protocol` | `src/mcp/protocol.rs` | MCP/JSON-RPC 2.0 protocol types |
| `mcp::transport` | `src/mcp/transport.rs` | Transport layer (stdio, SSE, WebSocket, in-process) |
| `error` | `src/error.rs` | Error types and result aliases |
| `wasm` | `src/wasm.rs` | WASM bindings (only compiled for `wasm32`) |

---

## Data Flow

### Command Execution Flow

```
User code                   Shell                    Runtime                  VFS
  │                          │                         │                      │
  │  runbox.exec("npm add express")                    │                      │
  │─────────────────────────>│                         │                      │
  │                          │ Command::parse(line)    │                      │
  │                          │──────────┐              │                      │
  │                          │<─────────┘              │                      │
  │                          │ RuntimeTarget::detect   │                      │
  │                          │──────────┐              │                      │
  │                          │<─────────┘ → Npm        │                      │
  │                          │                         │                      │
  │                          │ PackageManagerRuntime    │                      │
  │                          │ ::npm().exec(cmd, vfs, pm)                     │
  │                          │────────────────────────>│                      │
  │                          │                         │ Read package.json    │
  │                          │                         │─────────────────────>│
  │                          │                         │<─────────────────────│
  │                          │                         │ Write package.json   │
  │                          │                         │─────────────────────>│
  │                          │                         │ Write lockfile       │
  │                          │                         │─────────────────────>│
  │                          │  ExecOutput { stdout,   │                      │
  │                          │    stderr, exit_code }  │                      │
  │                          │<────────────────────────│                      │
  │  JSON string             │                         │                      │
  │<─────────────────────────│                         │                      │
```

### Hot Reload Flow

```
File Write           VFS               HotReload Engine            Browser
  │                   │                      │                       │
  │ write_file()      │                      │                       │
  │──────────────────>│                      │                       │
  │                   │ track change         │                       │
  │                   │─────────┐            │                       │
  │                   │<────────┘            │                       │
  │                                          │                       │
  │ hot_tick(now_ms)                         │                       │
  │─────────────────────────────────────────>│                       │
  │                                          │ check debounce        │
  │                                          │ classify changes      │
  │                                          │ (CSS/JS/other)        │
  │                                          │                       │
  │  { type: "inject_css",                   │                       │
  │    paths: ["/app.css"] }                 │                       │
  │<─────────────────────────────────────────│                       │
  │                                                                  │
  │  Apply CSS/HMR/reload                                            │
  │─────────────────────────────────────────────────────────────────>│
```

---

## Virtual Filesystem (VFS)

**File:** `src/vfs.rs`

The VFS is the foundation of RunboxJS. It provides an in-memory filesystem with these key features:

### Data Structure

```rust
pub struct Vfs {
    files: HashMap<String, Vec<u8>>,     // path -> content
    dirty: HashSet<String>,              // changed paths for hot reload
}
```

### Operations

| Operation | Description |
|---|---|
| `write(path, content)` | Creates or overwrites a file, creates parent directories implicitly, marks as dirty |
| `read(path)` | Returns file content as byte slice |
| `list(path)` | Lists entries in a directory |
| `exists(path)` | Checks if a file or directory exists |
| `remove(path)` | Removes a file or directory recursively |
| `drain_dirty()` | Returns and clears all dirty (changed) paths |
| `search(root, query, ext)` | Recursive text search with optional extension filter |
| `file_tree(root, depth)` | Generates indented file tree string |

### Path Handling

- All paths are normalized to start with `/`
- Directory entries are tracked implicitly (any path with children is a directory)
- Paths use forward slashes only

### Change Tracking

When a file is written, its path is added to the `dirty` set. The hot reload engine periodically calls `drain_dirty()` to collect changed paths and determine the reload strategy.

---

## Shell and Command Routing

**File:** `src/shell.rs`

The shell module is responsible for:

1. **Parsing** raw command lines into structured `Command` objects
2. **Detecting** which runtime should handle each command

### Command Structure

```rust
pub struct Command {
    pub program: String,        // e.g., "npm", "git", "bun"
    pub args: Vec<String>,      // remaining arguments
    pub raw: String,            // original command line
}
```

### Runtime Target Detection

`RuntimeTarget::detect(&cmd)` maps the `program` field to a runtime:

```rust
pub enum RuntimeTarget {
    Bun,      // bun, node, nodejs, tsx, ts-node
    Npm,      // npm, npx
    Pnpm,     // pnpm, pnpx
    Yarn,     // yarn
    Git,      // git
    Python,   // python, python3, pip, pip3
    Shell,    // echo, ls, cat, pwd, mkdir, rm, touch, cd, cp, mv
    Unknown,  // unrecognized commands
}
```

### Parsing Features

- Supports quoted arguments (single and double quotes)
- Handles escaped characters
- Splits on whitespace respecting quote boundaries
- Preserves the raw command line for display

---

## Runtime System

**File:** `src/runtime/mod.rs`

All runtimes implement the `Runtime` trait:

```rust
pub trait Runtime {
    fn name(&self) -> &str;
    fn exec(&self, cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput, RunboxError>;
}

pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}
```

### Bun/Node Runtime

**File:** `src/runtime/bun.rs` (481 lines)

Handles JavaScript and TypeScript execution:

| Subcommand | Behavior |
|---|---|
| `bun run <script>` | Looks up script in `package.json`, or falls back to direct file execution |
| `bun install` | Delegates to `PackageManagerRuntime` |
| `bun add <pkg>` | Delegates to `PackageManagerRuntime` |
| `bun build <entry>` | Reads entry file from VFS, creates bundled output |
| `bun test` | Scans for `*.test.*` / `*.spec.*` files and runs them |
| `node <file>` | Reads file from VFS, strips TypeScript if needed, evaluates JS |

**JavaScript Execution Strategy:**
- **WASM target:** Uses `js_sys::eval()` with polyfills for `require()`, `process`, `path`, `http`, `fs`
- **Native target:** Uses `boa_engine` (pure Rust JS interpreter)

**Module Loading:**
- ESM `import` statements are transformed to `require()` calls
- VFS modules are preloaded into `globalThis.__vfs_modules`
- `node_modules` packages are resolved from VFS

### Package Manager Runtime

**File:** `src/runtime/npm.rs` (855 lines)

Unified runtime for npm, pnpm, yarn, and bun package operations:

| Command Family | npm | pnpm | yarn | bun |
|---|---|---|---|---|
| Install all | `npm install` | `pnpm install` | `yarn install` | `bun install` |
| Add package | `npm add` | `pnpm add` | `yarn add` | `bun add` |
| Remove package | `npm remove` | `pnpm remove` | `yarn remove` | `bun remove` |
| Run script | `npm run` | `pnpm run` | `yarn run` | `bun run` |
| Execute | `npx` | `pnpx` / `pnpm dlx` | `yarn dlx` | `bunx` |
| Init project | `npm init` | `pnpm init` | `yarn init` | `bun init` |
| List packages | `npm list` | `pnpm list` | `yarn list` | `bun list` |
| Update | `npm update` | `pnpm update` | `yarn upgrade` | `bun update` |
| Audit | `npm audit` | `pnpm audit` | `yarn audit` | `bun audit` |

**Lockfile Generation:**

Each package manager generates its own lockfile format in the VFS:

| Manager | Lockfile Path | Format |
|---|---|---|
| npm | `/package-lock.json` | JSON (`lockfileVersion: 3`) |
| pnpm | `/pnpm-lock.yaml` | YAML |
| yarn | `/yarn.lock` | Custom format |
| bun | `/bun.lock` | JSON-like |

**WASM Install Strategy:**

In the browser, synchronous HTTP requests are not available. The install flow uses a two-phase bridge:

1. `npm_packages_needed()` -- returns JSON array of `{ name, version }` objects for missing dependencies
2. Host JavaScript fetches tarballs from the npm registry
3. `npm_process_tarball(name, version, bytes)` -- extracts the tarball into `/node_modules/<name>/` in the VFS

### Git Runtime

**File:** `src/runtime/git.rs` (1376 lines)

A complete in-memory git implementation:

**Storage Layout in VFS:**

```
/.git/
  HEAD              → "ref: refs/heads/main"
  config            → user.name, user.email, token
  index             → staged file list (JSON)
  log               → commit objects (JSON array)
  refs/
    heads/
      main          → latest commit hash
      <branch>      → branch commit hash
    remotes/
      origin/
        <branch>    → remote tracking ref
  objects/          → (not used; commits stored in log)
```

**Local Operations:**

| Command | Description |
|---|---|
| `git init` | Creates `.git/` structure with HEAD pointing to `main` |
| `git add <pathspec>` | Adds files to index (`.` adds all non-gitignored files) |
| `git commit -m "msg"` | Creates commit object with SHA1 hash, updates HEAD ref |
| `git status` | Shows staged, unstaged, and untracked files |
| `git log` | Shows commit history with hash, author, date, message |
| `git diff` | Shows unified diff of changes |
| `git branch` | Lists, creates, or deletes branches |
| `git checkout` | Switches branches or creates with `-b` |
| `git merge <branch>` | Merges branch into current (fast-forward or three-way) |
| `git reset` | Resets HEAD to a previous commit |

**Network Operations (HTTP Smart Protocol):**

| Command | Description |
|---|---|
| `git remote add` | Adds remote URL (defaults to `origin`) |
| `git clone <url>` | Clones via HTTP smart protocol (info/refs + git-upload-pack) |
| `git fetch` | Fetches refs and objects from remote |
| `git pull` | Fetch + merge |
| `git push` | Pushes commits via git-receive-pack |

**Credential Management:**
- `git_set_user(name, email)` -- stored in `/.git/config`
- `git_set_token(token)` -- stored in `/.git/config`, used as Bearer token for HTTP auth

**Commit Hashing:**
Uses `sha1_smol` to compute SHA1 hashes from commit content (author, message, timestamp, parent, tree).

### Python Runtime

**File:** `src/runtime/python.rs` (264 lines)

| Command | Behavior |
|---|---|
| `python <file>` | **Native:** spawns system `python3`/`python`. **WASM:** returns message to use Pyodide adapter |
| `python -c "code"` | Executes inline Python code |
| `pip install <pkg>` | Records package in internal registry, writes to VFS marker files |
| `pip list` | Lists installed pip packages |
| `pip show <pkg>` | Shows package metadata |
| `pip freeze` | Outputs `requirements.txt` format |

### Shell Builtins

**File:** `src/runtime/shell_builtins.rs` (83 lines)

| Command | Description |
|---|---|
| `echo <args>` | Prints arguments to stdout |
| `pwd` | Prints current working directory (`/`) |
| `ls [path]` | Lists directory contents from VFS |
| `cat <file>` | Reads and prints file content from VFS |
| `mkdir <path>` | Creates directory in VFS |
| `rm <path>` | Removes file or directory from VFS |
| `touch <file>` | Creates empty file in VFS |

### JavaScript Engine

**File:** `src/runtime/js_engine.rs` (722 lines)

The JS engine handles actual JavaScript/TypeScript evaluation:

**TypeScript Stripping (`strip_typescript`):**

Removes TypeScript-specific syntax to produce valid JavaScript:
- Type annotations (`: string`, `: number`, etc.)
- Interfaces and type aliases
- Generic parameters (`<T>`)
- Access modifiers (`public`, `private`, `protected`, `readonly`)
- Decorators (`@decorator`)
- Non-null assertions (`!`)
- Type assertions (`as Type`, `<Type>`)
- `declare` blocks
- `enum` declarations (converted to objects)

**WASM Execution:**

Uses `js_sys::eval()` with polyfills injected into the global scope:
- `globalThis.require` -- resolves VFS modules and `node_modules`
- `globalThis.process` -- `{ env: {}, argv: [], cwd: () => "/", exit: () => {} }`
- `globalThis.__vfs_modules` -- preloaded module map from VFS
- `globalThis.__runbox_servers` -- HTTP server registry for Express-style apps
- `module.exports` / `exports` -- CommonJS compatibility

**Native Execution:**

Uses `boa_engine` (pure Rust ECMAScript interpreter):
- Sets up a `Context` with source
- Evaluates the (TypeScript-stripped) source
- Captures the result or error

**ESM to CommonJS Transform:**

The engine transforms ESM import/export statements for eval() compatibility:
- `import x from 'mod'` → `const x = require('mod')`
- `import { a, b } from 'mod'` → `const { a, b } = require('mod')`
- `export default expr` → `module.exports.default = expr`
- `export { a, b }` → `module.exports.a = a; module.exports.b = b`

---

## Process Manager

**File:** `src/process.rs`

Lightweight process registry tracking running processes:

```rust
pub struct ProcessManager {
    processes: Vec<ProcessInfo>,
    next_pid: u32,
}

pub struct ProcessInfo {
    pub pid: u32,
    pub command: String,
    pub args: Vec<String>,
    pub status: ProcessStatus,
}
```

Used by:
- The MCP server to list active processes
- Command execution to register/deregister running commands
- Console to tag log entries with PIDs

---

## Console System

**File:** `src/console.rs`

Structured log collection with:

```rust
pub struct Console {
    entries: Vec<LogEntry>,
    next_id: u64,
}

pub struct LogEntry {
    pub id: u64,
    pub level: LogLevel,
    pub message: String,
    pub source: String,
    pub timestamp_ms: u64,
}

pub enum LogLevel { Log, Info, Warn, Error, Debug }
```

**Features:**
- Auto-incrementing IDs for cursor-based pagination
- Level-based filtering (`by_level`)
- Time-based filtering (`since`)
- Process output ingestion (`ingest_process` splits stdout/stderr into log entries)
- JSON serialization for transport

---

## Terminal System

**File:** `src/terminal.rs`

Bidirectional terminal I/O streams designed for xterm.js integration:

```rust
pub struct Terminal {
    output_buffer: Vec<TermChunk>,
    cols: u16,
    rows: u16,
}

pub struct TermChunk {
    pub pid: u32,
    pub data: String,
}
```

**Operations:**
- `input(data, pid)` -- feeds user input to a process
- `drain()` -- returns and clears all pending output chunks
- `resize(cols, rows)` -- updates terminal dimensions
- `size()` -- returns current dimensions as `{ cols, rows }`
- `clear()` -- clears the output buffer

---

## Hot Reload Engine

**File:** `src/hotreload.rs`

Intelligent file change detection with debounced reload strategies:

```rust
pub struct HotReloadEngine {
    pending: Vec<String>,          // accumulated changed paths
    last_signal_ms: f64,           // timestamp of last signal
    debounce_ms: f64,              // debounce interval (default: 300ms)
}
```

**Reload Strategies:**

| File Extension | Strategy | Description |
|---|---|---|
| `.css` | `inject_css` | Injects updated CSS without page reload |
| `.js`, `.ts`, `.jsx`, `.tsx` | `hmr` | Hot Module Replacement |
| Others | `full_reload` | Full page reload |

**API:**
- `hot_tick(now_ms)` -- checks if debounce period has elapsed and returns pending action (or `null`)
- `hot_flush()` -- immediately returns any pending action regardless of debounce

**Action Format:**
```json
{
  "type": "inject_css",
  "paths": ["/src/styles/app.css", "/src/styles/header.css"]
}
```

---

## DOM Inspector

**File:** `src/inspector.rs`

Browser-side element inspection with session management:

```rust
pub struct Inspector {
    active: bool,
    selected: Option<NodeData>,
    history: Vec<NodeData>,
    overlay: Option<OverlayData>,
}
```

**Node Data:**
```rust
pub struct NodeData {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attributes: HashMap<String, String>,
    pub box_model: Option<BoxModel>,
    pub computed_styles: HashMap<String, String>,
}

pub struct BoxModel {
    pub x: f64, pub y: f64,
    pub width: f64, pub height: f64,
    pub margin: Rect, pub padding: Rect, pub border: Rect,
}
```

**Request Types:**
- `point:x,y` -- select element at screen coordinates
- `selector:.class` -- select element by CSS selector
- `dismiss` -- deselect current element

---

## Network Layer

**File:** `src/network.rs`

Handles HTTP request/response simulation and Service Worker bridge:

**HTTP Request Handling:**
- Receives serialized request objects from the browser
- Routes to `globalThis.__runbox_servers` handlers (Express-style)
- Falls back to VFS file serving for static assets
- Returns serialized response objects

**Service Worker Bridge:**
- Intercepts `fetch` events from a Service Worker
- Resolves files from VFS
- Returns appropriate MIME types and headers

---

## Sandbox Bridge

**File:** `src/sandbox.rs`

General-purpose command/event bridge between the host application and the sandbox:

- `sandbox_command(cmd_json)` -- receives structured commands from the host
- `sandbox_event(event_json)` -- receives lifecycle events (reload, navigate, etc.)

---

## MCP Server

**File:** `src/mcp/server.rs` (599 lines)

Full MCP server implementing JSON-RPC 2.0 over the Model Context Protocol specification:

### Architecture

```
┌─────────────┐     JSON-RPC      ┌───────────┐
│  MCP Client │ ◄──────────────► │ McpServer │
│ (Claude,    │   stdin/stdout    │           │
│  Cursor,    │   HTTP/SSE        │  ┌─────┐  │
│  Zed)       │   WebSocket       │  │ VFS │  │
└─────────────┘                   │  ├─────┤  │
                                  │  │Shell│  │
                                  │  ├─────┤  │
                                  │  │ Con │  │
                                  │  └─────┘  │
                                  └───────────┘
```

### Protocol Types

**File:** `src/mcp/protocol.rs` (294 lines)

Defines JSON-RPC 2.0 and MCP-specific types:
- `RpcRequest`, `RpcResponse`, `RpcError`
- `McpTool`, `McpResource`, `McpPrompt`
- `McpContent` (Text, Image, Resource)
- `InitializeResult`, `ServerCapabilities`
- Standard error codes (`PARSE_ERROR`, `INVALID_REQUEST`, `METHOD_NOT_FOUND`, `INVALID_PARAMS`, `INTERNAL_ERROR`)

### Transport Layer

**File:** `src/mcp/transport.rs` (280 lines)

Four transport implementations behind the `McpTransport` trait:

| Transport | Target | Description |
|---|---|---|
| `StdioTransport` | Native | Spawns child process, communicates via stdin/stdout |
| `SseTransport` | Native | HTTP POST for sending + GET `/sse` for receiving events |
| `WebSocketTransport` | Native | Full-duplex WebSocket via `tungstenite` |
| `InProcessTransport` | Both | Closure-based handler for testing/embedding |

WASM targets have stub implementations that direct users to browser-native APIs (`EventSource`, `WebSocket`).

### Client and Registry

**File:** `src/mcp/client.rs` (149 lines), `src/mcp/registry.rs` (164 lines)

- `McpClient` -- connects to a single external MCP server with initialize/call_tool/read_resource
- `McpRegistry` -- manages multiple connected servers with namespaced tool routing (`server_name/tool_name`)

For complete MCP details, see [MCP_GUIDE.md](./MCP_GUIDE.md).

---

## AI Tooling

### Tool Definitions

**File:** `src/ai/tools.rs` (282 lines)

Defines 9 tools with JSON Schema parameter definitions:

| Tool | Parameters | Description |
|---|---|---|
| `read_file` | `{ path }` | Read a file from VFS |
| `write_file` | `{ path, content }` | Write a file to VFS |
| `list_dir` | `{ path }` | List directory contents |
| `exec_command` | `{ command }` | Execute a shell command |
| `search_code` | `{ query, path?, extension? }` | Search files for text |
| `get_console_logs` | `{ level?, since_id? }` | Get console log entries |
| `reload_sandbox` | `{ hard? }` | Trigger sandbox reload |
| `install_packages` | `{ packages, dev?, manager? }` | Install npm packages |
| `get_file_tree` | `{ path?, depth? }` | Get file tree visualization |

**Provider Format Serialization:**

Tools can be serialized to multiple AI provider formats:
- `to_openai_format(&tools)` -- OpenAI function calling format
- `to_anthropic_format(&tools)` -- Anthropic tool use format
- `to_gemini_format(&tools)` -- Google Gemini function declarations

### Skill Dispatch

**File:** `src/ai/skills.rs` (301 lines)

The `dispatch()` function routes tool calls to skill implementations:

```rust
pub fn dispatch(call_json: &str, vfs: &mut Vfs, pm: &mut ProcessManager, console: &mut Console) -> String
```

Each skill function (`skill_read_file`, `skill_write_file`, etc.) implements the actual logic, interacting with VFS, the process manager, and the console. Results are returned as JSON strings with `{ name, content, error? }` format.

---

## WASM Bindings Layer

**File:** `src/wasm.rs` (536 lines, only compiled for `wasm32`)

`RunboxInstance` is the main export, annotated with `#[wasm_bindgen]`. It owns:

```rust
pub struct RunboxInstance {
    vfs: Vfs,
    pm: ProcessManager,
    console: Console,
    terminal: Terminal,
    hot: HotReloadEngine,
    inspector: Inspector,
}
```

All methods return either:
- `String` (JSON-serialized data)
- `Result<T, JsValue>` (for operations that can fail)
- Primitive types (`bool`, `u32`)

The bindings layer handles serialization/deserialization between Rust types and JavaScript values.

---

## Native Binary (CLI)

**File:** `src/main.rs` (75 lines)

The native binary runs as an MCP server:

```
runbox              -- MCP server mode (reads JSON-RPC from stdin, responds on stdout)
runbox --list-tools -- prints available AI tools in OpenAI format and exits
```

- Logging goes to stderr to avoid contaminating the JSON-RPC channel
- Log level controlled via `RUNBOX_LOG` environment variable (default: `warn`)

---

## Dual-Target Architecture

RunboxJS is designed to compile for both WASM and native targets:

| Feature | WASM (`wasm32`) | Native |
|---|---|---|
| JS execution | `js_sys::eval()` | `boa_engine` |
| HTTP requests | Not available (bridge pattern) | `reqwest` |
| File I/O | VFS only | VFS + optional `tempfile` |
| WebSocket | Stub (use browser API) | `tungstenite` |
| Process spawning | Not available | `std::process::Command` |
| Async runtime | `wasm-bindgen-futures` | `tokio` |
| Entry point | `RunboxInstance` (JS class) | `main()` (MCP server) |

Conditional compilation is used extensively:

```rust
#[cfg(target_arch = "wasm32")]
// WASM-specific code

#[cfg(not(target_arch = "wasm32"))]
// Native-specific code
```

---

## Error Handling

**File:** `src/error.rs`

Uses `thiserror` for structured error types:

```rust
pub enum RunboxError {
    Io(String),           // I/O operations
    NotFound(String),     // File/resource not found
    Shell(String),        // Command parsing errors
    Runtime(String),      // Runtime execution errors
    Parse(String),        // JSON/data parsing errors
    Git(String),          // Git operation errors
    Network(String),      // Network/HTTP errors
}
```

All errors implement `Display` and can be converted to `JsValue` for WASM or `anyhow::Error` for native.

---

## Dependencies

### Core (all targets)

| Crate | Version | Purpose |
|---|---|---|
| `serde` | 1 | Serialization/deserialization |
| `serde_json` | 1 | JSON handling |
| `thiserror` | 1 | Error derive macros |
| `anyhow` | 1 | Error context |
| `tracing` | 0.1 | Structured logging |
| `sha1_smol` | 1 | SHA1 hashing for git objects |
| `chrono` | 0.4 | Timestamps (std + clock features) |
| `flate2` | 1 | Gzip compression (tar extraction) |
| `tar` | 0.4 | Tar archive handling |

### WASM-only

| Crate | Version | Purpose |
|---|---|---|
| `wasm-bindgen` | 0.2 | Rust-to-JS bindings |
| `wasm-bindgen-futures` | 0.4 | Async support |
| `js-sys` | 0.3 | JavaScript built-in bindings |
| `web-sys` | 0.3 | Web API bindings (console, WebSocket, etc.) |

### Native-only

| Crate | Version | Purpose |
|---|---|---|
| `tokio` | 1 | Async runtime |
| `tracing-subscriber` | 0.3 | Log formatting |
| `reqwest` | 0.12 | HTTP client (blocking + JSON) |
| `tempfile` | 3 | Temporary file creation |
| `tungstenite` | 0.24 | WebSocket client |
| `boa_engine` | 0.19 | Pure Rust JavaScript interpreter |

### Dev Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `criterion` | 0.5 | Benchmarking framework |

---

## Build Profile

The release profile is optimized for small WASM binary size:

```toml
[profile.release]
opt-level = "z"      # Optimize for size
lto = true           # Link-time optimization
codegen-units = 1    # Single codegen unit for better optimization
panic = "abort"      # Abort on panic (smaller binary)
```
