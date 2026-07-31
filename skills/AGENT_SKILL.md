# RunBox Agent Skill Guide — Complete Reference

> **Version**: 0.3.9 | **Crate**: `runbox` | **Edition**: Rust 2024 | **License**: MIT
>
> This document is the single source of truth for any AI agent integrating with RunBox.
> It covers every API, every command, every runtime, every pattern, and every edge case.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Boot Sequence](#2-boot-sequence)
3. [AI Tool Surface — Complete Reference](#3-ai-tool-surface--complete-reference)
4. [RunboxInstance WASM API — All Public Methods](#4-runboxinstance-wasm-api--all-public-methods)
5. [Command Routing & Runtime Detection](#5-command-routing--runtime-detection)
6. [Runtime Reference — npm / pnpm / yarn](#6-runtime-reference--npm--pnpm--yarn)
7. [Runtime Reference — Bun](#7-runtime-reference--bun)
8. [Runtime Reference — Git](#8-runtime-reference--git)
9. [Runtime Reference — Python](#9-runtime-reference--python)
10. [Runtime Reference — Shell Builtins](#10-runtime-reference--shell-builtins)
11. [Virtual Filesystem (VFS)](#11-virtual-filesystem-vfs)
12. [Console System](#12-console-system)
13. [Process Manager](#13-process-manager)
14. [Hot Reload System](#14-hot-reload-system)
15. [DOM Inspector](#15-dom-inspector)
16. [Terminal Integration](#16-terminal-integration)
17. [Network & Service Worker](#17-network--service-worker)
18. [MCP Server (Model Context Protocol)](#18-mcp-server-model-context-protocol)
19. [MCP Client & Registry](#19-mcp-client--registry)
20. [Provider-Specific Tool Formatting](#20-provider-specific-tool-formatting)
21. [Error Handling Patterns](#21-error-handling-patterns)
22. [Decision Trees](#22-decision-trees)
23. [Advanced Workflows](#23-advanced-workflows)
24. [Security Practices](#24-security-practices)
25. [Performance Considerations](#25-performance-considerations)
26. [Constraints & Limitations](#26-constraints--limitations)
27. [Troubleshooting Guide](#27-troubleshooting-guide)

---

## 1. Architecture Overview

RunBox is a **WebAssembly sandbox runtime** for executing project workflows in the browser. It compiles to two targets:

| Target | Use Case | JS Engine | Network | Shell |
|--------|----------|-----------|---------|-------|
| `wasm32` | Browser sandbox | `js_sys::eval()` | Service Worker intercept | Virtual |
| Native | MCP server, CLI, tests | `boa_engine` (pure Rust) | `reqwest` blocking | System fallback |

### Core Subsystems

```
+--------------------------------------------------------------+
|                    RunboxInstance (WASM)                       |
|  +---------+ +-----------+ +---------+ +------------+        |
|  |   VFS   | |   Shell   | | Console | |  Process   |        |
|  | in-mem  | |  parser   | | circular| |  manager   |        |
|  | tree    | | tokenizer | | buffer  | |  PID track |        |
|  +----+----+ +-----+-----+ +----+----+ +-----+------+        |
|       |            |            |             |               |
|  +----+------------+------------+-------------+----------+    |
|  |              Runtime Router                            |   |
|  |  Bun | Node | npm | pnpm | yarn | git | python | shell|   |
|  +-------------------------------------------------------+   |
|  +----------+ +------------+ +----------+ +---------+        |
|  | Hot      | |  Inspector | | Terminal | | Network |        |
|  | Reload   | |  DOM       | | xterm.js | | SW/HTTP |        |
|  +----------+ +------------+ +----------+ +---------+        |
|  +-------------------------------------------------------+   |
|  |        AI Layer (tools.rs + skills.rs)                 |   |
|  |  9 tools x 3 provider formats (OpenAI/Claude/Gemini)  |   |
|  |  + dispatch engine                                     |   |
|  +-------------------------------------------------------+   |
|  +-------------------------------------------------------+   |
|  |        MCP Layer (server + client + registry)          |   |
|  |  JSON-RPC 2.0 stdio transport                          |   |
|  |  8 tools, 3+ resources, 3 prompts                      |   |
|  +-------------------------------------------------------+   |
+--------------------------------------------------------------+
```

### Dual-Target Compilation

- **WASM build**: `wasm-pack build --target web` — exposes `RunboxInstance` to JavaScript via `wasm-bindgen`
- **Native build**: `cargo build` — produces MCP stdio server binary (`src/main.rs`)

---

## 2. Boot Sequence

### WASM (Browser)

```ts
import init, { RunboxInstance } from 'runboxjs';

// Step 1: Initialize WASM module (REQUIRED before anything else)
await init();

// Step 2: Create instance
const runbox = new RunboxInstance();

// Step 3: (Optional) Configure git credentials
runbox.git_set_token("ghp_...");
runbox.git_set_user("username", "user@email.com");

// Step 4: Ready — begin operations
const tree = runbox.ai_tools("openai"); // Get tool definitions
```

**CRITICAL**: Never call any RunboxInstance method before `await init()` completes. The WASM module must be fully loaded.

### Native (MCP Server)

```bash
# Run as MCP stdio server (default mode)
cargo run
# Reads JSON-RPC from stdin, writes to stdout, logs to stderr

# List available tools and exit
cargo run -- --list-tools
```

The native binary reads `RUST_LOG` env var for log filtering (default: `info`). Logs go to stderr to avoid contaminating the JSON-RPC channel on stdout.

---

## 3. AI Tool Surface — Complete Reference

The AI layer exposes **9 tools** via `ai_dispatch`. Each tool accepts a JSON object and returns a `ToolResult`.

### ToolResult Format

Every tool call returns:

```json
{
  "name": "tool_name",
  "content": "success output string",
  "error": null
}
```

On failure:

```json
{
  "name": "tool_name",
  "content": null,
  "error": "error description"
}
```

### Tool 1: `read_file`

Read a file from the virtual filesystem.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | YES | Absolute VFS path (must start with `/`) |

```json
{ "name": "read_file", "arguments": { "path": "/src/index.ts" } }
```

**Returns**: File content as string. Binary files return `[binary output]`.

**Errors**: `"not found: /path"` if file doesn't exist. `"/path is a directory"` if path is a directory.

---

### Tool 2: `write_file`

Create or overwrite a file. Automatically creates intermediate directories.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | YES | Absolute VFS path |
| `content` | string | YES | File content (UTF-8) |

```json
{ "name": "write_file", "arguments": { "path": "/src/app.ts", "content": "console.log('hello');" } }
```

**Returns**: `"wrote N bytes to /path"`

**Side effects**:
- Creates parent directories automatically
- Triggers change tracking (hot reload will detect it)
- Changes inside `/.git/` are excluded from change tracking

---

### Tool 3: `list_dir`

List files and directories inside a path.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | YES | Directory path to list |

```json
{ "name": "list_dir", "arguments": { "path": "/src" } }
```

**Returns**: Sorted newline-separated list of entries.

**Errors**: `"/path is not a directory"` if path is a file. `"not found: path"` if doesn't exist.

---

### Tool 4: `exec_command`

Execute a shell command in the sandbox runtime. The command is parsed, the runtime is auto-detected, and execution is routed to the appropriate handler.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `command` | string | YES | Full command line (e.g., `npm run build`) |

```json
{ "name": "exec_command", "arguments": { "command": "bun run index.ts" } }
```

**Returns**: JSON string with execution result:

```json
{
  "stdout": "output text",
  "stderr": "error text",
  "exit_code": 0
}
```

**Runtime detection**: See [Section 5](#5-command-routing--runtime-detection) for the full routing table.

**Features**:
- Supports environment variables: `NODE_ENV=production bun run build.ts`
- Handles quoted arguments: `echo "hello world"`
- Handles escaped quotes: `echo "say \"hi\""`
- Console output (stdout/stderr) is automatically ingested into the Console system

---

### Tool 5: `search_code`

Search text across project files with optional directory and extension filters.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | YES | Text to search for (case-sensitive substring match) |
| `path` | string | no | Directory to search in (default: `/`) |
| `extension` | string | no | File extension filter (e.g., `.ts`, `.rs`) |

```json
{ "name": "search_code", "arguments": { "query": "useState", "path": "/src", "extension": ".tsx" } }
```

**Returns**: Newline-separated results in format `{path}: {matching_line}`. Returns `"no matches"` if nothing found.

**Behavior**:
- Recursively walks VFS directories
- Skips `.git/` and `node_modules/` directories
- Skips binary files (non-UTF-8)
- Limited to 200 matches max to prevent output overflow

---

### Tool 6: `get_console_logs`

Read console logs from the sandbox.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `level` | string | no | Filter by level: `log`, `info`, `warn`, `error`, `debug` |
| `since_id` | number | no | Only return entries with `id > since_id` (incremental reads) |

```json
{ "name": "get_console_logs", "arguments": { "level": "error", "since_id": 42 } }
```

**Returns**: JSON array of console entries:

```json
[
  {
    "id": 43,
    "level": "error",
    "message": "TypeError: x is not a function",
    "source": "pid:5",
    "pid": 5,
    "timestamp_ms": 1234
  }
]
```

**Tips**:
- Use `since_id` to poll for new logs without re-reading everything
- Console uses a circular buffer (default capacity: 1000 entries)
- Old entries are evicted when buffer is full
- Timestamp is milliseconds since RunBox instance creation

---

### Tool 7: `reload_sandbox`

Request a sandbox reload. Returns metadata about the reload action but does NOT directly trigger it — the host must act on the returned action.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `hard` | boolean | no | `true` = full page reload, `false` = soft/HMR reload (default: `false`) |

```json
{ "name": "reload_sandbox", "arguments": { "hard": true } }
```

**Returns**: JSON describing the reload action:

```json
{ "action": "FullReload" }
```

or

```json
{ "action": "None" }
```

The host JavaScript must interpret this and trigger the actual reload. RunBox does not control the browser directly.

---

### Tool 8: `install_packages`

Install project dependencies with auto-detected or explicit package manager.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `packages` | string[] | no | Specific packages to install. Empty = install all from package.json |
| `dev` | boolean | no | Install as devDependency (default: `false`) |
| `manager` | string | no | Force package manager: `npm`, `pnpm`, `yarn`, `bun` (default: auto-detect) |

```json
{ "name": "install_packages", "arguments": { "packages": ["react", "react-dom"], "dev": false, "manager": "npm" } }
```

**Auto-detection logic** (when `manager` is omitted):
1. If `bun.lock` exists -> `bun`
2. If `pnpm-lock.yaml` exists -> `pnpm`
3. If `yarn.lock` exists -> `yarn`
4. Default -> `npm`

**Returns**: Installation summary with package count.

**Side effects**:
- Updates `/package.json` with new dependencies
- Creates stubs in `/node_modules/<pkg>/package.json`
- Generates appropriate lockfile (`package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, or `bun.lock`)
- On native builds: actually fetches tarballs from `registry.npmjs.org` and extracts to VFS
- On WASM builds: creates stubs (JS host must call `npm_packages_needed()` + `npm_process_tarball()`)

---

### Tool 9: `get_file_tree`

Return a recursive JSON tree of files and directories.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | no | Root path (default: `/`) |
| `depth` | number | no | Max recursion depth (default: 4) |

```json
{ "name": "get_file_tree", "arguments": { "path": "/src", "depth": 3 } }
```

**Returns**: JSON tree structure:

```json
{
  "name": "src",
  "type": "dir",
  "children": [
    { "name": "index.ts", "type": "file" },
    { "name": "components", "type": "dir", "children": [] }
  ]
}
```

**Tips**:
- Use `depth: 1` for shallow listing
- Automatically skips `.git` contents
- Results are sorted alphabetically

---

## 4. RunboxInstance WASM API — All Public Methods

These are the methods exposed to JavaScript via `wasm-bindgen`. All methods are called on a `RunboxInstance` object.

### VFS Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `write_file` | `(path: string, content: Uint8Array)` | Write binary content to VFS |
| `read_file` | `(path: string) -> Uint8Array` | Read binary content from VFS |
| `list_dir` | `(path: string) -> string` | JSON array of directory entries |
| `file_exists` | `(path: string) -> boolean` | Check if path exists |
| `remove_file` | `(path: string)` | Remove file or directory |

### Shell Execution

| Method | Signature | Description |
|--------|-----------|-------------|
| `exec` | `(command_line: string) -> string` | Parse command, detect runtime, execute. Returns JSON `{stdout, stderr, exit_code}` |

**`exec` flow**:
1. `Command::parse(line)` — tokenize and extract program/args/env
2. `RuntimeTarget::detect(&cmd)` — determine which runtime handles it
3. Route to runtime handler (Bun, npm, git, python, shell, etc.)
4. Ingest stdout/stderr into Console
5. Return JSON result

### npm / Package Manager Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `npm_packages_needed` | `() -> string` | JSON array of `{name, version}` packages not yet in `node_modules` |
| `npm_process_tarball` | `(name: string, version: string, bytes: Uint8Array)` | Extract npm tarball into VFS |

**WASM install flow** (JS host must orchestrate):

```js
async function npmInstall(runbox) {
  const needed = JSON.parse(runbox.npm_packages_needed());
  for (const { name, version } of needed) {
    const meta = await fetch(`https://registry.npmjs.org/${name}/${version}`).then(r => r.json());
    const buf = await fetch(meta.dist.tarball).then(r => r.arrayBuffer());
    runbox.npm_process_tarball(name, version, new Uint8Array(buf));
  }
}
```

### Git Credential Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `git_set_token` | `(token: string)` | Set GitHub/GitLab token for push/clone operations |
| `git_set_user` | `(name: string, email: string)` | Set git author identity |

### Console Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `console_push` | `(level: string, message: string, source: string) -> number` | Push a log entry. Returns entry ID. Levels: `log`, `info`, `warn`, `error`, `debug` |
| `console_all` | `() -> string` | JSON array of all console entries |
| `console_since` | `(id: number) -> string` | JSON array of entries with `id > given_id` |
| `console_clear` | `()` | Clear all console entries |

### AI Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `ai_tools` | `(provider: string) -> string` | Get tool definitions formatted for provider. Providers: `openai`, `anthropic`, `gemini` |
| `ai_dispatch` | `(call_json: string) -> string` | Dispatch a tool call. Input: JSON `{name, arguments}`. Output: JSON `{name, content, error}` |

### Sandbox Event Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `sandbox_event` | `(event_json: string) -> string` | Handle sandbox lifecycle events |
| `sandbox_command` | `(cmd_json: string) -> string` | Execute sandbox-level commands |

### Terminal Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `terminal_drain` | `() -> string` | Drain all pending terminal output chunks as JSON |
| `terminal_input` | `(data: string)` | Send user input to terminal |
| `terminal_resize` | `(cols: number, rows: number)` | Resize terminal dimensions |
| `terminal_size` | `() -> string` | Get current terminal size as JSON `{cols, rows}` |
| `terminal_clear` | `()` | Clear terminal buffers |

### HTTP / Service Worker Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `http_handle_request` | `(request_json: string) -> string` | Handle HTTP request internally |
| `sw_handle_request` | `(request_json: string) -> string` | Handle Service Worker intercepted request. Returns `SwResponse` JSON |

### Hot Reload Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `hot_tick` | `() -> string` | Check for pending file changes and return reload action. Returns JSON: `InjectCss`, `Hmr`, `FullReload`, or `None` |
| `hot_flush` | `()` | Force flush all pending changes |

### DOM Inspector Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `inspector_activate` | `()` | Enable DOM inspection mode |
| `inspector_deactivate` | `()` | Disable DOM inspection mode |
| `inspector_is_active` | `() -> boolean` | Check if inspector is active |
| `inspector_set_node` | `(node_json: string)` | Set the inspected node data |
| `inspector_selected` | `() -> string` | Get currently selected node as JSON |
| `inspector_overlay` | `() -> string` | Get overlay rendering data as JSON |
| `inspector_history` | `() -> string` | Get inspection history as JSON array |
| `inspector_request` | `(request_json: string) -> string` | Send inspection request (AtPoint, BySelector, ById, Dismiss) |

---

## 5. Command Routing & Runtime Detection

When you call `exec("some command")`, RunBox parses the command and auto-detects which runtime should handle it.

### Detection Table

| Program | Aliases | Runtime | Handler |
|---------|---------|---------|---------|
| `bun` | `bunx` | Bun | `BunRuntime` |
| `node` | `nodejs`, `tsx`, `ts-node` | Bun (shimmed) | `BunRuntime` — rewrites to `bun run` |
| `npm` | `npx` | npm | `PackageManagerRuntime::npm()` |
| `pnpm` | `pnpx` | pnpm | `PackageManagerRuntime::pnpm()` |
| `yarn` | — | yarn | `PackageManagerRuntime::yarn()` |
| `python` | `python3`, `pip`, `pip3` | Python | `PythonRuntime` |
| `git` | — | Git | `GitRuntime` |
| `curl` | `wget` | Curl | Stub (not fully implemented) |
| `cd`, `ls`, `echo`, `cat`, `pwd`, `mkdir`, `rm`, `cp`, `mv`, `touch` | — | Shell | `ShellBuiltins` |
| _anything else_ | — | Unknown | Returns error |

### Command Parsing

Commands are parsed by a tokenizer that handles:
- **Quoted strings**: `echo "hello world"` -> args: `["hello world"]`
- **Single quotes**: `echo 'hello world'` -> args: `["hello world"]`
- **Escaped characters**: `echo "say \"hi\""` -> args: `["say \"hi\""]`
- **Environment variables**: `NODE_ENV=production bun run build.ts` -> env: `[("NODE_ENV", "production")]`, program: `bun`
- **Trailing backslash**: preserved literally

### Important: `node` is shimmed to `bun`

When you run `node index.js`, RunBox internally converts it to `bun run index.js`. The same applies to `tsx` and `ts-node`. This means:
- Node.js-specific APIs are available through Bun's compatibility layer
- In WASM mode, execution uses `js_sys::eval()` with ESM->CJS transformation
- In native mode, RunBox first tries system `bun`, then falls back to `boa_engine`

---

## 6. Runtime Reference — npm / pnpm / yarn

All three package managers share the same implementation (`PackageManagerRuntime`) with PM-specific lockfile formats.

### Supported Subcommands

| Subcommand | Aliases | Description |
|------------|---------|-------------|
| `install` | `i`, `ci` | Install all deps from package.json |
| `add` | — | Add new package(s) |
| `remove` | `uninstall`, `rm`, `un` | Remove package(s) |
| `run` | — | Run a script from package.json |
| `exec` | `dlx`, `npx`, `pnpx`, `create` | Execute a package binary |
| `init` | — | Create a new package.json |
| `list` | `ls` | List installed packages |
| `update` | `upgrade` | Update packages |
| `outdated` | — | Check for outdated packages |
| `audit` | — | Security audit |

### `npm install` / `pnpm install` / `yarn install`

```json
{ "name": "exec_command", "arguments": { "command": "npm install" } }
```

**Behavior**:
- Reads `/package.json` dependencies and devDependencies
- **Native**: fetches actual tarballs from `registry.npmjs.org`, extracts to `/node_modules/`
- **WASM**: creates stub `package.json` in `/node_modules/<name>/` (host must complete with `npm_packages_needed` + `npm_process_tarball`)
- Generates appropriate lockfile
- Returns summary: `"added N packages (X prod, Y dev)"`

### `npm add <package>` / `pnpm add` / `yarn add`

```json
{ "name": "exec_command", "arguments": { "command": "npm add react@18.2.0 -D" } }
```

**Flags**:
- `-D` / `--save-dev` / `--dev`: Add as devDependency
- `-E` / `--save-exact`: Pin exact version (no `^` prefix)

**Package spec parsing**: `react@18.2.0` -> name: `react`, version: `^18.2.0` (or `18.2.0` with `-E`)

**Side effects**:
- Updates `/package.json`
- Creates `/node_modules/<name>/package.json`
- Regenerates lockfile

### `npm remove <package>`

```json
{ "name": "exec_command", "arguments": { "command": "npm remove lodash" } }
```

Removes from all dependency maps (dependencies, devDependencies, peerDependencies).

### `npm run <script>`

```json
{ "name": "exec_command", "arguments": { "command": "npm run build" } }
```

**Behavior**:
1. Reads `scripts` from `/package.json`
2. If script not found -> error: `"missing script: build"`
3. If script found -> parses and executes the script command through the runtime router
4. Prints header: `"> package-name@version build\n> script-command"`

**Supported script patterns**:
- Simple commands: `"build": "bun run build.ts"` -> executes via Bun runtime
- Chained commands: `"dev": "tsc && node dist/index.js"` -> splits on `&&` and runs sequentially

### `npm init`

```json
{ "name": "exec_command", "arguments": { "command": "npm init" } }
```

Creates a default `/package.json`:

```json
{
  "name": "my-project",
  "version": "1.0.0",
  "description": "",
  "main": "index.js",
  "scripts": { "test": "echo \"Error: no test specified\" && exit 1" },
  "keywords": [],
  "license": "ISC"
}
```

### Lockfile Formats

| Manager | Lockfile | Format |
|---------|----------|--------|
| npm | `package-lock.json` | JSON with lockfileVersion: 3 |
| pnpm | `pnpm-lock.yaml` | YAML with lockfileVersion: 9.0 |
| yarn | `yarn.lock` | Custom text format |
| bun | `bun.lock` | Same as npm (JSON) |

---

## 7. Runtime Reference — Bun

### Supported Subcommands

| Subcommand | Description |
|------------|-------------|
| `bun run <file>` | Execute a JS/TS file |
| `bun run <script>` | Run npm script (delegates to PackageManagerRuntime) |
| `bun install` / `bun i` | Install deps (delegates to PackageManagerRuntime) |
| `bun add <pkg>` | Add package (delegates to PackageManagerRuntime) |
| `bun build` | Bundle (stub — native bun required for full execution) |
| `bun test` | Run test files (`*.test.ts`, `*.spec.ts`) |

### Execution Chain (for `bun run <file>`)

1. Check if argument has a file extension — if not, treat as npm script name
2. Resolve path (prepend `/` if relative)
3. Verify file exists in VFS

**Native execution** (in order of preference):
1. Try system `bun` binary — materializes VFS to temp dir and runs
2. Fallback to `boa_engine` — pure Rust JS interpreter, reads source from VFS
   - TypeScript files are first stripped of type annotations

**WASM execution**:
1. Preload VFS modules into `globalThis.__vfs_modules`
2. Strip TypeScript annotations if `.ts`/`.tsx`
3. Transform ESM imports -> `require()` calls
4. Wrap in polyfilled environment with:
   - `require()` function resolving from VFS
   - `module.exports` / `exports` support
   - Node.js polyfills: `http.createServer`, `process`, `path`, `fs` (partial)
   - `console.log/warn/error` capture
5. Execute via `js_sys::eval()`
6. Capture stdout/stderr from log arrays

### TypeScript Stripping

RunBox includes a built-in TypeScript-to-JavaScript transpiler that removes type annotations without a full parser. Coverage ~90%:

**Stripped constructs**:
- `interface Foo { ... }` declarations
- `type Foo = ...;` aliases
- `import type ...` statements
- `declare ...` blocks
- Inline type annotations: `const x: string = "hi"` -> `const x = "hi"`
- Generic parameters: `function foo<T>(x: T)` -> `function foo(x)`
- `as Type` assertions: `x as string` -> `x`
- `!` non-null assertions: `x!.prop` -> `x.prop`
- Access modifiers: `public`, `private`, `protected`, `readonly`, `abstract`, `override`
- `satisfies` expressions

**NOT stripped** (will cause runtime errors):
- Enums (only `const enum` patterns)
- Decorators (experimental)
- Complex mapped types in runtime positions

### Node.js Polyfills (WASM)

The WASM eval environment provides these Node.js API polyfills:

| Module | Available APIs |
|--------|---------------|
| `http` | `createServer(handler)` -> handler stored in `globalThis.__runbox_servers[port]` |
| `process` | `env`, `argv`, `version`, `platform`, `exit()`, `cwd()`, `stdout.write()`, `stderr.write()` |
| `path` | `join()`, `resolve()`, `extname()`, `basename()`, `dirname()` |
| `require()` | CJS module loader from `globalThis.__vfs_modules` map |

### `node` / `nodejs` / `tsx` / `ts-node` Shimming

These programs are automatically shimmed to `bun run`:

```
node index.js       -> bun run index.js
tsx src/server.ts   -> bun run src/server.ts
ts-node app.ts      -> bun run app.ts
```

---

## 8. Runtime Reference — Git

RunBox implements a complete in-memory Git system on top of the VFS. All Git state is stored under `/.git/`.

### Supported Subcommands

| Subcommand | Description |
|------------|-------------|
| `git init` | Initialize a new repository |
| `git add <files>` | Stage files (supports `.` and `--all`) |
| `git commit -m "msg"` | Create a commit from staged files |
| `git status` | Show working tree status |
| `git log` | Show commit history (supports `--oneline`) |
| `git diff` | Show unstaged changes (supports `--staged`/`--cached`) |
| `git branch [name]` | List/create/delete branches (supports `-d`/`-D`) |
| `git checkout <branch>` | Switch branches (supports `-b` for new branch) |
| `git merge <branch>` | Merge branch into current |
| `git reset [--soft|--hard]` | Reset HEAD to previous commit |
| `git clone <url>` | Clone remote repository (HTTP smart protocol) |
| `git fetch` | Fetch refs from remote |
| `git pull` | Fetch + merge |
| `git push` | Push commits to remote (HTTP smart protocol) |
| `git remote` | Manage remotes (`add`, `remove`, `-v`) |
| `git config` | Set config values (`user.name`, `user.email`, etc.) |

### Git Internal Storage

| Path | Content |
|------|---------|
| `/.git/HEAD` | `ref: refs/heads/main\n` |
| `/.git/refs/heads/<branch>` | Commit SHA |
| `/.git/index` | JSON-serialized staging area (`path -> SHA1`) |
| `/.git/log` | JSON array of commit objects |
| `/.git/config` | Git config text |
| `/.git/COMMIT_EDITMSG` | Last commit message |
| `/.git/remotes/<name>/url` | Remote URL |

### Git Commit Object

```json
{
  "sha": "abc1234...",
  "message": "feat: add login page",
  "author": "RunBox User <runbox@local>",
  "timestamp": "2024-01-15 10:30:00 +0000",
  "parent": "def5678...",
  "tree": { "/src/index.ts": "sha1_of_blob" }
}
```

### SHA1 Computation

- **Blob SHA**: `sha1("blob {length}\0{content}")`
- **Commit SHA**: `sha1("commit {length}\0{commit_content}")`

### Authentication for Push/Clone

For remote operations (push/clone/fetch), set a token first via `git_set_token()` (WASM method). Then set user identity via the RunBox `exec_command` tool to run the in-sandbox git config commands:

```json
{ "name": "exec_command", "arguments": { "command": "git config user.name \"Agent\"" } }
{ "name": "exec_command", "arguments": { "command": "git config user.email \"agent@runbox.dev\"" } }
```

### Push Protocol

RunBox implements HTTP smart protocol for `git push`:
1. Discovery: `GET {remote}/info/refs?service=git-receive-pack`
2. Parse remote refs to find current SHA
3. Build pack with commit data
4. Send: `POST {remote}/git-receive-pack`

---

## 9. Runtime Reference — Python

### Supported Commands

| Command | Description |
|---------|-------------|
| `python <file>` | Run a Python file |
| `python -c "code"` | Run inline Python code |
| `python3 <file>` | Same as `python` |
| `pip install <pkg>` | Install packages (simulated in VFS) |
| `pip install -r requirements.txt` | Install from requirements file |
| `pip list` | List installed packages |
| `pip show <pkg>` | Show package details |
| `pip freeze` | Output installed packages in requirements format |

### Execution Chain

**Native**:
1. Try system `python3`, then `python`
2. Materialize VFS to temp directory
3. Execute with system Python
4. Capture stdout/stderr

**WASM**:
- Python is NOT directly available
- Returns guidance: "In the browser build, Pyodide provides Python execution automatically"
- The host page must integrate Pyodide separately

### pip install behavior

```json
{ "name": "exec_command", "arguments": { "command": "pip install requests flask" } }
```

- Creates metadata files in `/site-packages/<name>-<version>.dist-info/METADATA`
- Tracks installed packages for `pip list`, `pip show`, `pip freeze`
- Does NOT actually download Python packages in WASM mode

### pip install -r requirements.txt

```json
{ "name": "exec_command", "arguments": { "command": "pip install -r requirements.txt" } }
```

- Reads the requirements file from VFS
- Filters comments and empty lines
- Reports count of packages installed

---

## 10. Runtime Reference — Shell Builtins

These commands execute directly against the VFS without spawning external processes.

| Command | Behavior |
|---------|----------|
| `echo <args>` | Concatenates args with spaces, appends newline |
| `pwd` | Always returns `/` (VFS root) |
| `ls [path]` | Lists directory entries (sorted), default `/` |
| `cat <file>` | Reads and outputs file content |
| `mkdir <path>` | Creates directory (via `.runbox_dir` placeholder) |
| `rm <path>` | Removes file or directory |
| `touch <file>` | Creates empty file if it doesn't exist |
| `cd` | No-op (VFS has no concept of current directory) |
| `cp` | NOT implemented — will error |
| `mv` | NOT implemented — will error |

**Important**: `cp` and `mv` are recognized as shell builtins but not implemented. They will return `"command not found"`. To copy a file, use `read_file` + `write_file`.

---

## 11. Virtual Filesystem (VFS)

### Data Model

```
VFS Root (/)
+-- Node::Dir(HashMap<String, Node>)
    +-- "src" -> Node::Dir(...)
    |   +-- "index.ts" -> Node::File(Vec<u8>)
    |   +-- "app.tsx" -> Node::File(Vec<u8>)
    +-- "package.json" -> Node::File(Vec<u8>)
    +-- ".git" -> Node::Dir(...)
```

- **All paths are absolute** (start with `/`)
- **Paths are normalized**: leading/trailing slashes are trimmed, empty segments ignored
- **No symlinks** — only files and directories
- **No permissions** — all files are readable/writable
- **Binary-safe** — files are `Vec<u8>`, not strings

### Change Tracking

Every write/remove operation (except under `/.git/`) is recorded:

```rust
struct FileChange {
    path: String,        // "/src/index.ts"
    kind: ChangeKind,    // Created | Modified | Deleted
}
```

- `drain_changes()` — returns all pending changes and clears the queue
- `peek_changes()` — returns pending changes without clearing
- Used by hot reload system to determine reload strategy

### Key Behaviors

1. **Auto-create directories**: `write("/a/b/c/file.txt", ...)` creates `/a/`, `/a/b/`, `/a/b/c/` automatically
2. **Overwrite detection**: Writing to an existing file records `Modified`; new file records `Created`
3. **Git exclusion**: Changes to `/.git/**` are NOT tracked (prevents git operations from triggering hot reload)

---

## 12. Console System

### Architecture

- **Circular buffer** with configurable capacity (default: 1000 entries)
- **Monotonically increasing IDs** — never reset, safe for incremental polling
- **WASM timestamps** use `js_sys::Date::now()`, native uses `SystemTime`
- **Process output** is automatically ingested as log entries

### Entry Structure

```json
{
  "id": 42,
  "level": "error",
  "message": "Cannot find module './utils'",
  "source": "pid:3",
  "pid": 3,
  "timestamp_ms": 5432
}
```

### Log Levels

| Level | Use Case |
|-------|----------|
| `log` | General output, stdout lines |
| `info` | Informational messages |
| `warn` | Warnings, non-fatal issues |
| `error` | Errors, stderr lines |
| `debug` | Debug/trace output |

### Convenience Methods

```
console.log(msg, src)    -> push(LogLevel::Log, msg, src, None)
console.info(msg, src)   -> push(LogLevel::Info, msg, src, None)
console.warn(msg, src)   -> push(LogLevel::Warn, msg, src, None)
console.error(msg, src)  -> push(LogLevel::Error, msg, src, None)
console.debug(msg, src)  -> push(LogLevel::Debug, msg, src, None)
```

### Process Output Ingestion

When a command runs, its stdout/stderr is automatically split by newlines and pushed:
- **stdout** lines -> `LogLevel::Log` with source `"pid:{pid}"`
- **stderr** lines -> `LogLevel::Error` with source `"pid:{pid}"`

---

## 13. Process Manager

### Process Lifecycle

```
spawn(command, args) -> PID
     |
     v
  Running --> exit(pid, code) -> Exited(code)
     |
     +---> kill(pid)         -> Killed
```

### PID Assignment

- PIDs start at 1 and increment
- PID 0 is never assigned (skipped on overflow via `wrapping_add`)
- PIDs are unique per RunboxInstance lifecycle

### Methods

| Method | Description |
|--------|-------------|
| `spawn(command, args) -> Pid` | Create new process, returns PID |
| `get(pid) -> &Process` | Get process by PID |
| `exit(pid, code)` | Mark process as exited with code |
| `kill(pid)` | Mark process as killed |
| `running() -> Vec<&Process>` | List running processes |
| `cleanup()` | Remove all terminated processes (free memory) |
| `count() -> usize` | Total tracked processes |

### Process Structure

```json
{
  "pid": 5,
  "command": "bun",
  "args": ["run", "index.ts"],
  "status": "Running",
  "stdout": [],
  "stderr": []
}
```

---

## 14. Hot Reload System

### How It Works

1. VFS change tracking records all file modifications
2. `hot_tick()` checks for pending changes within a debounce window (default: 80ms)
3. Changes are classified by file type -> reload strategy
4. The most "severe" strategy wins

### Reload Strategies (Priority Order)

| Strategy | Trigger Files | Behavior |
|----------|---------------|----------|
| `FullReload` | `.html`, `.htm`, `.toml`, `.yaml`, `.yml`, `.env`, `.config.*`, or any deletion | Full page reload |
| `Hmr` | `.js`, `.mjs`, `.jsx`, `.ts`, `.tsx` | Hot Module Replacement — update in place |
| `InjectCss` | `.css`, `.scss`, `.less`, `.sass` | Inject updated styles without reload |
| `None` | `.png`, `.jpg`, `.svg`, `.woff2`, `.ico`, etc. | No action needed |

### Classification Logic

```
File extension -> ReloadAction:
  .css, .scss, .less, .sass           -> InjectCss { paths }
  .js, .mjs, .jsx, .ts, .tsx          -> Hmr { paths }
  .html, .htm, .toml, .yaml, .yml,
  .env, .config.*, .json              -> FullReload
  File deletion (any type)             -> FullReload
  .png, .jpg, .jpeg, .gif, .svg,
  .woff, .woff2, .ttf, .ico, .mp4    -> None
```

### Debounce Behavior

- Changes accumulate in an 80ms window
- Multiple rapid file saves are batched into a single reload action
- The `hot_tick()` method should be called periodically by the host (e.g., in a `setInterval`)

### Using Hot Reload

```ts
// Poll for reload actions (call every ~100ms)
const action = JSON.parse(runbox.hot_tick());

switch (action.action) {
  case "InjectCss":
    // Re-fetch and apply CSS files listed in action.paths
    break;
  case "Hmr":
    // Hot-replace JS modules listed in action.paths
    break;
  case "FullReload":
    // window.location.reload()
    break;
  case "None":
    // No action needed
    break;
}
```

---

## 15. DOM Inspector

### Inspection Request Types

| Request | Parameters | Description |
|---------|------------|-------------|
| `AtPoint` | `{ x: f64, y: f64 }` | Find element at screen coordinates |
| `BySelector` | `{ selector: string }` | Find element by CSS selector |
| `ById` | `{ id: u64 }` | Find element by internal node ID |
| `Dismiss` | — | Clear selection |

### InspectedNode Structure

```json
{
  "node_id": 42,
  "tag": "div",
  "id": "app",
  "classes": ["container", "flex"],
  "attributes": { "data-testid": "main" },
  "box_model": {
    "content": { "x": 10, "y": 20, "width": 800, "height": 600 },
    "padding": { "top": 8, "right": 16, "bottom": 8, "left": 16 },
    "border": { "top": 1, "right": 1, "bottom": 1, "left": 1 },
    "margin": { "top": 0, "right": 0, "bottom": 0, "left": 0 }
  },
  "computed_styles": {
    "display": "flex",
    "color": "rgb(0, 0, 0)",
    "font-size": "16px"
  },
  "children": [
    { "node_id": 43, "tag": "h1" }
  ],
  "source_location": { "file": "/src/App.tsx", "line": 15 }
}
```

### Highlight Overlay

The inspector can render a visual overlay showing:
- **Content box** (blue)
- **Padding** (green)
- **Border** (yellow)
- **Margin** (orange)

### Usage Flow

```ts
// 1. Activate inspector
runbox.inspector_activate();

// 2. User clicks element -> send coordinates
const result = JSON.parse(
  runbox.inspector_request(JSON.stringify({ AtPoint: { x: 150, y: 200 } }))
);

// 3. Get selected node details
const node = JSON.parse(runbox.inspector_selected());

// 4. Get overlay data for rendering
const overlay = JSON.parse(runbox.inspector_overlay());

// 5. Deactivate when done
runbox.inspector_deactivate();
```

---

## 16. Terminal Integration

### Architecture

- Integrates with **xterm.js** on the frontend
- Manages **output buffer** (stdout/stderr chunks) and **input buffer** (user keystrokes)
- Configurable capacity (default circular buffer)
- Supports ANSI escape codes natively

### Data Types

**OutputChunk**:
```json
{ "stream": "stdout", "data": "Hello, world!\n" }
```

**InputChunk**:
```json
{ "data": "ls\n" }
```

### Terminal Methods

| Method | Description |
|--------|-------------|
| `write_stdout(data)` | Push data to stdout buffer |
| `write_stderr(data)` | Push data to stderr buffer |
| `output_drain() -> Vec<OutputChunk>` | Drain all output (for xterm rendering) |
| `input_push(data)` | Queue user input |
| `input_pop() -> Option<InputChunk>` | Consume next input chunk |
| `resize(cols, rows)` | Update terminal dimensions |
| `clear()` | Clear all buffers |
| `move_cursor(row, col)` | Send ANSI cursor move sequence |

### Usage Pattern

```ts
// Render loop (connect to xterm.js)
setInterval(() => {
  const chunks = JSON.parse(runbox.terminal_drain());
  for (const chunk of chunks) {
    xterm.write(chunk.data);
  }
}, 50);

// Handle user input
xterm.onData((data) => {
  runbox.terminal_input(data);
});

// Handle resize
xterm.onResize(({ cols, rows }) => {
  runbox.terminal_resize(cols, rows);
});
```

---

## 17. Network & Service Worker

### Service Worker Protocol

RunBox intercepts network requests from the sandbox iframe via Service Worker:

```
iframe fetch("http://localhost:3000/api/data")
  -> Service Worker (intercepts)
  -> postMessage to main thread
  -> RunBox.sw_handle_request(request)
  -> Checks VFS for static files
  -> Falls back to SPA routing (serves /index.html)
  -> Returns SwResponse
  -> Service Worker responds to iframe
```

### SwRequest / SwResponse

```json
// Request
{
  "id": "req-123",
  "method": "GET",
  "url": "http://localhost:3000/src/app.js",
  "headers": { "accept": "text/javascript" },
  "body": null
}

// Response
{
  "id": "req-123",
  "status": 200,
  "headers": { "content-type": "text/javascript" },
  "body": "console.log('hi');"
}
```

### Static File Serving

1. Extract path from URL: `http://localhost:3000/src/app.js` -> `/src/app.js`
2. Check VFS for file -> serve with correct MIME type
3. If not found and path has no extension -> try `/index.html` (SPA fallback)
4. If nothing found -> 404

### MIME Types

| Extension | Content-Type |
|-----------|-------------|
| `.html`, `.htm` | `text/html; charset=utf-8` |
| `.js`, `.mjs` | `text/javascript` |
| `.ts`, `.tsx` | `text/typescript` |
| `.css` | `text/css` |
| `.json` | `application/json` |
| `.svg` | `image/svg+xml` |
| `.png` | `image/png` |
| `.jpg`, `.jpeg` | `image/jpeg` |
| `.woff2` | `font/woff2` |
| _other_ | `application/octet-stream` |

### HTTP Server Detection (WASM)

When JS code calls `http.createServer()` and `.listen(port)`, the polyfill:
1. Stores the handler in `globalThis.__runbox_servers[port]`
2. Pushes `__RUNBOX_SERVER_READY__:{port}` to the log array
3. The host can then route SW requests to the handler

### Native HTTP Client

On native builds, RunBox has blocking HTTP via `reqwest`:
- `http_get(url) -> HttpResponse`
- `http_post(url, content_type, body) -> HttpResponse`

These are used for:
- npm registry resolution
- Git HTTP smart protocol (clone/fetch/push)

### VFS Materialization (Native)

For commands that need real filesystem access (system `bun`, system `python`), RunBox materializes the VFS to a temp directory:

```rust
materialize_vfs(vfs, tmp_dir_path) // writes all VFS files to disk
```

This allows system binaries to read/write files normally. The temp directory is cleaned up after command execution.

---

## 18. MCP Server (Model Context Protocol)

RunBox includes a built-in MCP server that exposes its capabilities via the standard Model Context Protocol (JSON-RPC 2.0 over stdio).

### Running the MCP Server

```bash
# As stdio server (default)
cargo run

# List tools and exit
cargo run -- --list-tools
```

### Client Configuration

**Claude Desktop** (`claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "runbox": {
      "command": "cargo",
      "args": ["run", "--manifest-path", "/path/to/runbox/Cargo.toml"]
    }
  }
}
```

**Cursor** (`.cursor/mcp.json`):
```json
{
  "mcpServers": {
    "runbox": {
      "command": "cargo",
      "args": ["run", "--manifest-path", "/path/to/runbox/Cargo.toml"]
    }
  }
}
```

### MCP Tools (8 total)

| Tool | Parameters | Description |
|------|------------|-------------|
| `exec` | `command: string` | Execute shell command |
| `read_file` | `path: string` | Read file from VFS |
| `write_file` | `path: string, content: string` | Write file to VFS |
| `list_dir` | `path: string` | List directory contents |
| `remove` | `path: string` | Remove file or directory |
| `search` | `query: string, path?: string, extension?: string` | Search text in files |
| `console_logs` | `level?: string, since_id?: number` | Get console logs |
| `process_list` | — | List running processes |

### MCP Resources

| URI | Description |
|-----|-------------|
| `runbox://console/logs` | All console entries as JSON |
| `runbox://process/list` | Active processes as JSON |
| `file:///{path}` | VFS files exposed as resources |

### MCP Prompts (3 total)

| Prompt | Arguments | Description |
|--------|-----------|-------------|
| `explain_file` | `path: string` | Generates prompt to explain file content |
| `fix_error` | `error: string` | Generates prompt to analyze and fix an error |
| `scaffold` | `description: string` | Generates prompt to scaffold a project |

### JSON-RPC Error Codes

| Code | Constant | Meaning |
|------|----------|---------|
| -32700 | PARSE_ERROR | Invalid JSON |
| -32600 | INVALID_REQUEST | Malformed request |
| -32601 | METHOD_NOT_FOUND | Unknown method |
| -32602 | INVALID_PARAMS | Invalid parameters |
| -32603 | INTERNAL_ERROR | Server error |

### Protocol Version

- JSON-RPC: `"2.0"`
- MCP: `"2024-11-05"`

---

## 19. MCP Client & Registry

RunBox can also act as an **MCP client**, connecting to external MCP servers.

### Server Configuration

```json
{
  "name": "filesystem",
  "transport": {
    "type": "stdio",
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
  },
  "env": {}
}
```

### Transport Types

| Type | Description | Native | WASM |
|------|-------------|--------|------|
| `stdio` | Spawn process, communicate via stdin/stdout | Full | Stub |
| `sse` | HTTP Server-Sent Events | Full | Stub |
| `websocket` | WebSocket connection | Full | Stub |
| `in-process` | Direct function call | Full | Full |

### Registry Methods

| Method | Description |
|--------|-------------|
| `add(config)` | Register and initialize MCP server |
| `remove(name)` | Disconnect and remove server |
| `list_servers()` | List all servers with connection state |
| `all_tools()` | Aggregate tools from all servers (namespaced as `server_name/tool_name`) |
| `all_resources()` | Aggregate resources from all servers |
| `call_tool(qualified_name, args)` | Route tool call to correct server |
| `read_resource(uri)` | Read resource from correct server |

### Connection States

| State | Description |
|-------|-------------|
| `Disconnected` | Not connected |
| `Connecting` | Handshake in progress |
| `Connected` | Ready to use |
| `Error(msg)` | Connection failed |

### Current Status

> **NOTE**: MCP client transport is currently **stub-only**. The `initialize()`, `call_tool()`, and `read_resource()` methods simulate the handshake and return placeholder responses. Full transport integration is pending. The protocol types and registry are fully implemented.

---

## 20. Provider-Specific Tool Formatting

RunBox can format its tool definitions for three AI providers:

### OpenAI Format

```ts
const tools = JSON.parse(runbox.ai_tools("openai"));
// Returns array of:
// { type: "function", function: { name, description, parameters: { type: "object", properties, required } } }
```

### Anthropic (Claude) Format

```ts
const tools = JSON.parse(runbox.ai_tools("anthropic"));
// Returns array of:
// { name, description, input_schema: { type: "object", properties, required } }
```

### Gemini Format

```ts
const tools = JSON.parse(runbox.ai_tools("gemini"));
// Returns:
// [{ functionDeclarations: [{ name, description, parameters: { type: "OBJECT", properties, required } }] }]
```

### Unknown Provider

Any other string returns OpenAI format as default.

---

## 21. Error Handling Patterns

### Error Types

RunBox uses a unified error enum:

| Variant | When |
|---------|------|
| `RunboxError::Vfs(msg)` | Filesystem operations fail (not found, wrong type) |
| `RunboxError::Shell(msg)` | Command parsing fails (empty command, no program after env vars) |
| `RunboxError::Runtime(msg)` | Runtime execution fails (unknown subcommand, network error) |
| `RunboxError::NotFound(path)` | File/resource not found |
| `RunboxError::Process(msg)` | PID not found |

### Agent Error Handling Pattern

```
1. Execute command/tool
2. Check result:
   - If ToolResult.error is set -> report error to user
   - If exec exit_code != 0 -> parse stderr for diagnostics
3. On error:
   a. Show the failing command
   b. Include stderr summary (first 5 lines)
   c. Propose direct correction
   d. Do NOT silently retry without reporting
```

### Common Error Scenarios & Recovery

| Scenario | Error | Recovery |
|----------|-------|----------|
| File not found | `"not found: /path"` | Check path with `list_dir`, verify spelling |
| Command unknown | `"unknown subcommand 'X'"` | Check supported subcommands in this document |
| Script not found | `"missing script: dev"` | Read `/package.json` to check available scripts |
| Empty command | `"empty command"` | Ensure command string is non-empty |
| Git not init | `"fatal: not a git repository"` | Run `git init` first |
| Package not found | `"not found: react@99.0.0"` | Check npm registry for correct version |
| Binary output | Returns `"[binary output]"` | File is not UTF-8; use raw bytes |

---

## 22. Decision Trees

### "I need to run a JavaScript/TypeScript file"

```
Is it a .ts/.tsx file?
+-- YES -> exec("bun run file.ts")
|          RunBox strips types -> runs via eval (WASM) or boa (native)
+-- NO (.js/.mjs) -> exec("bun run file.js")
                     RunBox runs directly
```

### "I need to install packages"

```
Does package.json exist?
+-- NO -> exec("npm init") first, then proceed
+-- YES -> Which package manager?
    +-- Lock file present?
    |   +-- bun.lock       -> exec("bun install")
    |   +-- pnpm-lock.yaml -> exec("pnpm install")
    |   +-- yarn.lock      -> exec("yarn install")
    |   +-- package-lock   -> exec("npm install")
    +-- No lock file -> exec("npm install") (default)

Adding specific packages:
    exec("npm add react react-dom")       # prod dependency
    exec("npm add -D vitest")             # dev dependency
    exec("npm add react@18.2.0 -E")      # exact version
```

### "I need to manage git"

```
Is repo initialized?
+-- NO -> exec("git init")
|         Then set user.name and user.email via in-sandbox git config
+-- YES -> What operation?
    +-- Stage files -> exec("git add .")
    +-- Commit -> exec("git commit -m \"message\"")
    +-- Check status -> exec("git status")
    +-- View history -> exec("git log --oneline")
    +-- Create branch -> exec("git branch feature-x")
    +-- Switch branch -> exec("git checkout feature-x")
    +-- New branch + switch -> exec("git checkout -b feature-x")
    +-- Merge -> exec("git merge feature-x")
    +-- Push -> set token first, then exec("git push")
```

### "I need to check what's in the project"

```
Quick overview -> ai_dispatch("get_file_tree", { path: "/", depth: 2 })
List specific dir -> ai_dispatch("list_dir", { path: "/src" })
Read a file -> ai_dispatch("read_file", { path: "/package.json" })
Search for code -> ai_dispatch("search_code", { query: "useState", path: "/src", extension: ".tsx" })
Check console -> ai_dispatch("get_console_logs", { level: "error" })
```

### "I need to handle hot reload"

```
What changed?
+-- CSS only -> InjectCss (no page reload needed)
+-- JS/TS files -> Hmr (hot module replacement)
+-- HTML/config -> FullReload (full page reload)
+-- Asset files -> None (no action needed)
+-- File deleted -> FullReload (always)
```

---

## 23. Advanced Workflows

### Workflow 1: Full Project Setup

```json
// 1. Initialize project
{ "name": "exec_command", "arguments": { "command": "npm init" } }

// 2. Add dependencies
{ "name": "exec_command", "arguments": { "command": "npm add react react-dom" } }
{ "name": "exec_command", "arguments": { "command": "npm add -D typescript @types/react" } }

// 3. Create entry file
{ "name": "write_file", "arguments": {
  "path": "/src/index.tsx",
  "content": "import React from 'react';\nimport ReactDOM from 'react-dom/client';\n\nfunction App() {\n  return <h1>Hello RunBox</h1>;\n}\n\nReactDOM.createRoot(document.getElementById('root')!).render(<App />);\n"
} }

// 4. Create HTML
{ "name": "write_file", "arguments": {
  "path": "/index.html",
  "content": "<!DOCTYPE html>\n<html>\n<head><title>RunBox App</title></head>\n<body>\n<div id=\"root\"></div>\n<script type=\"module\" src=\"/src/index.tsx\"></script>\n</body>\n</html>"
} }

// 5. Initialize git
{ "name": "exec_command", "arguments": { "command": "git init" } }
{ "name": "exec_command", "arguments": { "command": "git add ." } }
{ "name": "exec_command", "arguments": { "command": "git commit -m \"feat: initial project setup\"" } }
```

### Workflow 2: Debug Build Failure

```json
// 1. Run the build
{ "name": "exec_command", "arguments": { "command": "npm run build" } }

// 2. If exit_code != 0, check stderr
// 3. Read the failing file
{ "name": "read_file", "arguments": { "path": "/src/app.ts" } }

// 4. Search for related patterns
{ "name": "search_code", "arguments": { "query": "import.*missing", "path": "/src" } }

// 5. Fix the issue
{ "name": "write_file", "arguments": { "path": "/src/app.ts", "content": "...fixed..." } }

// 6. Retry
{ "name": "exec_command", "arguments": { "command": "npm run build" } }

// 7. Verify console
{ "name": "get_console_logs", "arguments": { "level": "error" } }
```

### Workflow 3: Python Project

```json
// 1. Create requirements.txt
{ "name": "write_file", "arguments": {
  "path": "/requirements.txt",
  "content": "flask==3.0.0\nrequests==2.31.0\npython-dotenv==1.0.0"
} }

// 2. Install dependencies
{ "name": "exec_command", "arguments": { "command": "pip install -r requirements.txt" } }

// 3. Verify installation
{ "name": "exec_command", "arguments": { "command": "pip list" } }

// 4. Create app
{ "name": "write_file", "arguments": {
  "path": "/app.py",
  "content": "from flask import Flask\napp = Flask(__name__)\n\n@app.route('/')\ndef hello():\n    return 'Hello from RunBox!'\n\nif __name__ == '__main__':\n    app.run(port=5000)\n"
} }

// 5. Run (native only -- WASM needs Pyodide host)
{ "name": "exec_command", "arguments": { "command": "python app.py" } }
```

### Workflow 4: Git Branching Workflow

```json
// 1. Check current status
{ "name": "exec_command", "arguments": { "command": "git status" } }

// 2. Create feature branch
{ "name": "exec_command", "arguments": { "command": "git checkout -b feature/login-page" } }

// 3. Make changes
{ "name": "write_file", "arguments": { "path": "/src/login.tsx", "content": "..." } }

// 4. Stage and commit
{ "name": "exec_command", "arguments": { "command": "git add ." } }
{ "name": "exec_command", "arguments": { "command": "git commit -m \"feat: add login page\"" } }

// 5. Switch back to main
{ "name": "exec_command", "arguments": { "command": "git checkout main" } }

// 6. Merge feature
{ "name": "exec_command", "arguments": { "command": "git merge feature/login-page" } }

// 7. View log
{ "name": "exec_command", "arguments": { "command": "git log --oneline" } }
```

### Workflow 5: Incremental Console Monitoring

```json
// 1. Get initial state
{ "name": "get_console_logs", "arguments": {} }
// Note the highest ID returned, e.g., 15

// 2. Run something
{ "name": "exec_command", "arguments": { "command": "bun run server.ts" } }

// 3. Check only new logs
{ "name": "get_console_logs", "arguments": { "since_id": 15 } }
// Returns only entries with id > 15

// 4. Filter for errors only
{ "name": "get_console_logs", "arguments": { "level": "error", "since_id": 15 } }
```

### Workflow 6: Using the MCP Server

```json
// From an MCP client (e.g., Claude Desktop), send JSON-RPC:

// List tools
{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}

// Execute command
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
  "name":"exec",
  "arguments":{"command":"npm install"}
}}

// Read file
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
  "name":"read_file",
  "arguments":{"path":"/package.json"}
}}

// List resources
{"jsonrpc":"2.0","id":4,"method":"resources/list","params":{}}

// Read resource
{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{
  "uri":"runbox://console/logs"
}}

// Get prompt
{"jsonrpc":"2.0","id":6,"method":"prompts/get","params":{
  "name":"fix_error",
  "arguments":{"error":"TypeError: x is not a function"}
}}
```

---

## 24. Security Practices

1. **Never log secrets**: `git_set_token()` values must never appear in console output or tool results
2. **VFS isolation**: The virtual filesystem is completely isolated from the host filesystem (except during temp materialization on native)
3. **No real shell**: Commands go through RunBox's runtime router, NOT a real system shell — prevents injection
4. **Temp directory cleanup**: When materializing VFS for system binary execution, temp directories are scoped and cleaned up automatically via `TempDir`
5. **Git credentials**: Stored in VFS at `/.git/credentials` — excluded from change tracking. Use HTTP Basic Auth header for remote operations
6. **Binary content safety**: `body_str()` and `stdout_str()` return `"[binary output]"` / `"[binary content]"` instead of panicking on non-UTF-8
7. **Tarball size limit**: npm tarballs skip files larger than 2MB to prevent memory exhaustion
8. **PID overflow protection**: PIDs use `wrapping_add` and skip 0 to prevent undefined behavior
9. **Request ID overflow protection**: MCP client request IDs use `wrapping_add` and skip 0

---

## 25. Performance Considerations

1. **VFS write optimization**: Change detection reuses the same tree traversal as the write operation (no double traversal)
2. **Console circular buffer**: Old entries are evicted when capacity is reached (default 1000) — prevents unbounded memory growth
3. **Process cleanup**: Call `ProcessManager::cleanup()` periodically to remove terminated processes and free memory
4. **Search limits**: `search_code` is capped at 200 matches to prevent output overflow
5. **Tarball extraction**: Files > 2MB are skipped during npm install to keep VFS lean
6. **Hot reload debouncing**: 80ms debounce window batches rapid file changes into a single reload action
7. **WASM module preloading**: npm packages are loaded per-package (isolated `eval()` calls) so one broken package doesn't prevent others from loading
8. **Release profile**: `opt-level = "z"` (size), LTO enabled, single codegen unit, panic = abort — optimized for minimal WASM binary size

---

## 26. Constraints & Limitations

| Area | Constraint |
|------|-----------|
| **Filesystem** | Entirely in-memory; no persistence across page reloads (unless serialized by host) |
| **No real shell** | Commands go through RunBox's router, not a real POSIX shell. No pipes, no redirects (`>`), no backgrounding (`&`) |
| **Package managers** | Simulated behavior — lockfiles are generated but dependency resolution is simplified |
| **Python (WASM)** | Not directly available; host must integrate Pyodide |
| **Curl/wget** | Recognized but NOT implemented (returns error) |
| **cp / mv** | Recognized as shell builtins but NOT implemented |
| **TypeScript** | ~90% coverage; complex patterns (enums, decorators, mapped types) may fail |
| **MCP Client transport** | Currently stubbed — protocol and registry are complete but transport isn't wired |
| **Network (WASM)** | No direct HTTP — must use Service Worker bridge |
| **File size** | npm tarball files > 2MB are silently skipped |
| **Console capacity** | Circular buffer (default 1000) — old entries are lost |
| **Git merge** | No three-way merge; only fast-forward is supported |
| **Git clone (WASM)** | HTTP smart protocol needs Service Worker or host-bridged fetch |
| **No symlinks** | VFS does not support symbolic links |
| **No file permissions** | All files are readable/writable |
| **cd command** | No-op — VFS has no current directory concept; all paths are absolute |

---

## 27. Troubleshooting Guide

### "empty command"
**Cause**: Passed an empty or whitespace-only string to `exec_command`.
**Fix**: Ensure the command string is non-empty.

### "unknown subcommand 'X'"
**Cause**: Ran a subcommand not supported by the runtime.
**Fix**: Check the supported subcommands listed in this document for each runtime.

### "not found: /path"
**Cause**: File doesn't exist in VFS.
**Fix**: Use `list_dir` to check what exists. Verify path spelling. Remember all paths must be absolute (start with `/`).

### "/path is a directory"
**Cause**: Tried to `read_file` on a directory.
**Fix**: Use `list_dir` instead. Ensure path points to a file, not a directory.

### "fatal: not a git repository"
**Cause**: Git command used before `git init`.
**Fix**: Run `exec("git init")` first.

### "missing script: X"
**Cause**: Script not defined in `package.json`.
**Fix**: Read `/package.json` and check the `scripts` object. Add the missing script with `write_file`.

### "bun: could not execute 'X'"
**Cause**: File not found in VFS, or neither system bun nor boa_engine could run it.
**Fix**: Verify file exists with `file_exists` or `list_dir`. Ensure file has `.js` or `.ts` extension.

### "pip: unknown subcommand 'X'"
**Cause**: Unsupported pip subcommand.
**Fix**: Only `install`, `list`, `show`, `freeze` are supported.

### "Python 3.x (RunBox) Type -c 'code' to run inline."
**Cause**: Ran `python` without arguments.
**Fix**: Provide a file path or use `-c` flag: `python -c "print('hello')"`.

### "network not available in WASM"
**Cause**: Tried to make HTTP request from WASM build.
**Fix**: In WASM, HTTP must go through the Service Worker bridge. Use `sw_handle_request` instead.

### Console entries disappearing
**Cause**: Circular buffer overflow (default: 1000 entries).
**Fix**: Use `since_id` for incremental reads. Increase capacity if needed at creation time.

### Hot reload not triggering
**Cause**: Files under `/.git/` don't trigger change tracking. Or debounce window hasn't elapsed.
**Fix**: Ensure you're modifying files outside `/.git/`. Call `hot_tick()` periodically (every ~100ms). Use `hot_flush()` to force immediate processing.

### Process count growing indefinitely
**Cause**: Terminated processes accumulate in the ProcessManager.
**Fix**: Call `ProcessManager::cleanup()` periodically to remove terminated processes.

### Git push fails
**Cause**: No remote configured or no authentication token set.
**Fix**:
1. Add remote: `git remote add origin https://github.com/user/repo.git`
2. Set token: `git_set_token("ghp_...")` (WASM) or via git credential config
3. Retry push

### TypeScript stripping fails
**Cause**: Complex TS patterns not covered by the simple stripper.
**Fix**: Simplify the TypeScript code. Avoid: runtime enums, decorators, complex mapped types. Use `interface` instead of runtime constructs where possible.

---

## Quick Reference Card

```
+-----------------------------------------------------+
|              RunBox Agent Quick Reference            |
+-----------------------------------------------------+
| TOOLS (ai_dispatch):                                 |
|   read_file     { path }                             |
|   write_file    { path, content }                    |
|   list_dir      { path }                             |
|   exec_command  { command }                          |
|   search_code   { query, path?, extension? }         |
|   get_console_logs { level?, since_id? }             |
|   reload_sandbox   { hard? }                         |
|   install_packages { packages?, dev?, manager? }     |
|   get_file_tree    { path?, depth? }                 |
+-----------------------------------------------------+
| RUNTIMES:                                            |
|   bun/node/tsx/ts-node  -> BunRuntime                |
|   npm/npx               -> PackageManagerRuntime     |
|   pnpm/pnpx             -> PackageManagerRuntime     |
|   yarn                  -> PackageManagerRuntime     |
|   git                   -> GitRuntime                |
|   python/python3/pip    -> PythonRuntime             |
|   cd/ls/echo/cat/...    -> ShellBuiltins             |
+-----------------------------------------------------+
| PROVIDERS:                                           |
|   ai_tools("openai")    -> OpenAI function format    |
|   ai_tools("anthropic") -> Claude tool format        |
|   ai_tools("gemini")    -> Gemini function format    |
+-----------------------------------------------------+
| MCP (native only):                                   |
|   8 tools: exec, read_file, write_file, list_dir,   |
|            remove, search, console_logs, process_list|
|   3 prompts: explain_file, fix_error, scaffold       |
|   Resources: runbox://console/logs, file:///...      |
+-----------------------------------------------------+
```
