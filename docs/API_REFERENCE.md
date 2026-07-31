# RunboxJS API Reference

Complete reference for all `RunboxInstance` methods exposed via WebAssembly.

---

## Table of Contents

- [Initialization](#initialization)
- [Filesystem API](#filesystem-api)
  - [write_file](#write_file)
  - [read_file](#read_file)
  - [list_dir](#list_dir)
  - [file_exists](#file_exists)
  - [remove_file](#remove_file)
- [Command Execution](#command-execution)
  - [exec](#exec)
- [npm Tarball Flow (WASM)](#npm-tarball-flow-wasm)
  - [npm_packages_needed](#npm_packages_needed)
  - [npm_process_tarball](#npm_process_tarball)
- [Git Credentials](#git-credentials)
  - [git_set_user](#git_set_user)
  - [git_set_token](#git_set_token)
- [Console API](#console-api)
  - [console_push](#console_push)
  - [console_all](#console_all)
  - [console_since](#console_since)
  - [console_clear](#console_clear)
- [Terminal API](#terminal-api)
  - [terminal_input](#terminal_input)
  - [terminal_drain](#terminal_drain)
  - [terminal_resize](#terminal_resize)
  - [terminal_size](#terminal_size)
  - [terminal_clear](#terminal_clear)
- [Hot Reload API](#hot-reload-api)
  - [hot_tick](#hot_tick)
  - [hot_flush](#hot_flush)
- [Inspector API](#inspector-api)
  - [inspector_activate](#inspector_activate)
  - [inspector_deactivate](#inspector_deactivate)
  - [inspector_is_active](#inspector_is_active)
  - [inspector_set_node](#inspector_set_node)
  - [inspector_selected](#inspector_selected)
  - [inspector_overlay](#inspector_overlay)
  - [inspector_history](#inspector_history)
  - [inspector_request](#inspector_request)
- [Sandbox Bridge](#sandbox-bridge)
  - [sandbox_command](#sandbox_command)
  - [sandbox_event](#sandbox_event)
- [HTTP and Service Worker Bridge](#http-and-service-worker-bridge)
  - [http_handle_request](#http_handle_request)
  - [sw_handle_request](#sw_handle_request)
- [AI Tooling](#ai-tooling)
  - [ai_tools](#ai_tools)
  - [ai_dispatch](#ai_dispatch)

---

## Initialization

Before using any `RunboxInstance` method, you must initialize the WASM module:

```ts
import init, { RunboxInstance } from 'runboxjs';

await init();
const runbox = new RunboxInstance();
```

**Important:** Never call any method on `RunboxInstance` before `await init()` has completed. The WASM module must be fully loaded first.

---

## Filesystem API

### write_file

Writes a file to the virtual filesystem. Creates parent directories automatically. Marks the file as dirty for hot reload tracking.

```ts
write_file(path: string, content: Uint8Array): void
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path in the VFS (e.g., `/src/index.ts`) |
| `content` | `Uint8Array` | File content as a byte array |

**Example:**

```ts
// Write a text file
runbox.write_file('/src/app.ts', new TextEncoder().encode(`
  export function greet(name: string): string {
    return \`Hello, \${name}!\`;
  }
`));

// Write a JSON file
const pkg = { name: "my-app", version: "1.0.0", dependencies: {} };
runbox.write_file('/package.json', new TextEncoder().encode(JSON.stringify(pkg, null, 2)));
```

---

### read_file

Reads a file from the virtual filesystem.

```ts
read_file(path: string): Uint8Array
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path to the file |

**Returns:** `Uint8Array` -- the file content as raw bytes.

**Throws:** Error if the file does not exist.

**Example:**

```ts
const bytes = runbox.read_file('/package.json');
const content = new TextDecoder().decode(bytes);
const pkg = JSON.parse(content);
console.log(pkg.name); // "my-app"
```

---

### list_dir

Lists the entries (files and subdirectories) in a directory.

```ts
list_dir(path: string): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path to the directory |

**Returns:** JSON-encoded array of entry names.

**Example:**

```ts
runbox.write_file('/src/index.ts', new TextEncoder().encode(''));
runbox.write_file('/src/utils.ts', new TextEncoder().encode(''));

const entries = JSON.parse(runbox.list_dir('/src'));
console.log(entries); // ["index.ts", "utils.ts"]
```

---

### file_exists

Checks whether a file exists in the virtual filesystem.

```ts
file_exists(path: string): boolean
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path to check |

**Returns:** `boolean` -- `true` if the file exists, `false` otherwise.

**Example:**

```ts
console.log(runbox.file_exists('/package.json')); // false

runbox.exec('npm init -y');

console.log(runbox.file_exists('/package.json')); // true
```

---

### remove_file

Removes a file or directory from the virtual filesystem. Directories are removed recursively.

```ts
remove_file(path: string): void
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `path` | `string` | Absolute path to remove |

**Example:**

```ts
runbox.write_file('/tmp/test.txt', new TextEncoder().encode('hello'));
console.log(runbox.file_exists('/tmp/test.txt')); // true

runbox.remove_file('/tmp/test.txt');
console.log(runbox.file_exists('/tmp/test.txt')); // false
```

---

## Command Execution

### exec

Executes a shell command in the sandbox. The command is parsed, routed to the appropriate runtime, and executed against the VFS.

```ts
exec(line: string): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `line` | `string` | The full command line to execute |

**Returns:** JSON string with the following structure:

```json
{
  "stdout": "command output text",
  "stderr": "error output text",
  "exit_code": 0
}
```

**Supported Commands:**

| Runtime | Commands |
|---|---|
| Bun/Node | `bun`, `node`, `nodejs`, `tsx`, `ts-node` |
| npm | `npm`, `npx` |
| pnpm | `pnpm`, `pnpx` |
| Yarn | `yarn` |
| Git | `git` |
| Python | `python`, `python3`, `pip`, `pip3` |
| Shell | `echo`, `ls`, `cat`, `pwd`, `mkdir`, `rm`, `touch` |

**Examples:**

```ts
// Execute JavaScript
runbox.write_file('/app.js', new TextEncoder().encode("console.log('hi');"));
const r1 = JSON.parse(runbox.exec('node /app.js'));
console.log(r1.stdout); // "hi\n"

// Package management
const r2 = JSON.parse(runbox.exec('npm init -y'));
console.log(r2.exit_code); // 0

const r3 = JSON.parse(runbox.exec('npm add express'));
console.log(r3.stdout); // "added 1 package..."

// Git workflow
JSON.parse(runbox.exec('git init'));
JSON.parse(runbox.exec('git add .'));
const r4 = JSON.parse(runbox.exec('git commit -m "feat: initial"'));
console.log(r4.exit_code); // 0

// Shell builtins
const r5 = JSON.parse(runbox.exec('ls /'));
console.log(r5.stdout); // lists root directory

// Python
const r6 = JSON.parse(runbox.exec('pip install requests'));
console.log(r6.exit_code); // 0

// Error handling
const r7 = JSON.parse(runbox.exec('unknown-command'));
console.log(r7.exit_code); // non-zero
console.log(r7.stderr);    // "unknown-command: command not found"
```

---

## npm Tarball Flow (WASM)

In WASM environments, direct HTTP requests to the npm registry are not possible from synchronous Rust code. These methods implement a two-phase bridge pattern where the host JavaScript fetches tarballs and feeds them to RunboxJS.

### npm_packages_needed

Returns a list of packages that need to be fetched from the npm registry.

```ts
npm_packages_needed(): string
```

**Returns:** JSON-encoded array of package descriptors:

```json
[
  { "name": "express", "version": "4.18.2" },
  { "name": "cors", "version": "2.8.5" }
]
```

Returns `"[]"` if all dependencies are satisfied.

**Example:**

```ts
runbox.exec('npm add express cors');

const needed = JSON.parse(runbox.npm_packages_needed());
for (const { name, version } of needed) {
  // Fetch tarball from registry...
}
```

---

### npm_process_tarball

Processes a fetched npm tarball and installs the package into the VFS.

```ts
npm_process_tarball(name: string, version: string, bytes: Uint8Array): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `name` | `string` | Package name (e.g., `"express"`) |
| `version` | `string` | Package version (e.g., `"4.18.2"`) |
| `bytes` | `Uint8Array` | Raw tarball bytes (`.tgz` format) |

**Returns:** JSON status string.

**Complete WASM Install Example:**

```ts
async function installPackages(runbox) {
  const needed = JSON.parse(runbox.npm_packages_needed());

  for (const { name, version } of needed) {
    // Fetch package metadata
    const meta = await fetch(
      `https://registry.npmjs.org/${name}/${version}`
    ).then(r => r.json());

    // Download the tarball
    const tarball = await fetch(meta.dist.tarball)
      .then(r => r.arrayBuffer());

    // Install into VFS
    runbox.npm_process_tarball(name, version, new Uint8Array(tarball));
  }
}
```

---

## Git Credentials

### git_set_user

Sets the git user name and email for commits.

```ts
git_set_user(name: string, email: string): void
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `name` | `string` | Git author name |
| `email` | `string` | Git author email |

**Example:**

```ts
runbox.git_set_user('Jane Doe', 'jane@example.com');
runbox.exec('git init');
runbox.exec('git add .');
runbox.exec('git commit -m "initial"');
// Commit will show: Jane Doe <jane@example.com>
```

---

### git_set_token

Sets a token for authenticating remote git operations (clone, push, pull, fetch).

```ts
git_set_token(token: string): void
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `token` | `string` | Authentication token (used as Bearer token for HTTP auth) |

**Example:**

```ts
runbox.git_set_token('ghp_xxxxxxxxxxxxxxxxxxxx');
runbox.exec('git clone https://github.com/user/repo.git');
```

**Security:** Treat the token as sensitive. It is stored in `/.git/config` within the VFS (in-memory only).

---

## Console API

The console API provides structured logging with levels, timestamps, and source tagging.

### console_push

Adds a log entry to the console.

```ts
console_push(level: string, message: string, source: string): number
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `level` | `string` | Log level: `"log"`, `"info"`, `"warn"`, `"error"`, or `"debug"` |
| `message` | `string` | Log message text |
| `source` | `string` | Source identifier (e.g., `"app"`, `"build"`, `"runtime"`) |

**Returns:** `number` -- the ID of the created log entry (auto-incrementing).

**Example:**

```ts
const id = runbox.console_push('info', 'Application started', 'app');
console.log(`Created log entry #${id}`);

runbox.console_push('error', 'Connection failed', 'network');
runbox.console_push('debug', 'Cache hit for /api/users', 'cache');
```

---

### console_all

Returns all console log entries.

```ts
console_all(): string
```

**Returns:** JSON-encoded array of log entries:

```json
[
  {
    "id": 1,
    "level": "info",
    "message": "Application started",
    "source": "app",
    "timestamp_ms": 1700000000000
  }
]
```

**Example:**

```ts
const entries = JSON.parse(runbox.console_all());
for (const entry of entries) {
  console.log(`[${entry.level}] ${entry.message}`);
}
```

---

### console_since

Returns console entries added after the specified ID. Useful for cursor-based polling.

```ts
console_since(id: number): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `id` | `number` | The last seen entry ID; returns entries with ID > this value |

**Returns:** JSON-encoded array of log entries (same format as `console_all`).

**Example:**

```ts
let lastId = 0;

// Poll for new entries
setInterval(() => {
  const newEntries = JSON.parse(runbox.console_since(lastId));
  for (const entry of newEntries) {
    renderLogEntry(entry);
    lastId = entry.id;
  }
}, 1000);
```

---

### console_clear

Clears all console log entries.

```ts
console_clear(): void
```

**Example:**

```ts
runbox.console_push('info', 'test', 'app');
console.log(JSON.parse(runbox.console_all()).length); // 1

runbox.console_clear();
console.log(JSON.parse(runbox.console_all()).length); // 0
```

---

## Terminal API

The terminal API provides bidirectional I/O streams for xterm.js-like terminal integration.

### terminal_input

Sends user input to the terminal (e.g., keystrokes from xterm.js).

```ts
terminal_input(data: string, pid?: number): void
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `data` | `string` | Input data (e.g., typed characters, control sequences) |
| `pid` | `number` (optional) | Target process ID. Defaults to the active process |

**Example:**

```ts
// Connect xterm.js input to RunboxJS
const term = new Terminal();
term.onData((data) => {
  runbox.terminal_input(data);
});
```

---

### terminal_drain

Returns and clears all pending terminal output chunks.

```ts
terminal_drain(): string
```

**Returns:** JSON-encoded array of output chunks:

```json
[
  { "pid": 1, "data": "Hello, world!\n" },
  { "pid": 1, "data": "$ " }
]
```

**Example:**

```ts
// Poll terminal output and write to xterm.js
function pollTerminal() {
  const chunks = JSON.parse(runbox.terminal_drain());
  for (const chunk of chunks) {
    term.write(chunk.data);
  }
  requestAnimationFrame(pollTerminal);
}
pollTerminal();
```

---

### terminal_resize

Updates the terminal dimensions. Call this when the xterm.js container is resized.

```ts
terminal_resize(cols: number, rows: number): void
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `cols` | `number` | Number of columns |
| `rows` | `number` | Number of rows |

**Example:**

```ts
// Sync xterm.js resize events
const fitAddon = new FitAddon();
term.loadAddon(fitAddon);

new ResizeObserver(() => {
  fitAddon.fit();
  runbox.terminal_resize(term.cols, term.rows);
}).observe(termContainer);
```

---

### terminal_size

Returns the current terminal dimensions.

```ts
terminal_size(): string
```

**Returns:** JSON string:

```json
{ "cols": 80, "rows": 24 }
```

---

### terminal_clear

Clears the terminal output buffer.

```ts
terminal_clear(): void
```

---

## Hot Reload API

The hot reload engine provides intelligent file change detection with debounced reload strategies.

### hot_tick

Checks for pending file changes and returns a reload action if the debounce period has elapsed.

```ts
hot_tick(now_ms: number): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `now_ms` | `number` | Current timestamp in milliseconds (e.g., `performance.now()` or `Date.now()`) |

**Returns:** JSON string -- either `"null"` (no action needed) or a reload action:

```json
{
  "type": "inject_css",
  "paths": ["/src/styles/app.css"]
}
```

**Action Types:**

| Type | Trigger | Description |
|---|---|---|
| `inject_css` | `.css` file changes | Inject updated CSS without page reload |
| `hmr` | `.js`, `.ts`, `.jsx`, `.tsx` changes | Hot Module Replacement |
| `full_reload` | All other file changes | Full page reload |

**Example:**

```ts
// Call hot_tick on every animation frame after file writes
function checkHotReload() {
  const action = JSON.parse(runbox.hot_tick(performance.now()));
  if (action) {
    switch (action.type) {
      case 'inject_css':
        action.paths.forEach(path => reloadCSS(path));
        break;
      case 'hmr':
        action.paths.forEach(path => hotReplaceModule(path));
        break;
      case 'full_reload':
        location.reload();
        break;
    }
  }
  requestAnimationFrame(checkHotReload);
}
checkHotReload();
```

---

### hot_flush

Immediately returns any pending reload action, bypassing the debounce timer.

```ts
hot_flush(): string
```

**Returns:** Same format as `hot_tick`. Returns `"null"` if no changes are pending.

**Example:**

```ts
// Force-check after a batch of file writes
runbox.write_file('/src/a.ts', new TextEncoder().encode('...'));
runbox.write_file('/src/b.ts', new TextEncoder().encode('...'));
const action = JSON.parse(runbox.hot_flush());
```

---

## Inspector API

The inspector API provides DOM element inspection capabilities, similar to browser DevTools element inspection.

### inspector_activate

Activates the DOM inspector session.

```ts
inspector_activate(): void
```

---

### inspector_deactivate

Deactivates the DOM inspector session and clears the selection.

```ts
inspector_deactivate(): void
```

---

### inspector_is_active

Returns whether the inspector is currently active.

```ts
inspector_is_active(): boolean
```

---

### inspector_set_node

Sets the currently inspected node data. Called from the browser when the user hovers or clicks an element.

```ts
inspector_set_node(node_json: string): void
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `node_json` | `string` | JSON-encoded node data |

**Node Data Format:**

```json
{
  "tag": "div",
  "id": "header",
  "classes": ["container", "flex"],
  "attributes": { "data-testid": "main-header" },
  "box_model": {
    "x": 0, "y": 0, "width": 1024, "height": 64,
    "margin": { "top": 0, "right": 0, "bottom": 0, "left": 0 },
    "padding": { "top": 16, "right": 24, "bottom": 16, "left": 24 },
    "border": { "top": 0, "right": 0, "bottom": 1, "left": 0 }
  },
  "computed_styles": {
    "display": "flex",
    "background-color": "rgb(255, 255, 255)"
  }
}
```

---

### inspector_selected

Returns the currently selected node data.

```ts
inspector_selected(): string
```

**Returns:** JSON-encoded node data, or `"null"` if no node is selected.

---

### inspector_overlay

Returns overlay rendering data for the currently selected element (highlight box position and dimensions).

```ts
inspector_overlay(): string
```

**Returns:** JSON-encoded overlay data for rendering highlight boxes in the browser.

---

### inspector_history

Returns the history of previously inspected nodes.

```ts
inspector_history(limit: number): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `limit` | `number` | Maximum number of history entries to return |

**Returns:** JSON-encoded array of node data objects.

---

### inspector_request

Sends an inspection request. This is the main entry point for element selection.

```ts
inspector_request(target: string): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `target` | `string` | Target specifier (see formats below) |

**Target Formats:**

| Format | Example | Description |
|---|---|---|
| `point:x,y` | `point:120,240` | Select element at screen coordinates |
| `selector:css` | `selector:.card` | Select element by CSS selector |
| `dismiss` | `dismiss` | Deselect current element |

**Returns:** JSON-encoded inspection result.

**Example:**

```ts
// Select element at coordinates
const result1 = JSON.parse(runbox.inspector_request('point:100,200'));

// Select by CSS selector
const result2 = JSON.parse(runbox.inspector_request('selector:#main-content'));

// Dismiss selection
runbox.inspector_request('dismiss');
```

---

## Sandbox Bridge

General-purpose bridge for structured commands and events between the host application and the sandbox.

### sandbox_command

Sends a structured command to the sandbox.

```ts
sandbox_command(cmd_json: string): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `cmd_json` | `string` | JSON-encoded command object |

**Returns:** JSON-encoded response.

---

### sandbox_event

Sends a lifecycle event to the sandbox.

```ts
sandbox_event(event_json: string): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `event_json` | `string` | JSON-encoded event object |

**Returns:** JSON-encoded response.

---

## HTTP and Service Worker Bridge

These methods allow browser adapters to proxy network requests through the virtual runtime.

### http_handle_request

Handles an HTTP request and returns a response. Used for simulating localhost servers.

```ts
http_handle_request(request_json: string): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `request_json` | `string` | JSON-encoded HTTP request object |

**Request Format:**

```json
{
  "method": "GET",
  "url": "http://localhost:3000/api/users",
  "headers": { "Accept": "application/json" },
  "body": null
}
```

**Returns:** JSON-encoded HTTP response:

```json
{
  "status": 200,
  "headers": { "Content-Type": "application/json" },
  "body": "[{\"id\": 1, \"name\": \"Alice\"}]"
}
```

---

### sw_handle_request

Handles a Service Worker fetch event. Used for intercepting network requests and serving files from the VFS.

```ts
sw_handle_request(request_json: string): string
```

**Parameters:** Same format as `http_handle_request`.

**Returns:** Same format as `http_handle_request`.

**Example: Service Worker Integration:**

```js
// In your Service Worker
self.addEventListener('fetch', (event) => {
  const req = {
    method: event.request.method,
    url: event.request.url,
    headers: Object.fromEntries(event.request.headers),
  };
  const response = JSON.parse(runbox.sw_handle_request(JSON.stringify(req)));
  event.respondWith(new Response(response.body, {
    status: response.status,
    headers: response.headers,
  }));
});
```

---

## AI Tooling

### ai_tools

Returns AI tool definitions in the specified provider's format.

```ts
ai_tools(provider: string): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `provider` | `string` | AI provider format: `"openai"`, `"anthropic"`, `"gemini"`, or `"raw"` |

**Returns:** JSON-encoded tool definitions in the provider's expected format.

**Provider Formats:**

| Provider | Format |
|---|---|
| `openai` | `[{ type: "function", function: { name, description, parameters } }]` |
| `anthropic` | `[{ name, description, input_schema }]` |
| `gemini` | `[{ name, description, parameters }]` |
| `raw` | `[{ name, description, parameters }]` (internal format) |

**Example:**

```ts
// For OpenAI
const tools = JSON.parse(runbox.ai_tools('openai'));
const response = await openai.chat.completions.create({
  model: 'gpt-4',
  messages: [...],
  tools: tools,
});

// For Anthropic
const tools = JSON.parse(runbox.ai_tools('anthropic'));
const response = await anthropic.messages.create({
  model: 'claude-3-sonnet-20240229',
  messages: [...],
  tools: tools,
});
```

---

### ai_dispatch

Dispatches an AI tool call and returns the result.

```ts
ai_dispatch(call_json: string): string
```

**Parameters:**

| Name | Type | Description |
|---|---|---|
| `call_json` | `string` | JSON-encoded tool call |

**Call Format:**

```json
{
  "name": "tool_name",
  "arguments": { ... }
}
```

**Returns:** JSON-encoded result:

```json
{
  "name": "tool_name",
  "content": "result text or data",
  "error": null
}
```

**Available Tools:**

| Tool Name | Arguments | Description |
|---|---|---|
| `read_file` | `{ path: string }` | Read file content from VFS |
| `write_file` | `{ path: string, content: string }` | Write file to VFS |
| `list_dir` | `{ path: string }` | List directory contents |
| `exec_command` | `{ command: string }` | Execute shell command |
| `search_code` | `{ query: string, path?: string, extension?: string }` | Search code in files |
| `get_console_logs` | `{ level?: string, since_id?: number }` | Get console entries |
| `reload_sandbox` | `{ hard?: boolean }` | Trigger sandbox reload |
| `install_packages` | `{ packages: string[], dev?: boolean, manager?: string }` | Install npm packages |
| `get_file_tree` | `{ path?: string, depth?: number }` | Get file tree visualization |

**Example: AI Assistant Loop:**

```ts
// 1. Get tool definitions
const tools = JSON.parse(runbox.ai_tools('openai'));

// 2. Send to AI with user message
const response = await openai.chat.completions.create({
  model: 'gpt-4',
  messages: [{ role: 'user', content: 'Create a hello world Express app' }],
  tools: tools,
});

// 3. Process tool calls from the AI response
for (const toolCall of response.choices[0].message.tool_calls) {
  const result = JSON.parse(runbox.ai_dispatch(JSON.stringify({
    name: toolCall.function.name,
    arguments: JSON.parse(toolCall.function.arguments),
  })));

  console.log(`Tool: ${result.name}`);
  console.log(`Result: ${result.content}`);
  if (result.error) {
    console.error(`Error: ${result.error}`);
  }
}
```
