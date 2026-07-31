# RunboxJS Development Guide

This guide covers local development setup, building, testing, contributing, and troubleshooting for the RunboxJS project.

---

## Table of Contents

- [Prerequisites](#prerequisites)
- [Project Structure](#project-structure)
- [Local Setup](#local-setup)
- [Building](#building)
  - [Rust Checks](#rust-checks)
  - [WASM Build](#wasm-build)
  - [Build Script Options](#build-script-options)
- [Testing](#testing)
  - [Unit Tests](#unit-tests)
  - [Benchmarks](#benchmarks)
  - [Manual Testing](#manual-testing)
- [Code Architecture](#code-architecture)
  - [Adding a New Runtime](#adding-a-new-runtime)
  - [Adding a New AI Tool](#adding-a-new-ai-tool)
  - [Adding an MCP Tool](#adding-an-mcp-tool)
- [Publishing](#publishing)
- [Release Process](#release-process)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)

---

## Prerequisites

| Tool | Version | Purpose |
|---|---|---|
| **Rust** | Latest stable (edition 2024) | Core language |
| **wasm-pack** | Latest | WASM compilation and packaging |
| **Node.js** | 18+ | Build scripts and npm publishing |
| **npm** | 9+ | Package management |

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Install wasm-pack

```bash
cargo install wasm-pack
```

Or via npm:

```bash
npm install -g wasm-pack
```

### Add the WASM target

```bash
rustup target add wasm32-unknown-unknown
```

---

## Project Structure

```
runbox/
├── src/
│   ├── lib.rs                 # Crate root -- module declarations
│   ├── main.rs                # Native binary -- MCP server (stdio)
│   ├── error.rs               # Error types (RunboxError)
│   ├── vfs.rs                 # Virtual Filesystem
│   ├── shell.rs               # Command parser & runtime target detection
│   ├── process.rs             # Process registry
│   ├── console.rs             # Structured logging
│   ├── terminal.rs            # Terminal I/O streams
│   ├── hotreload.rs           # Hot reload engine
│   ├── inspector.rs           # DOM inspector
│   ├── network.rs             # HTTP/SW bridge
│   ├── sandbox.rs             # Sandbox command/event bridge
│   ├── wasm.rs                # WASM bindings (wasm32 only)
│   ├── runtime/
│   │   ├── mod.rs             # Runtime trait definition
│   │   ├── bun.rs             # Bun/Node.js runtime
│   │   ├── npm.rs             # Package manager runtime (npm/pnpm/yarn/bun)
│   │   ├── git.rs             # In-memory Git runtime
│   │   ├── python.rs          # Python/pip runtime
│   │   ├── shell_builtins.rs  # Shell builtin commands
│   │   └── js_engine.rs       # JS/TS execution engine
│   ├── ai/
│   │   ├── mod.rs             # AI module entry point
│   │   ├── tools.rs           # Tool schema definitions
│   │   └── skills.rs          # Tool dispatch and skill implementations
│   └── mcp/
│       ├── mod.rs             # MCP module entry point
│       ├── server.rs          # MCP server implementation
│       ├── client.rs          # MCP client for external servers
│       ├── registry.rs        # Multi-server registry
│       ├── protocol.rs        # JSON-RPC 2.0 / MCP protocol types
│       └── transport.rs       # Transport layer (stdio/SSE/WS)
├── skills/
│   └── AGENT_SKILL.md         # AI assistant integration guide
├── benches/
│   └── core_bench.rs          # Criterion benchmarks
├── tests/                     # Integration tests
├── docs/                      # Documentation
│   ├── ARCHITECTURE.md        # Architecture documentation
│   ├── API_REFERENCE.md       # API reference
│   ├── DEVELOPMENT.md         # This file
│   └── MCP_GUIDE.md           # MCP server guide
├── build.mjs                  # WASM build & publish script
├── Cargo.toml                 # Rust dependencies and metadata
├── TECHNICAL_DOCS.md          # Legacy technical documentation
├── WASM_SETUP.md              # WASM setup guide
├── NPM_PUBLISH.md             # npm publishing guide
└── README.md                  # Project overview
```

---

## Local Setup

1. **Clone the repository:**

```bash
git clone https://github.com/owellandry/runbox.git
cd runbox
```

2. **Verify Rust setup:**

```bash
rustc --version
cargo --version
```

3. **Run initial checks:**

```bash
cargo check
cargo test
```

4. **Build WASM package (optional):**

```bash
node build.mjs
```

---

## Building

### Rust Checks

```bash
# Type checking (fast, no codegen)
cargo check

# Full build (native target)
cargo build

# Release build (native target, optimized)
cargo build --release
```

### WASM Build

The project uses a custom build script (`build.mjs`) that wraps `wasm-pack`:

```bash
# Standard WASM build
node build.mjs
```

This will:
1. Run `wasm-pack build --target web --release`
2. Copy the template `package.json` to `pkg/`
3. Apply any necessary post-processing

### Build Script Options

```bash
# Bump version and build
node build.mjs --bump patch    # 0.3.8 -> 0.3.9
node build.mjs --bump minor    # 0.3.8 -> 0.4.0
node build.mjs --bump major    # 0.3.8 -> 1.0.0

# Publish to npm
node build.mjs --publish

# Combined: bump + build + publish
node build.mjs --bump patch --publish
```

### Release Profile

The release build is optimized for small WASM binary size:

```toml
[profile.release]
opt-level = "z"      # Optimize for binary size
lto = true           # Link-time optimization
codegen-units = 1    # Single codegen unit
panic = "abort"      # Abort on panic (no unwinding)
```

---

## Testing

### Unit Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name

# Run tests for a specific module
cargo test vfs::
cargo test runtime::
```

### Benchmarks

The project uses [Criterion](https://github.com/bheisler/criterion.rs) for benchmarking:

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench core_bench
```

Benchmark results are stored in `target/criterion/` with HTML reports.

### Manual Testing

#### Testing the Native MCP Server

```bash
# Build and run the MCP server
cargo run

# List available tools
cargo run -- --list-tools

# Test with a JSON-RPC request (piped to stdin)
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run
```

#### Testing WASM in a Browser

1. Build the WASM package:

```bash
node build.mjs
```

2. Create a test HTML file or link the `pkg/` directory to a Vite project:

```bash
# In your test Vite project
npm link ../runbox/pkg
```

3. Import and use `RunboxInstance`:

```ts
import init, { RunboxInstance } from 'runboxjs';
await init();
const runbox = new RunboxInstance();
```

---

## Code Architecture

### Adding a New Runtime

To add support for a new command/language runtime:

1. **Create the runtime file** at `src/runtime/your_runtime.rs`

2. **Implement the `Runtime` trait:**

```rust
use crate::runtime::{Runtime, ExecOutput};
use crate::shell::Command;
use crate::vfs::Vfs;
use crate::process::ProcessManager;
use crate::error::RunboxError;

pub struct YourRuntime;

impl Runtime for YourRuntime {
    fn name(&self) -> &str {
        "your-runtime"
    }

    fn exec(
        &self,
        cmd: &Command,
        vfs: &mut Vfs,
        pm: &mut ProcessManager,
    ) -> Result<ExecOutput, RunboxError> {
        // Implementation here
        Ok(ExecOutput {
            stdout: b"output".to_vec(),
            stderr: vec![],
            exit_code: 0,
        })
    }
}
```

3. **Register in `src/runtime/mod.rs`:**

```rust
pub mod your_runtime;
```

4. **Add to the shell routing** in `src/shell.rs`:

Add your program names to the `RuntimeTarget` enum and `detect()` method.

5. **Wire up in execution points** (`src/wasm.rs` exec method, `src/mcp/server.rs` tool_exec).

### Adding a New AI Tool

1. **Define the tool schema** in `src/ai/tools.rs`:

```rust
ToolDef {
    name: "your_tool",
    description: "Description of what the tool does",
    parameters: json!({
        "type": "object",
        "properties": {
            "param1": { "type": "string", "description": "..." }
        },
        "required": ["param1"]
    }),
}
```

Add it to the `all_tools()` function.

2. **Implement the skill** in `src/ai/skills.rs`:

```rust
fn skill_your_tool(args: &Value, vfs: &mut Vfs, pm: &mut ProcessManager, console: &mut Console) -> String {
    let param1 = args["param1"].as_str().unwrap_or("");
    // Implementation
    format!("result: {param1}")
}
```

Add the dispatch case in the `dispatch()` function.

### Adding an MCP Tool

1. **Add the tool definition** in the `tools_list()` method of `src/mcp/server.rs`:

```rust
mcp_tool(
    "your_tool",
    "Description of the tool",
    json!({
        "type": "object",
        "properties": { ... },
        "required": [...]
    }),
),
```

2. **Implement the handler:**

```rust
fn tool_your_tool(&mut self, args: &Value) -> ToolCallResult {
    // Implementation
    ToolCallResult::ok("result")
}
```

3. **Add to the dispatch** in `handle_tool_call()`:

```rust
"your_tool" => self.tool_your_tool(args),
```

---

## Publishing

### Prerequisites

- npm account with publish access to `runboxjs`
- `npm login` completed

### Steps

```bash
# 1. Bump version in Cargo.toml
node build.mjs --bump patch

# 2. Build WASM package
node build.mjs

# 3. Verify the pkg/ directory
ls pkg/

# 4. Publish to npm
node build.mjs --publish
# or manually:
cd pkg && npm publish
```

For detailed publishing notes, see [NPM_PUBLISH.md](../NPM_PUBLISH.md).

---

## Release Process

1. **Update version:** Run `node build.mjs --bump <patch|minor|major>`
2. **Run checks:** `cargo check && cargo test && cargo bench`
3. **Build WASM:** `node build.mjs`
4. **Test locally:** Verify the `pkg/` output works in a browser project
5. **Publish:** `node build.mjs --publish`
6. **Tag release:** `git tag v0.X.Y && git push --tags`

---

## Troubleshooting

### Cargo check fails with "edition 2024"

Ensure you have the latest stable Rust:

```bash
rustup update stable
```

### wasm-pack not found

```bash
cargo install wasm-pack
# or
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
```

### WASM build fails with "crate-type must be cdylib"

`Cargo.toml` must include:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

### Native build fails with missing dependencies

Some native-only dependencies require system libraries. On Ubuntu/Debian:

```bash
sudo apt-get install pkg-config libssl-dev
```

### Tests fail with "python3 not found"

Some tests may require Python 3. Install it or skip Python-related tests:

```bash
sudo apt-get install python3
```

### Benchmark results are noisy

Use `cargo bench` with minimal system load. Close other applications and use:

```bash
cargo bench -- --warm-up-time 5
```

### WASM module too large

The release profile already optimizes for size (`opt-level = "z"`, LTO, etc.). Additionally:

```bash
# Install wasm-opt for further optimization
cargo install wasm-opt
wasm-opt -Oz pkg/runbox_bg.wasm -o pkg/runbox_bg.wasm
```

---

## Contributing

### Code Style

- Follow standard Rust formatting (`cargo fmt`)
- Use `cargo clippy` for linting
- Keep functions focused and well-named
- Add doc comments for public APIs
- Use `tracing` for logging, not `println!`

### Commit Messages

Follow conventional commits format:

```
feat: add deno runtime support
fix: correct lockfile generation for pnpm
docs: update API reference
refactor: simplify VFS path normalization
test: add git merge conflict tests
```

### Pull Request Guidelines

1. Create a feature branch from `master`
2. Make focused, incremental changes
3. Add tests for new functionality
4. Run `cargo check && cargo test && cargo clippy` before submitting
5. Update documentation if you change public APIs

### Conditional Compilation

When writing code that behaves differently on WASM vs native:

```rust
// WASM-specific
#[cfg(target_arch = "wasm32")]
fn do_something() { /* browser behavior */ }

// Native-specific
#[cfg(not(target_arch = "wasm32"))]
fn do_something() { /* native behavior */ }
```

Always provide both implementations or a graceful fallback.

### Error Handling

Use `RunboxError` variants from `src/error.rs`:

```rust
use crate::error::RunboxError;

fn my_function() -> Result<String, RunboxError> {
    Err(RunboxError::NotFound("file /foo not found".into()))
}
```

Never use `unwrap()` or `expect()` in library code. Use `?` for error propagation.
