/// RunBox — servidor MCP stdio.
/// Lee mensajes JSON-RPC de stdin, responde por stdout.
/// Compatible con Claude Desktop, Cursor, Zed, Continue y cualquier cliente MCP.
///
/// Uso:
///   runbox                  — modo servidor MCP (stdin/stdout)
///   runbox --list-tools     — imprime las tools disponibles y sale
use std::io::{self, BufRead, Write};

use runbox::console::Console;
use runbox::mcp::server::McpServer;
use runbox::process::ProcessManager;
use runbox::vfs::Vfs;

fn main() {
    // Logs al stderr para no contaminar el canal JSON-RPC de stdout
    #[cfg(not(target_arch = "wasm32"))]
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(std::env::var("RUNBOX_LOG").unwrap_or_else(|_| "warn".into()))
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--list-tools") {
        print_tools();
        return;
    }

    tracing::info!("runbox MCP server starting (stdio transport)");

    let mut server = McpServer::new(Vfs::new(), ProcessManager::new(), Console::default());

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("stdin read error: {e}");
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        tracing::debug!("→ {trimmed}");

        if let Some(response) = server.handle(trimmed) {
            tracing::debug!("← {response}");
            if let Err(e) = writeln!(out, "{response}") {
                tracing::error!("stdout write error: {e}");
                break;
            }
            out.flush().ok();
        }
    }

    tracing::info!("runbox MCP server stopped");
}

fn print_tools() {
    use runbox::ai::tools::{all_tools, to_openai_format};
    let tools = all_tools();
    println!(
        "{}",
        serde_json::to_string_pretty(&to_openai_format(&tools)).unwrap()
    );
}
