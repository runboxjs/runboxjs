# RunboxJS

[![npm](https://img.shields.io/npm/v/runboxjs)](https://www.npmjs.com/package/runboxjs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust Edition](https://img.shields.io/badge/Rust-2024-orange)](https://www.rust-lang.org/)

**RunboxJS** is a WebAssembly sandbox runtime that executes project workflows directly in the browser. It provides a complete development environment simulation including a virtual filesystem, multi-runtime command execution (Bun/Node/npm/pnpm/Yarn/Git/Python/pip), terminal I/O streams, hot reload signaling, a DOM inspector bridge, an MCP (Model Context Protocol) server, and AI tool dispatch for assistant orchestration.

- **npm package:** [`runboxjs`](https://www.npmjs.com/package/runboxjs)
- **Rust crate source:** `runbox`
- **Current version:** `0.3.8`

---

## Table of Contents

- [Why RunboxJS](#why-runboxjs)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Vite Integration](#vite-integration)
- [Runtime Command Support](#runtime-command-support)
- [API Overview](#api-overview)
- [MCP Server](#mcp-server)
- [AI Integration](#ai-integration)
- [Agent Journal](#agent-journal)
- [Local Development](#local-development)
- [Publishing](#publishing)
- [Troubleshooting](#troubleshooting)
- [Documentation Index](#documentation-index)
- [License](#license)

---

## Why RunboxJS

RunboxJS was designed to bring a full development sandbox experience into the browser, without requiring any server-side infrastructure or host filesystem access:

- **Isolated execution** in browser memory via WebAssembly -- no host filesystem access, no network side effects
- **Virtual Filesystem (VFS)** with in-memory file/directory tree, change tracking, and hot-reload integration
- **Multi-runtime command execution** with shell-style routing to Bun, Node.js, npm, pnpm, Yarn, Git, Python, and pip
- **Package manager simulation** with real `package.json` manipulation and lockfile generation (npm, pnpm, Yarn, Bun)
- **Git workflow simulation** entirely in memory -- init, add, commit, branch, checkout, merge, push, pull, and more
- **Python/pip workflow simulation** with native fallback on non-WASM targets and Pyodide bridging in browsers
- **Hot reload engine** with intelligent strategies: CSS injection, HMR, or full reload based on file type
- **DOM Inspector bridge** for element inspection with box model, computed styles, and highlight overlays
- **Terminal streams** compatible with xterm.js for bidirectional I/O
- **AI tool bridge** (`ai_tools` + `ai_dispatch`) for seamless assistant orchestration with OpenAI, Anthropic, and Gemini
- **MCP Server** (Model Context Protocol) exposing VFS, shell, and console as tools/resources for Claude Desktop, Cursor, Zed, and Continue
- **Service Worker bridge** for intercepting network requests and serving files from the VFS
- **HTTP server simulation** via `globalThis.__runbox_servers` for Express/http-style request handling

---

## Installation

```bash
npm install runboxjs
```

Or with other package managers:

```bash
pnpm add runboxjs
yarn add runboxjs
bun add runboxjs
```

---

## Quick Start

```ts
import init, { RunboxInstance } from 'runboxjs';

// Initialize the WASM module (must be awaited before any API calls)
await init();

// Create a sandbox instance
const runbox = new RunboxInstance();

// Write a file to the virtual filesystem
runbox.write_file('/index.js', new TextEncoder().encode("console.log('Hello from RunboxJS!');"));

// Execute the file
const result = JSON.parse(runbox.exec('node /index.js'));
console.log(result.stdout); // "Hello from RunboxJS!"

// Install packages
runbox.exec('npm init -y');
runbox.exec('npm add express');

// Git operations
runbox.exec('git init');
runbox.exec('git add .');
runbox.exec('git commit -m "initial commit"');
```

### Browser-side npm install flow (WASM)

In WASM, direct HTTP requests are not possible from synchronous Rust code. The install flow uses a two-step bridge:

```js
async function npmInstall(runbox) {
  // 1. Ask RunboxJS which packages are missing
  const needed = JSON.parse(runbox.npm_packages_needed());

  // 2. Fetch each tarball from the registry and feed it to RunboxJS
  for (const { name, version } of needed) {
    const meta = await fetch(`https://registry.npmjs.org/${name}/${version}`).then(r => r.json());
    const buf = await fetch(meta.dist.tarball).then(r => r.arrayBuffer());
    runbox.npm_process_tarball(name, version, new Uint8Array(buf));
  }
}
```

---

## Vite Integration

`runboxjs` is built with `wasm-pack --target web`, so Vite can bundle it directly with zero extra configuration:

```bash
npm install runboxjs
```

```ts
import init, { RunboxInstance } from 'runboxjs';
```

No `vite-plugin-wasm` is required for standard client-side Vite apps. See [WASM_SETUP.md](./WASM_SETUP.md) for edge cases and troubleshooting.

---

## Runtime Command Support

`runbox.exec(line)` parses and routes commands by program name to the appropriate runtime:

| Category | Commands | Description |
|---|---|---|
| **JS Runtime** | `bun`, `node`, `nodejs`, `tsx`, `ts-node` | JavaScript/TypeScript execution via Bun runtime |
| **Package Managers** | `npm`, `npx`, `pnpm`, `pnpx`, `yarn` | Package install, add, remove, run, exec, init, list, update, audit |
| **Git** | `git` | In-memory git operations (init, add, commit, status, log, diff, branch, checkout, merge, reset, clone, fetch, pull, push, remote, config) |
| **Python** | `python`, `python3`, `pip`, `pip3` | Python execution and pip package management |
| **Shell Builtins** | `echo`, `ls`, `cat`, `pwd`, `mkdir`, `rm`, `touch` | Basic filesystem operations |

All commands return a JSON string:

```json
{
  "stdout": "...",
  "stderr": "...",
  "exit_code": 0
}
```

---

## API Overview

All methods belong to `RunboxInstance`. For complete details, parameters, return types, and examples, see [docs/API_REFERENCE.md](./docs/API_REFERENCE.md).

| Category | Key Methods |
|---|---|
| **Filesystem** | `write_file`, `read_file`, `list_dir`, `file_exists`, `remove_file` |
| **Command Execution** | `exec` |
| **npm WASM Install** | `npm_packages_needed`, `npm_process_tarball` |
| **Git Credentials** | `git_set_user`, `git_set_token` |
| **Console** | `console_push`, `console_all`, `console_since`, `console_clear` |
| **Terminal** | `terminal_input`, `terminal_drain`, `terminal_resize`, `terminal_size`, `terminal_clear` |
| **Hot Reload** | `hot_tick`, `hot_flush` |
| **Inspector** | `inspector_activate`, `inspector_deactivate`, `inspector_is_active`, `inspector_set_node`, `inspector_selected`, `inspector_overlay`, `inspector_history`, `inspector_request` |
| **Sandbox Bridge** | `sandbox_command`, `sandbox_event` |
| **HTTP/SW Bridge** | `http_handle_request`, `sw_handle_request` |
| **AI Tooling** | `ai_tools`, `ai_dispatch` |
| **Agent Journal** | `journal_entries`, `journal_since` |

---

## MCP Server

RunboxJS includes a full [Model Context Protocol](https://spec.modelcontextprotocol.io) server that exposes its capabilities to AI assistants. It supports the JSON-RPC 2.0 protocol over stdio, HTTP/SSE, and WebSocket transports.

### Supported MCP Clients

- Claude Desktop
- Cursor
- Zed
- Continue
- Any MCP-compatible client

### Quick Setup (Claude Desktop)

Add to your Claude Desktop `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "runbox": {
      "command": "runbox",
      "args": []
    }
  }
}
```

For detailed MCP setup, tools, resources, and prompts, see [docs/MCP_GUIDE.md](./docs/MCP_GUIDE.md).

---

## AI Integration

RunboxJS provides a built-in AI tool bridge compatible with OpenAI, Anthropic, and Gemini function calling formats:

```ts
// Get tool definitions for your AI provider
const tools = JSON.parse(runbox.ai_tools('openai')); // or 'anthropic', 'gemini'

// Dispatch a tool call from the AI
const result = JSON.parse(runbox.ai_dispatch(JSON.stringify({
  name: 'exec_command',
  arguments: { command: 'npm run build' }
})));
```

Available AI tool names: `read_file`, `write_file`, `list_dir`, `exec_command`, `search_code`, `get_console_logs`, `reload_sandbox`, `install_packages`, `get_file_tree`, `agent_memo`, `get_agent_journal`.

For assistant integration guidance, see [skills/AGENT_SKILL.md](./skills/AGENT_SKILL.md).

---

## Agent Journal

RunboxJS includes a structured **Agent Journal** — a reasoning log that records *why* a change happened, not just *what* changed. This allows any agent (or developer) to resume work with full decision context.

### How it works

Agents call `agent_memo` before acting to record their intent. The journal is stored in `RunboxInstance` and queryable at any time.

```ts
// Record intent before making a change
await runbox.ai_dispatch(JSON.stringify({
  name: 'agent_memo',
  arguments: {
    tool: 'write_file',
    reason: 'Extracting auth logic into a hook to avoid duplication across 3 components',
    files_affected: ['/src/hooks/useAuth.ts'],
  }
}));

// Then make the change
await runbox.ai_dispatch(JSON.stringify({
  name: 'write_file',
  arguments: { path: '/src/hooks/useAuth.ts', content: '...' }
}));

// A future agent reads the journal to understand context
const journal = JSON.parse(runbox.journal_entries());
// Or incrementally:
const recent = JSON.parse(runbox.journal_since(lastSeenId));
```

### Journal entry shape

```json
{
  "id": 0,
  "timestamp_ms": 1234,
  "tool": "write_file",
  "reason": "Extracting auth logic into a hook to avoid duplication",
  "result_summary": "",
  "files_affected": ["/src/hooks/useAuth.ts"]
}
```

### AI tool: `get_agent_journal`

```ts
// All entries
runbox.ai_dispatch(JSON.stringify({ name: 'get_agent_journal', arguments: {} }));

// Since a cursor
runbox.ai_dispatch(JSON.stringify({ name: 'get_agent_journal', arguments: { since_id: 4 } }));

// Filter by file
runbox.ai_dispatch(JSON.stringify({ name: 'get_agent_journal', arguments: { file: '/src/auth.ts' } }));
```

---

## Local Development

```bash
# Rust checks and tests
cargo check
cargo test
cargo bench

# WASM package build (writes pkg/package.json from template)
node build.mjs

# Bump version and build
node build.mjs --bump patch
```

For detailed build instructions, project setup, and contribution guidelines, see [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md).

---

## Publishing

Use the scripted flow (recommended):

```bash
node build.mjs --bump patch
node build.mjs --publish
```

Detailed publishing notes: [NPM_PUBLISH.md](./NPM_PUBLISH.md)

---

## Troubleshooting

### "crate-type must be cdylib"

`Cargo.toml` must include:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

### Vite WASM import errors

RunboxJS no longer requires `vite-plugin-wasm`. If you hit a WASM error, clear caches and ensure your app imports `runboxjs` from the published package entry.

### Missing module after package install

Confirm both:
1. The dependency exists in `/package.json`
2. The corresponding package exists under `/node_modules/<name>/package.json`

### Python message "python3 not found"

Expected in environments without native Python. In browser WASM flows, host adapters should provide Pyodide integration.

### MCP Server not responding

1. Ensure the `runbox` binary is in your `PATH`
2. Check stderr for log output (set `RUNBOX_LOG=debug` for verbose logging)
3. Verify your MCP client configuration matches the transport type

For more troubleshooting, see [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md#troubleshooting).

---

## Documentation Index

| Document | Description |
|---|---|
| [CHANGELOG.md](./CHANGELOG.md) | Release history and notable changes |
| [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) | Internal architecture, module structure, data flow, and design decisions |
| [docs/API_REFERENCE.md](./docs/API_REFERENCE.md) | Complete API reference with all methods, parameters, return types, and examples |
| [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md) | Local development setup, building, testing, and contributing |
| [docs/MCP_GUIDE.md](./docs/MCP_GUIDE.md) | MCP server setup, tools, resources, prompts, and client integration |
| [TECHNICAL_DOCS.md](./TECHNICAL_DOCS.md) | Legacy technical documentation |
| [WASM_SETUP.md](./WASM_SETUP.md) | WASM setup guide for Vite integration |
| [NPM_PUBLISH.md](./NPM_PUBLISH.md) | npm publishing guide |
| [skills/AGENT_SKILL.md](./skills/AGENT_SKILL.md) | AI assistant integration skill guide |

---

## License

MIT
