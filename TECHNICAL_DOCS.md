# RunboxJS Technical Documentation

This document describes the internal architecture of `runbox` (Rust core) and the browser-facing WASM API.

## 1. Architecture Overview

RunboxJS is split into layered modules:

1. `vfs`:
- In-memory virtual filesystem
- path-based read/write/list/remove
- change tracking for hot reload

2. `shell`:
- command parser and runtime target detection
- maps command program names to runtime implementations

3. `runtime/*`:
- language and tooling runtimes (`bun`, `npm/pnpm/yarn`, `git`, `python`, shell builtins)

4. `process`:
- lightweight process registry and lifecycle metadata

5. `console` and `terminal`:
- structured logs and terminal chunk streams

6. `hotreload` and `inspector`:
- browser-facing reload actions and DOM inspect session data

7. `ai`:
- tool schemas + skill dispatch for assistant workflows

8. `wasm`:
- wasm-bindgen wrapper exposing `RunboxInstance`

## 2. Command Routing

Command flow:

1. `RunboxInstance::exec(line)` parses line into `shell::Command`
2. `RuntimeTarget::detect` decides runtime by `program`
3. runtime executes and returns `ExecOutput { stdout, stderr, exit_code }`
4. output is ingested into console and returned as JSON string

Current routing map:

- Bun target: `bun`, `node`, `nodejs`, `tsx`, `ts-node`
- Python target: `python`, `python3`, `pip`, `pip3`
- Package managers: `npm`, `npx`, `pnpm`, `pnpx`, `yarn`
- Git target: `git`
- Shell builtins: `cd`, `ls`, `echo`, `cat`, `pwd`, `mkdir`, `rm`, `cp`, `mv`, `touch`

## 3. Package Manager Runtime

`runtime/npm.rs` handles npm/pnpm/yarn/bun package operations.

Supported command families:

- `install`, `i`, `ci`
- `add`
- `remove`, `uninstall`, `rm`, `un`
- `run`
- `exec`, `dlx`, `npx`, `pnpx`, `create`
- `init`
- `list`, `ls`
- `update`, `upgrade`
- `outdated`
- `audit`

Lockfile generation by manager:

- npm -> `/package-lock.json`
- pnpm -> `/pnpm-lock.yaml`
- yarn -> `/yarn.lock`
- bun -> `/bun.lock`

WASM/npm install strategy:

- `npm_packages_needed()` reports missing deps
- host fetches tarballs from npm registry
- `npm_process_tarball()` installs extracted package contents into VFS

## 4. Git Runtime

`runtime/git.rs` supports in-memory git workflows:

- local workflows: `init`, `add`, `commit`, `status`, `log`, `diff`, `branch`, `checkout`, `merge`, `reset`
- remote workflows: `remote`, `clone`, `fetch`, `pull`, `push`
- configuration: `git config ...` mapped to VFS credentials storage

Credential helpers:

- `git_set_user(name, email)`
- `git_set_token(token)`

## 5. Python Runtime

`runtime/python.rs` behavior:

- native target: tries system `python3` then `python`
- wasm target: returns guidance indicating browser adapters should provide Pyodide execution
- pip emulation supports: `install`, `list`, `show`, `freeze`

## 6. AI Tooling

Tool schemas are declared in `ai/tools.rs` and dispatched by `ai/skills.rs`.

Current skill names:

- `read_file`
- `write_file`
- `list_dir`
- `exec_command`
- `search_code`
- `get_console_logs`
- `reload_sandbox`
- `install_packages`
- `get_file_tree`

Integration surface:

- `ai_tools(provider)` returns provider-specific tool schema format
- `ai_dispatch(call_json)` executes tool calls and returns `{ name, content, error }`

## 7. WASM API Surface

`RunboxInstance` is defined in `wasm.rs` and exported via wasm-bindgen.

Primary method groups:

- filesystem
- command execution
- package tarball install
- git credentials
- console/terminal
- hot reload
- inspector
- sandbox bridge
- HTTP/SW bridge
- AI tools dispatch

## 8. Browser Integration Notes

Typical host responsibilities:

1. initialize wasm (`await init()`)
2. maintain editor + VFS synchronization
3. render command outputs and terminal streams
4. poll `terminal_drain()`
5. call `hot_tick(performance.now())` after write operations
6. wire service worker/preview networking to `sw_handle_request` and `http_handle_request`
7. wire DOM inspector events into `inspector_set_node`

## 9. Development and Verification

Core checks:

```bash
cargo check
cargo test
cargo bench
```

WASM package build:

```bash
node build.mjs
```

Publish flow:

```bash
node build.mjs --bump patch
node build.mjs --publish
```
