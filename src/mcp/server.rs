use super::protocol::*;
use crate::console::Console;
use crate::error::RunboxError;
use crate::process::ProcessManager;
use crate::runtime::bun::BunRuntime;
use crate::runtime::git::GitRuntime;
use crate::runtime::npm::PackageManagerRuntime;
use crate::runtime::python::PythonRuntime;
use crate::runtime::shell_builtins::ShellBuiltins;
use crate::runtime::{ExecOutput, Runtime};
use crate::shell::{Command, RuntimeTarget};
use crate::vfs::Vfs;
/// MCP Server — RunBox expone sus capacidades como servidor MCP.
/// Cualquier cliente compatible (Claude Desktop, Cursor, Zed, Continue, etc.)
/// puede conectarse y usar el VFS, shell, consola, etc.
use serde_json::{Value, json};

pub struct McpServer {
    pub vfs: Vfs,
    pub pm: ProcessManager,
    pub console: Console,
    pub preview: crate::preview::PreviewManager,
    initialized: bool,
}

impl McpServer {
    pub fn new(vfs: Vfs, pm: ProcessManager, console: Console) -> Self {
        Self {
            vfs,
            pm,
            console,
            preview: crate::preview::PreviewManager::new(),
            initialized: false,
        }
    }

    /// Punto de entrada: recibe un mensaje JSON y devuelve la respuesta JSON.
    pub fn handle(&mut self, raw: &str) -> Option<String> {
        let req = match parse_request(raw) {
            Ok(r) => r,
            Err(e) => {
                let resp =
                    RpcResponse::err(RequestId::Number(0), error_code::PARSE_ERROR, e.to_string());
                return Some(serialize_response(&resp));
            }
        };

        // Notifications no necesitan respuesta
        if req.is_notification() {
            self.handle_notification(&req.method, &req.params);
            return None;
        }

        let id = req.id.clone().unwrap();
        let result = self.dispatch(&req.method, &req.params, id.clone());
        Some(serialize_response(&result))
    }

    fn handle_notification(&mut self, method: &str, _params: &Value) {
        if method == "notifications/initialized" {
            self.initialized = true;
            self.console.info("MCP client connected", "mcp/server");
        }
    }

    fn dispatch(&mut self, method: &str, params: &Value, id: RequestId) -> RpcResponse {
        match method {
            "initialize" => self.handle_initialize(params, id),

            "tools/list" => RpcResponse::ok(id, self.tools_list()),
            "tools/call" => self.handle_tool_call(params, id),

            "resources/list" => RpcResponse::ok(id, self.resources_list()),
            "resources/read" => self.handle_resource_read(params, id),
            "resources/subscribe" => self.handle_resource_subscribe(params, id),
            "resources/unsubscribe" => self.handle_resource_unsubscribe(params, id),

            "prompts/list" => RpcResponse::ok(id, self.prompts_list()),
            "prompts/get" => self.handle_prompt_get(params, id),

            "ping" => RpcResponse::ok(id, json!({})),

            _ => RpcResponse::err(
                id,
                error_code::METHOD_NOT_FOUND,
                format!("unknown method: {method}"),
            ),
        }
    }

    // ── Initialize ────────────────────────────────────────────────────────────

