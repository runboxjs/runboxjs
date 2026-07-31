# RunboxJS MCP Server Guide

This guide covers setup, configuration, and usage of the RunboxJS MCP (Model Context Protocol) server. The MCP server exposes RunboxJS capabilities (virtual filesystem, shell, console, inspector) to AI assistants via the standardized JSON-RPC 2.0 protocol.

---

## Table of Contents

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Client Configuration](#client-configuration)
  - [Claude Desktop](#claude-desktop)
  - [Cursor](#cursor)
  - [Zed](#zed)
  - [Continue](#continue)
  - [Custom MCP Clients](#custom-mcp-clients)
- [Server Architecture](#server-architecture)
- [Transport Types](#transport-types)
  - [Stdio (Default)](#stdio-default)
  - [HTTP/SSE](#httpsse)
  - [WebSocket](#websocket)
  - [In-Process](#in-process)
- [Available Tools](#available-tools)
  - [exec](#exec)
  - [read_file](#read_file)
  - [write_file](#write_file)
  - [list_dir](#list_dir)
  - [remove](#remove)
  - [search](#search)
  - [console_logs](#console_logs)
  - [process_list](#process_list)
- [Resources](#resources)
  - [Console Logs](#console-logs-resource)
  - [Process List](#process-list-resource)
  - [VFS Files](#vfs-files)
- [Prompts](#prompts)
  - [explain_file](#explain_file)
  - [fix_error](#fix_error)
  - [scaffold](#scaffold)
- [MCP Client (Outbound)](#mcp-client-outbound)
  - [Registry](#registry)
  - [Connecting External Servers](#connecting-external-servers)
  - [Namespaced Tool Calls](#namespaced-tool-calls)
- [Protocol Reference](#protocol-reference)
  - [JSON-RPC 2.0](#json-rpc-20)
  - [Initialization Handshake](#initialization-handshake)
  - [Error Codes](#error-codes)
- [Logging and Debugging](#logging-and-debugging)
- [Examples](#examples)

---

## Overview

The RunboxJS MCP server implements the [Model Context Protocol specification](https://spec.modelcontextprotocol.io) (version `2024-11-05`), enabling any MCP-compatible AI client to:

- **Execute commands** in the RunboxJS sandbox (bun, npm, git, python, shell builtins)
- **Read and write files** in the virtual filesystem
- **Search code** across the project
- **Access console logs** from command execution
- **List running processes**
- **Use prompt templates** for common tasks (explain files, fix errors, scaffold projects)

The server exposes three MCP capability categories:
- **Tools** -- executable actions (8 tools)
- **Resources** -- readable data sources (console, processes, VFS files)
- **Prompts** -- reusable prompt templates (3 prompts)

---

## Quick Start

### Building the native binary

```bash
cargo build --release
# Binary at: target/release/runbox
```

### Running the MCP server

```bash
# Standard mode (reads JSON-RPC from stdin, responds on stdout)
./target/release/runbox

# List available tools
./target/release/runbox --list-tools
```

### Testing with a manual request

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | cargo run
```

---

## Client Configuration

### Claude Desktop

Add to your `claude_desktop_config.json` (typically at `~/Library/Application Support/Claude/claude_desktop_config.json` on macOS):

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

If the `runbox` binary is not in your PATH, use the full path:

```json
{
  "mcpServers": {
    "runbox": {
      "command": "/path/to/runbox",
      "args": []
    }
  }
}
```

### Cursor

In Cursor settings, add an MCP server:

```json
{
  "mcp": {
    "servers": {
      "runbox": {
        "command": "runbox",
        "args": []
      }
    }
  }
}
```

### Zed

In your Zed settings (`~/.config/zed/settings.json`):

```json
{
  "language_models": {
    "mcp_servers": {
      "runbox": {
        "command": "runbox",
        "args": []
      }
    }
  }
}
```

### Continue

In your Continue configuration (`.continue/config.json`):

```json
{
  "mcpServers": [
    {
      "name": "runbox",
      "command": "runbox",
      "args": []
    }
  ]
}
```

### Custom MCP Clients

Any MCP-compatible client can connect by spawning the `runbox` binary and communicating via stdin/stdout using JSON-RPC 2.0.

---

## Server Architecture

```
┌────────────────────┐       JSON-RPC 2.0        ┌─────────────────────┐
│    MCP Client      │ ◄──────────────────────►  │     McpServer       │
│  (Claude, Cursor,  │   (stdin/stdout by        │                     │
│   Zed, Continue)   │    default)               │  ┌───────────────┐  │
└────────────────────┘                           │  │      VFS      │  │
                                                 │  │ (in-memory FS)│  │
                                                 │  ├───────────────┤  │
                                                 │  │ ProcessManager│  │
                                                 │  │ (process reg) │  │
                                                 │  ├───────────────┤  │
                                                 │  │    Console    │  │
                                                 │  │ (structured   │  │
                                                 │  │  logging)     │  │
                                                 │  └───────────────┘  │
                                                 └─────────────────────┘
```

The server owns three core components:
- **VFS (Virtual Filesystem)** -- all file operations happen here
- **ProcessManager** -- tracks running process metadata
- **Console** -- collects structured log entries from command execution

---

## Transport Types

### Stdio (Default)

The primary transport for MCP. The server reads JSON-RPC messages from stdin (one per line) and writes responses to stdout.

- Logs go to stderr to avoid contaminating the JSON-RPC channel
- Log level is controlled via the `RUNBOX_LOG` environment variable

```bash
RUNBOX_LOG=debug runbox
```

### HTTP/SSE

The server supports HTTP with Server-Sent Events for web-based clients:

1. Client sends requests via `POST {base_url}/message`
2. Server sends responses via SSE stream at `GET {base_url}/sse`

This is handled by the `SseTransport` in native builds. In WASM, use the browser's native `EventSource` API.

### WebSocket

Full-duplex communication via WebSocket, handled by the `WebSocketTransport` using `tungstenite`.

### In-Process

For testing and embedding, `InProcessTransport` accepts a closure-based handler:

```rust
let transport = InProcessTransport {
    handler: Box::new(|msg| {
        server.handle(msg)
    }),
};
```

---

## Available Tools

### exec

Executes a shell command in the sandbox.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "command": {
      "type": "string",
      "description": "Command to execute (bun, npm, python, git, ls, cat...)"
    }
  },
  "required": ["command"]
}
```

**Examples:**

```json
{"name": "exec", "arguments": {"command": "npm init -y"}}
{"name": "exec", "arguments": {"command": "git init"}}
{"name": "exec", "arguments": {"command": "ls /"}}
{"name": "exec", "arguments": {"command": "python -c \"print('hello')\""}}
```

**Response:**
- Success: stdout text (and stderr in `[stderr]` section if present)
- Error: `isError: true` with error message

---

### read_file

Reads a file from the VFS.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "path": { "type": "string" }
  },
  "required": ["path"]
}
```

**Example:**

```json
{"name": "read_file", "arguments": {"path": "/package.json"}}
```

---

### write_file

Creates or overwrites a file in the VFS.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "path": { "type": "string" },
    "content": { "type": "string" }
  },
  "required": ["path", "content"]
}
```

**Example:**

```json
{"name": "write_file", "arguments": {"path": "/index.js", "content": "console.log('hello');"}}
```

---

### list_dir

Lists entries in a directory.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "path": { "type": "string", "default": "/" }
  },
  "required": []
}
```

**Example:**

```json
{"name": "list_dir", "arguments": {"path": "/src"}}
```

---

### remove

Removes a file or directory.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "path": { "type": "string" }
  },
  "required": ["path"]
}
```

---

### search

Searches text across files in the project.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "query": { "type": "string" },
    "path": { "type": "string", "default": "/" },
    "ext": { "type": "string", "description": "Filter by extension, e.g., .ts" }
  },
  "required": ["query"]
}
```

**Example:**

```json
{"name": "search", "arguments": {"query": "TODO", "path": "/src", "ext": ".ts"}}
```

---

### console_logs

Gets console log entries with optional filtering.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "level": { "type": "string", "enum": ["log", "info", "warn", "error", "debug"] },
    "since_id": { "type": "number" }
  },
  "required": []
}
```

**Examples:**

```json
{"name": "console_logs", "arguments": {}}
{"name": "console_logs", "arguments": {"level": "error"}}
{"name": "console_logs", "arguments": {"since_id": 42}}
```

---

### process_list

Lists active processes in the sandbox.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {},
  "required": []
}
```

---

## Resources

MCP resources are readable data sources that clients can query.

### Console Logs Resource

| Property | Value |
|---|---|
| **URI** | `runbox://console/logs` |
| **Name** | Console logs |
| **MIME type** | `application/json` |

Returns all console entries as a JSON array.

### Process List Resource

| Property | Value |
|---|---|
| **URI** | `runbox://process/list` |
| **Name** | Process list |
| **MIME type** | `application/json` |

Returns running processes as a JSON array.

### VFS Files

Every file in the VFS root is exposed as a resource with:

| Property | Value |
|---|---|
| **URI** | `file:///<filename>` |
| **Name** | Filename |
| **MIME type** | Auto-detected (js, ts, json, html, css, md, etc.) |

MIME type detection supports common web development file extensions.

---

## Prompts

MCP prompts are reusable templates that guide AI interactions.

### explain_file

Generates a prompt asking the AI to explain a file's contents.

**Arguments:**

| Name | Required | Description |
|---|---|---|
| `path` | Yes | Path to the file to explain |

**Generated Prompt:**

```
The user wants an explanation of the following file:

--- <path> ---
<file contents>
---

Explain the purpose and structure of this file clearly.
```

### fix_error

Generates a prompt to analyze console errors and propose fixes.

**Arguments:** None

**Generated Prompt:**

```
The following errors appeared in the sandbox console:

<recent error-level log entries>

Analyze the root cause and propose a concrete fix.
```

### scaffold

Generates a prompt to create a new project structure.

**Arguments:**

| Name | Required | Description |
|---|---|---|
| `type` | Yes | Project type: `bun-api`, `python-script`, or `fullstack` |
| `name` | No | Project name |

**Generated Prompt:**

```
Scaffold a new <type> project named <name>.
Create the directory structure, config files, and a basic entry point.
Use best practices for the chosen stack.
```

---

## MCP Client (Outbound)

RunboxJS can also act as an MCP **client**, connecting to external MCP servers to extend its capabilities.

### Registry

The `McpRegistry` manages multiple concurrent server connections:

```rust
use runbox::mcp::registry::McpRegistry;
use runbox::mcp::client::McpServerConfig;

let mut registry = McpRegistry::new();

// Add a server
registry.add(McpServerConfig {
    name: "filesystem".into(),
    transport: TransportConfig::Stdio {
        command: "npx".into(),
        args: vec!["@modelcontextprotocol/server-filesystem".into(), "/tmp".into()],
    },
    env: HashMap::new(),
})?;
```

### Connecting External Servers

Three transport types are supported for outbound connections:

**Stdio:**
```json
{
  "name": "filesystem",
  "transport": { "type": "stdio", "command": "npx", "args": ["@mcp/server-filesystem", "/tmp"] }
}
```

**HTTP/SSE:**
```json
{
  "name": "api-server",
  "transport": { "type": "sse", "url": "https://api.example.com/mcp" }
}
```

**WebSocket:**
```json
{
  "name": "ws-server",
  "transport": { "type": "websocket", "url": "wss://mcp.example.com" }
}
```

### Namespaced Tool Calls

When multiple servers are connected, tools are namespaced to avoid collisions:

```
server_name/tool_name
```

Example: `filesystem/read_file`, `github/create_issue`

If a tool name is unique across all servers, it can be called without the prefix:

```rust
// Explicit namespace
registry.call_tool("filesystem/read_file", args)?;

// Implicit (only if unambiguous)
registry.call_tool("read_file", args)?;
```

---

## Protocol Reference

### JSON-RPC 2.0

All communication follows JSON-RPC 2.0:

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "exec",
    "arguments": { "command": "ls /" }
  }
}
```

**Response (success):**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "content": [{ "type": "text", "text": "src\npackage.json\n" }],
    "isError": false
  }
}
```

**Response (error):**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32601,
    "message": "unknown method: foo/bar"
  }
}
```

**Notification (no response expected):**
```json
{
  "jsonrpc": "2.0",
  "method": "notifications/initialized",
  "params": {}
}
```

### Initialization Handshake

1. Client sends `initialize` with protocol version, capabilities, and client info
2. Server responds with its capabilities and server info
3. Client sends `notifications/initialized` notification
4. Server marks the connection as ready

**Example:**

```json
// Client -> Server
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2024-11-05",
    "capabilities": { "roots": { "listChanged": false } },
    "clientInfo": { "name": "my-client", "version": "1.0.0" }
  }
}

// Server -> Client
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": {
      "tools": { "listChanged": true },
      "resources": { "subscribe": false, "listChanged": true },
      "prompts": { "listChanged": false }
    },
    "serverInfo": { "name": "runbox", "version": "0.3.8" },
    "instructions": "RunBox: sandbox with VFS, shell, console, and inspector..."
  }
}

// Client -> Server (notification, no response)
{
  "jsonrpc": "2.0",
  "method": "notifications/initialized",
  "params": {}
}
```

### Error Codes

| Code | Name | Description |
|---|---|---|
| `-32700` | Parse Error | Invalid JSON |
| `-32600` | Invalid Request | Not a valid JSON-RPC request |
| `-32601` | Method Not Found | Unknown method name |
| `-32602` | Invalid Params | Missing or invalid parameters |
| `-32603` | Internal Error | Server-side error |

---

## Logging and Debugging

The MCP server logs to **stderr** to avoid contaminating the JSON-RPC channel on stdout.

### Log Levels

Control via the `RUNBOX_LOG` environment variable:

```bash
RUNBOX_LOG=error runbox   # Only errors
RUNBOX_LOG=warn  runbox   # Warnings and above (default)
RUNBOX_LOG=info  runbox   # Informational messages
RUNBOX_LOG=debug runbox   # Detailed debug output
RUNBOX_LOG=trace runbox   # Maximum verbosity
```

### Debug Tips

1. **Inspect raw messages:** Set `RUNBOX_LOG=debug` to see all incoming/outgoing JSON-RPC messages
2. **Test manually:** Pipe JSON to stdin and read stdout
3. **Check initialization:** Look for "MCP client connected" in stderr logs
4. **Verify tools:** Use `runbox --list-tools` to confirm tool availability

---

## Examples

### Complete Session Example

```bash
# Terminal 1: Start the MCP server with debug logging
RUNBOX_LOG=debug cargo run 2>mcp.log

# Terminal 2: Send requests
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | cargo run

echo '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' | cargo run

echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"write_file","arguments":{"path":"/hello.js","content":"console.log(42);"}}}' | cargo run

echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"exec","arguments":{"command":"node /hello.js"}}}' | cargo run
```

### Listing Resources

```json
// Request
{"jsonrpc":"2.0","id":4,"method":"resources/list","params":{}}

// Response
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "resources": [
      {
        "uri": "runbox://console/logs",
        "name": "Console logs",
        "description": "All console entries from the sandbox",
        "mimeType": "application/json"
      },
      {
        "uri": "runbox://process/list",
        "name": "Process list",
        "mimeType": "application/json"
      },
      {
        "uri": "file:///hello.js",
        "name": "hello.js",
        "mimeType": "application/javascript"
      }
    ]
  }
}
```

### Using a Prompt

```json
// Request
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "prompts/get",
  "params": {
    "name": "scaffold",
    "arguments": { "type": "bun-api", "name": "my-api" }
  }
}

// Response
{
  "jsonrpc": "2.0",
  "id": 5,
  "result": {
    "messages": [
      {
        "role": "user",
        "content": {
          "type": "text",
          "text": "Scaffold a new bun-api project named my-api.\nCreate the directory structure, config files, and a basic entry point.\nUse best practices for the chosen stack."
        }
      }
    ]
  }
}
```