    fn handle_initialize(&mut self, _params: &Value, id: RequestId) -> RpcResponse {
        let result = InitializeResult {
            protocol_version: MCP_VERSION.into(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(true),
                }),
                resources: Some(ResourcesCapability {
                    subscribe: Some(true),
                    list_changed: Some(true),
                }),
                prompts: Some(PromptsCapability {
                    list_changed: Some(false),
                }),
                logging: None,
            },
            server_info: Implementation {
                name: "runbox".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some(
                "RunBox: sandbox de desarrollo con VFS virtual, shell (bun/npm/python/git), \
                 consola de logs e inspector. Usa los tools para leer/escribir archivos, \
                 ejecutar comandos y consultar la consola."
                    .into(),
            ),
        };
        RpcResponse::ok(id, serde_json::to_value(result).unwrap())
    }

    // ── Tools ─────────────────────────────────────────────────────────────────

    fn tools_list(&self) -> Value {
        let tools: Vec<McpTool> = vec![
            mcp_tool(
                "exec",
                "Ejecuta un comando de shell en el sandbox.",
                json!({ "type":"object","properties":{ "command":{"type":"string","description":"Comando a ejecutar (bun, npm, python, git, ls, cat...)"} },"required":["command"] }),
            ),
            mcp_tool(
                "read_file",
                "Lee un archivo del VFS.",
                json!({ "type":"object","properties":{ "path":{"type":"string"} },"required":["path"] }),
            ),
            mcp_tool(
                "write_file",
                "Crea o sobreescribe un archivo en el VFS.",
                json!({ "type":"object","properties":{ "path":{"type":"string"},"content":{"type":"string"} },"required":["path","content"] }),
            ),
            mcp_tool(
                "list_dir",
                "Lista entradas de un directorio.",
                json!({ "type":"object","properties":{ "path":{"type":"string","default":"/"} },"required":[] }),
            ),
            mcp_tool(
                "remove",
                "Elimina un archivo o directorio.",
                json!({ "type":"object","properties":{ "path":{"type":"string"} },"required":["path"] }),
            ),
            mcp_tool(
                "search",
                "Busca texto en los archivos del proyecto.",
                json!({ "type":"object","properties":{
                    "query":{"type":"string"},
                    "path":{"type":"string","default":"/"},
                    "ext":{"type":"string","description":"Filtrar por extensión, ej: .ts"}
                },"required":["query"] }),
            ),
            mcp_tool(
                "console_logs",
                "Obtiene entradas de la consola.",
                json!({ "type":"object","properties":{
                    "level":{"type":"string","enum":["log","info","warn","error","debug"]},
                    "since_id":{"type":"number"}
                },"required":[] }),
            ),
            mcp_tool(
                "process_list",
                "Lista los procesos activos en el sandbox.",
                json!({ "type":"object","properties":{},"required":[] }),
            ),
            mcp_tool(
                "preview_start",
                "Inicia una sesi\u{00f3}n de preview del proyecto con configuraci\u{00f3}n opcional de dominio, puerto, metadata y CORS.",
                json!({ "type":"object","properties":{
                    "domain":{"type":"string","description":"Dominio personalizado (ej: preview.myapp.com)"},
                    "port":{"type":"number","description":"Puerto para localhost. Default: 3000"},
                    "title":{"type":"string","description":"T\u{00ed}tulo del preview"},
                    "description":{"type":"string","description":"Descripci\u{00f3}n para social sharing"},
                    "spa":{"type":"boolean","description":"Modo SPA (index.html fallback). Default: true"},
                    "live_reload":{"type":"boolean","description":"Inyectar script de live-reload. Default: true"}
                },"required":[] }),
            ),
            mcp_tool(
                "preview_stop",
                "Detiene la sesi\u{00f3}n de preview actual.",
                json!({ "type":"object","properties":{},"required":[] }),
            ),
            mcp_tool(
                "preview_set_domain",
                "Configura un dominio personalizado para compartir el preview con otros usuarios.",
                json!({ "type":"object","properties":{
                    "domain":{"type":"string","description":"Dominio personalizado (ej: preview.myapp.com)"}
                },"required":["domain"] }),
            ),
            mcp_tool(
                "preview_share",
                "Genera una URL compartible del preview actual. Si hay dominio configurado, usa ese dominio.",
                json!({ "type":"object","properties":{},"required":[] }),
            ),
            mcp_tool(
                "preview_status",
                "Obtiene el estado actual del preview (sesi\u{00f3}n, URL, configuraci\u{00f3}n).",
                json!({ "type":"object","properties":{},"required":[] }),
            ),
        ];
        json!({ "tools": tools })
    }

    fn handle_tool_call(&mut self, params: &Value, id: RequestId) -> RpcResponse {
        let name = match params["name"].as_str() {
            Some(n) => n,
            None => return RpcResponse::err(id, error_code::INVALID_PARAMS, "missing tool name"),
        };
        let args = &params["arguments"];

        let result = match name {
            "exec" => self.tool_exec(args),
            "read_file" => self.tool_read_file(args),
            "write_file" => self.tool_write_file(args),
            "list_dir" => self.tool_list_dir(args),
            "remove" => self.tool_remove(args),
            "search" => self.tool_search(args),
            "console_logs" => self.tool_console_logs(args),
            "process_list" => self.tool_process_list(),
            "preview_start" => self.tool_preview_start(args),
            "preview_stop" => self.tool_preview_stop(),
            "preview_set_domain" => self.tool_preview_set_domain(args),
            "preview_share" => self.tool_preview_share(),
            "preview_status" => self.tool_preview_status(),
            other => ToolCallResult::err(format!("unknown tool: {other}")),
        };

        RpcResponse::ok(id, serde_json::to_value(result).unwrap())
    }

    fn tool_exec(&mut self, args: &Value) -> ToolCallResult {
        let line = match args["command"].as_str() {
            Some(l) => l,
            None => return ToolCallResult::err("missing 'command'"),
        };

        let result: Result<ExecOutput, RunboxError> = (|| {
            let cmd = Command::parse(line)?;
            match RuntimeTarget::detect(&cmd) {
                RuntimeTarget::Bun => BunRuntime.exec(&cmd, &mut self.vfs, &mut self.pm),
                RuntimeTarget::Python => PythonRuntime.exec(&cmd, &mut self.vfs, &mut self.pm),
                RuntimeTarget::Git => GitRuntime.exec(&cmd, &mut self.vfs, &mut self.pm),
                RuntimeTarget::Shell => ShellBuiltins.exec(&cmd, &mut self.vfs, &mut self.pm),
                RuntimeTarget::Npm => {
                    PackageManagerRuntime::npm().exec(&cmd, &mut self.vfs, &mut self.pm)
                }
                RuntimeTarget::Pnpm => {
                    PackageManagerRuntime::pnpm().exec(&cmd, &mut self.vfs, &mut self.pm)
                }
                RuntimeTarget::Yarn => {
                    PackageManagerRuntime::yarn().exec(&cmd, &mut self.vfs, &mut self.pm)
                }
                _ => Err(RunboxError::Shell(format!(
                    "{}: command not found",
                    cmd.program
                ))),
            }
        })();

        match result {
            Ok(o) => {
                self.console.ingest_process(0, &o.stdout, &o.stderr);
                let mut text = String::from_utf8_lossy(&o.stdout).to_string();
                if !o.stderr.is_empty() {
                    text.push_str(&format!(
                        "\n[stderr]\n{}",
                        String::from_utf8_lossy(&o.stderr)
                    ));
                }
                if o.exit_code != 0 {
                    return ToolCallResult {
                        content: vec![McpContent::text(text)],
                        is_error: true,
                    };
                }
                ToolCallResult::ok(text)
            }
            Err(e) => ToolCallResult::err(e.to_string()),
        }
    }

    fn tool_read_file(&self, args: &Value) -> ToolCallResult {
        let path = match args["path"].as_str() {
            Some(p) => p,
            None => return ToolCallResult::err("missing 'path'"),
        };
        match self.vfs.read(path) {
            Ok(b) => ToolCallResult::ok(String::from_utf8_lossy(b)),
            Err(e) => ToolCallResult::err(e.to_string()),
        }
    }

    fn tool_write_file(&mut self, args: &Value) -> ToolCallResult {
        let path = match args["path"].as_str() {
            Some(p) => p,
            None => return ToolCallResult::err("missing 'path'"),
        };
        let content = match args["content"].as_str() {
            Some(c) => c,
            None => return ToolCallResult::err("missing 'content'"),
        };
        match self.vfs.write(path, content.as_bytes().to_vec()) {
            Ok(_) => {
                self.console
                    .info(format!("file written: {path}"), "mcp/server");
                ToolCallResult::ok(format!("OK — written {}", content.len()))
            }
            Err(e) => ToolCallResult::err(e.to_string()),
        }
    }

    fn tool_list_dir(&self, args: &Value) -> ToolCallResult {
        let path = args["path"].as_str().unwrap_or("/");
        match self.vfs.list(path) {
            Ok(mut entries) => {
                entries.sort();
                ToolCallResult::ok(entries.join("\n"))
            }
            Err(e) => ToolCallResult::err(e.to_string()),
        }
    }

    fn tool_remove(&mut self, args: &Value) -> ToolCallResult {
        let path = match args["path"].as_str() {
            Some(p) => p,
            None => return ToolCallResult::err("missing 'path'"),
        };
        match self.vfs.remove(path) {
            Ok(_) => ToolCallResult::ok(format!("removed: {path}")),
            Err(e) => ToolCallResult::err(e.to_string()),
        }
    }

    fn tool_search(&self, args: &Value) -> ToolCallResult {
        let query = match args["query"].as_str() {
            Some(q) => q,
            None => return ToolCallResult::err("missing 'query'"),
        };
        let root = args["path"].as_str().unwrap_or("/");
        let ext = args["ext"].as_str();

        let mut results = vec![];
        search_recursive(&self.vfs, root, query, ext, &mut results);

        if results.is_empty() {
            ToolCallResult::ok(format!("No matches for '{query}'"))
        } else {
            let text = results.join("\n");
            ToolCallResult::ok(text)
        }
    }

    fn tool_console_logs(&self, args: &Value) -> ToolCallResult {
        let since_id = args["since_id"].as_u64();
        let level_filter = args["level"].as_str();

        use crate::console::LogLevel;
        let entries: Vec<_> = match (since_id, level_filter) {
            (Some(id), _) => self.console.since(id).into_iter().cloned().collect(),
            (None, Some(l)) => {
                let lvl = match l {
                    "info" => LogLevel::Info,
                    "warn" => LogLevel::Warn,
                    "error" => LogLevel::Error,
                    "debug" => LogLevel::Debug,
                    _ => LogLevel::Log,
                };
                self.console.by_level(&lvl).into_iter().cloned().collect()
            }
            _ => self.console.all().into_iter().cloned().collect(),
        };

        let text = entries
            .iter()
            .map(|e| format!("[{}] [{}] {}", e.timestamp_ms, e.level, e.message))
            .collect::<Vec<_>>()
            .join("\n");

        ToolCallResult::ok(if text.is_empty() {
            "(no logs)".into()
        } else {
            text
        })
    }

    fn tool_process_list(&self) -> ToolCallResult {
        let running = self.pm.running();
        if running.is_empty() {
            return ToolCallResult::ok("(no running processes)");
        }
        let text = running
            .iter()
            .map(|p| format!("pid={} cmd={} {}", p.pid, p.command, p.args.join(" ")))
            .collect::<Vec<_>>()
            .join("\n");
        ToolCallResult::ok(text)
    }

    // ── Preview tools ─────────────────────────────────────────────────────────

    fn tool_preview_start(&mut self, args: &Value) -> ToolCallResult {
        use crate::preview::PreviewConfig;
        let mut config = PreviewConfig::default();

        if let Some(domain) = args["domain"].as_str() {
            config.domain = Some(domain.to_string());
        }
        if let Some(port) = args["port"].as_u64() {
            config.port = u16::try_from(port).unwrap_or(3000);
        }
        if let Some(title) = args["title"].as_str() {
            config.metadata.title = title.to_string();
        }
        if let Some(desc) = args["description"].as_str() {
            config.metadata.description = desc.to_string();
        }
        if let Some(spa) = args["spa"].as_bool() {
            config.spa = spa;
        }
        if let Some(lr) = args["live_reload"].as_bool() {
            config.live_reload = lr;
        }

        let session = self.preview.start(config, 0);
        ToolCallResult::ok(format!(
            "Preview started\n  session_id: {}\n  url: {}\n  status: running",
            session.id,
            session.base_url()
        ))
    }

    fn tool_preview_stop(&mut self) -> ToolCallResult {
        match self.preview.stop() {
            Ok(()) => ToolCallResult::ok("Preview stopped"),
            Err(e) => ToolCallResult::err(e.to_string()),
        }
    }

    fn tool_preview_set_domain(&mut self, args: &Value) -> ToolCallResult {
        let domain = match args["domain"].as_str() {
            Some(d) => d,
            None => return ToolCallResult::err("missing 'domain'"),
        };
        match self.preview.set_domain(domain) {
            Ok(()) => {
                let url = self
                    .preview
                    .current()
                    .map(|s| s.base_url())
                    .unwrap_or_default();
                ToolCallResult::ok(format!("Domain set: {domain}\nPreview URL: {url}"))
            }
            Err(e) => ToolCallResult::err(e.to_string()),
        }
    }

    fn tool_preview_share(&mut self) -> ToolCallResult {
        match self.preview.share() {
            Ok(url) => ToolCallResult::ok(format!("Share URL: {url}")),
            Err(e) => ToolCallResult::err(e.to_string()),
        }
    }

    fn tool_preview_status(&self) -> ToolCallResult {
        ToolCallResult::ok(self.preview.status_json())
    }

    // ── Resources ─────────────────────────────────────────────────────────────

    fn resources_list(&self) -> Value {
        let mut resources: Vec<McpResource> = vec![
            McpResource {
                uri: "runbox://console/logs".into(),
                name: "Console logs".into(),
                description: Some("Todas las entradas de consola del sandbox".into()),
                mime_type: Some("application/json".into()),
            },
            McpResource {
                uri: "runbox://process/list".into(),
                name: "Process list".into(),
                description: Some("Procesos activos en el sandbox".into()),
                mime_type: Some("application/json".into()),
            },
        ];

        // Archivos del VFS como resources
        if let Ok(entries) = self.vfs.list("/") {
            for entry in entries {
                let uri = format!("file:///{entry}");
                resources.push(McpResource {
                    uri,
                    name: entry.clone(),
                    description: None,
                    mime_type: mime_for(&entry),
                });
            }
        }

        json!({ "resources": resources })
    }

    fn handle_resource_read(&self, params: &Value, id: RequestId) -> RpcResponse {
        let uri = match params["uri"].as_str() {
            Some(u) => u,
            None => return RpcResponse::err(id, error_code::INVALID_PARAMS, "missing uri"),
        };

        let content = match uri {
            "runbox://console/logs" => ResourceContent {
                uri: uri.into(),
                mime_type: Some("application/json".into()),
                text: Some(self.console.to_json()),
            },
            "runbox://process/list" => {
                let list: Vec<_> = self.pm.running().iter().map(|p| {
                    serde_json::json!({ "pid": p.pid, "command": p.command, "args": p.args })
                }).collect();
                ResourceContent {
                    uri: uri.into(),
                    mime_type: Some("application/json".into()),
                    text: Some(serde_json::to_string(&list).unwrap_or_default()),
                }
            }
            other if other.starts_with("file:///") => {
                let path = format!("/{}", &other[8..]);
                match self.vfs.read(&path) {
                    Ok(b) => ResourceContent {
                        uri: uri.into(),
                        mime_type: mime_for(&path),
                        text: Some(String::from_utf8_lossy(b).into_owned()),
                    },
                    Err(e) => {
                        return RpcResponse::err(id, error_code::INTERNAL_ERROR, e.to_string());
                    }
                }
            }
            _ => {
                return RpcResponse::err(
                    id,
                    error_code::INVALID_PARAMS,
                    format!("unknown resource: {uri}"),
                );
            }
        };

        RpcResponse::ok(id, json!({ "contents": [content] }))
    }

    fn handle_resource_subscribe(&self, _params: &Value, id: RequestId) -> RpcResponse {
        // Implement resource subscriptions via SSE / internal event loop tracking
        RpcResponse::ok(id, json!({ "status": "subscribed" }))
    }

    fn handle_resource_unsubscribe(&self, _params: &Value, id: RequestId) -> RpcResponse {
        RpcResponse::ok(id, json!({ "status": "unsubscribed" }))
    }

    // ── Prompts ───────────────────────────────────────────────────────────────

    fn prompts_list(&self) -> Value {
        let prompts = vec![
            McpPrompt {
                name: "explain_file".into(),
                description: Some("Explica el contenido de un archivo del proyecto".into()),
                arguments: vec![McpPromptArgument {
                    name: "path".into(),
                    description: Some("Ruta del archivo".into()),
                    required: true,
                }],
            },
            McpPrompt {
                name: "fix_error".into(),
                description: Some("Analiza el error de la consola y propone un fix".into()),
                arguments: vec![],
            },
            McpPrompt {
                name: "scaffold".into(),
                description: Some("Genera la estructura de un proyecto nuevo".into()),
                arguments: vec![
                    McpPromptArgument {
                        name: "type".into(),
                        description: Some("Tipo: bun-api, python-script, fullstack".into()),
                        required: true,
                    },
                    McpPromptArgument {
                        name: "name".into(),
                        description: Some("Nombre del proyecto".into()),
                        required: false,
                    },
                ],
            },
            McpPrompt {
                name: "explain_project".into(),
                description: Some(
                    "Explica la arquitectura y dependencias de todo el proyecto.".into(),
                ),
                arguments: vec![],
            },
            McpPrompt {
                name: "refactor_code".into(),
                description: Some("Reescribe un bloque de código según las instrucciones.".into()),
                arguments: vec![
                    McpPromptArgument {
                        name: "path".into(),
                        description: Some("Ruta al archivo a refactorizar".into()),
                        required: true,
                    },
                    McpPromptArgument {
                        name: "instructions".into(),
                        description: Some("Instrucciones detalladas de refactor".into()),
                        required: true,
                    },
                ],
            },
        ];
        json!({ "prompts": prompts })
    }

    fn handle_prompt_get(&self, params: &Value, id: RequestId) -> RpcResponse {
        let name = match params["name"].as_str() {
            Some(n) => n,
            None => return RpcResponse::err(id, error_code::INVALID_PARAMS, "missing name"),
        };
        let args = &params["arguments"];

        let messages = match name {
            "explain_file" => {
                let path = args["path"].as_str().unwrap_or("/");
                let content = self
                    .vfs
                    .read(path)
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_else(|_| "(file not found)".into());
                vec![json!({
                    "role": "user",
                    "content": format!("Explica este archivo (`{path}`):\n\n```\n{content}\n```")
                })]
            }
            "fix_error" => {
                let logs = self
                    .console
                    .by_level(&crate::console::LogLevel::Error)
                    .iter()
                    .map(|e| e.message.clone())
                    .collect::<Vec<_>>()
                    .join("\n");
                vec![json!({
                    "role": "user",
                    "content": format!("Hay estos errores en la consola del sandbox. Analiza y propone un fix:\n\n```\n{logs}\n```")
                })]
            }
            "scaffold" => {
                let project_type = args["type"].as_str().unwrap_or("bun-api");
                let name = args["name"].as_str().unwrap_or("my-project");
                vec![json!({
                    "role": "user",
                    "content": format!("Genera la estructura completa de un proyecto '{project_type}' llamado '{name}'. Usa los tools write_file para crear cada archivo.")
                })]
            }
            "explain_project" => {
                let tree_str = self
                    .tool_list_dir(&json!({"path": "/"}))
                    .content
                    .first()
                    .map(|c| {
                        if let crate::mcp::protocol::McpContent::Text { text } = c {
                            text.clone()
                        } else {
                            "(No text)".into()
                        }
                    })
                    .unwrap_or_else(|| "(No files found)".into());
                vec![json!({
                    "role": "user",
                    "content": format!("Actúa como un arquitecto de software. Explica qué hace el proyecto basándote en la siguiente estructura de archivos:\n\n```\n{}\n```", tree_str)
                })]
            }
            "refactor_code" => {
                let path = args["path"].as_str().unwrap_or("/");
                let cmds = args["instructions"].as_str().unwrap_or("");
                let content = self
                    .vfs
                    .read(path)
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or("".into());
                vec![json!({
                    "role": "user",
                    "content": format!("Archio: {path}\n\nInstrucciones de refactor:\n{cmds}\n\nCódigo original:\n```\n{content}\n```\n\nAplica este refactor y devuélveme el resultado.")
                })]
            }
            other => {
                return RpcResponse::err(
                    id,
                    error_code::INVALID_PARAMS,
                    format!("unknown prompt: {other}"),
                );
            }
        };

        RpcResponse::ok(id, json!({ "messages": messages }))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn mcp_tool(name: &str, description: &str, input_schema: Value) -> McpTool {
    McpTool {
        name: name.into(),
        description: Some(description.into()),
        input_schema,
    }
}

fn mime_for(path: &str) -> Option<String> {
    let ext = path.rsplit('.').next()?;
    Some(
        match ext {
            "ts" | "tsx" => "text/typescript",
            "js" | "jsx" | "mjs" => "text/javascript",
            "json" => "application/json",
            "py" => "text/x-python",
            "md" => "text/markdown",
            "html" => "text/html",
            "css" => "text/css",
            "toml" => "text/x-toml",
            "yaml" | "yml" => "text/yaml",
            "sh" | "bash" => "text/x-sh",
            "txt" => "text/plain",
            _ => "application/octet-stream",
        }
        .into(),
    )
}

fn search_recursive(vfs: &Vfs, path: &str, query: &str, ext: Option<&str>, out: &mut Vec<String>) {
    let entries = match vfs.list(path) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries {
        let full = if path == "/" {
            format!("/{entry}")
        } else {
            format!("{path}/{entry}")
        };
        if let Ok(bytes) = vfs.read(&full) {
            if let Some(e) = ext
                && !entry.ends_with(e)
            {
                continue;
            }
            let text = String::from_utf8_lossy(bytes);
            for (i, line) in text.lines().enumerate() {
                if line.contains(query) {
                    out.push(format!("{}:{}: {}", full, i + 1, line.trim()));
                }
            }
        } else {
            search_recursive(vfs, &full, query, ext, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::Console;
    use crate::process::ProcessManager;
    use crate::vfs::Vfs;
    use serde_json::json;

    fn setup_server() -> McpServer {
        let vfs = Vfs::new();
        let pm = ProcessManager::new();
        let console = Console::new(100);
        McpServer::new(vfs, pm, console)
    }

    #[test]
    fn test_initialize() {
        let mut server = setup_server();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });

        let response = server.handle(&req.to_string()).unwrap();
        let resp_json: serde_json::Value = serde_json::from_str(&response).unwrap();

        assert_eq!(resp_json["id"], 1);
        assert!(resp_json["result"]["capabilities"]["tools"].is_object());
        assert_eq!(resp_json["result"]["serverInfo"]["name"], "runbox");
    }

    #[test]
    fn test_notifications_initialized() {
        let mut server = setup_server();
        assert!(!server.initialized);

        let req = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });

        let response = server.handle(&req.to_string());
        assert!(response.is_none());
        assert!(server.initialized);
    }

    #[test]
    fn test_tools_list() {
        let mut server = setup_server();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });

        let response = server.handle(&req.to_string()).unwrap();
        let resp_json: serde_json::Value = serde_json::from_str(&response).unwrap();

        assert_eq!(resp_json["id"], 2);
        assert!(resp_json["result"]["tools"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn test_tool_write_and_read_file() {
        let mut server = setup_server();

        // Write file
        let req_write = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "write_file",
                "arguments": {
                    "path": "/test.txt",
                    "content": "hello world"
                }
            }
        });

        let res_write = server.handle(&req_write.to_string()).unwrap();
        let res_write_json: serde_json::Value = serde_json::from_str(&res_write).unwrap();
        assert_eq!(res_write_json["result"]["isError"], false);

        // Read file
        let req_read = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "read_file",
                "arguments": {
                    "path": "/test.txt"
                }
            }
        });

        let res_read = server.handle(&req_read.to_string()).unwrap();
        let res_read_json: serde_json::Value = serde_json::from_str(&res_read).unwrap();

        let content = res_read_json["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(content, "hello world");
    }
}
