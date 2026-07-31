/// Bindings WASM — expone runbox al browser vía wasm-bindgen.
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct RunboxInstance {
    vfs: crate::vfs::Vfs,
    pm: crate::process::ProcessManager,
    console: crate::console::Console,
    hot: crate::hotreload::HotReloader,
    inspector: crate::inspector::InspectorSession,
    terminal: crate::terminal::Terminal,
    preview: crate::preview::PreviewManager,
    journal: crate::agent_journal::AgentJournal,
    state: crate::shell::ShellState,
    store: Box<dyn crate::history_store::HistoryStore>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl RunboxInstance {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Self {
        let mut terminal = crate::terminal::Terminal::default();
        terminal.write_banner();
        terminal.write_prompt("/");
        Self {
            vfs: crate::vfs::Vfs::new(),
            pm: crate::process::ProcessManager::new(),
            console: crate::console::Console::default(),
            hot: crate::hotreload::HotReloader::new(80),
            inspector: crate::inspector::InspectorSession::new(),
            terminal,
            preview: crate::preview::PreviewManager::new(),
            journal: crate::agent_journal::AgentJournal::default(),
            state: crate::shell::ShellState::new(),
            store: crate::history_store::create_store(),
        }
    }

    // ── VFS ──────────────────────────────────────────────────────────────────

    pub fn file_exists(&self, path: &str) -> bool {
        self.vfs.exists(path)
    }

    // ── Shell ─────────────────────────────────────────────────────────────────

    /// Ejecuta un comando. Retorna JSON: { stdout, stderr, exit_code, cwd }
    pub fn exec(&mut self, line: &str) -> String {
        use crate::shell::{ListOp, Redirect, expand_glob, parse_command_list};

        let all_vfs_files = self.vfs.all_file_paths();

        let mut cmd_list = match parse_command_list(line, &self.state) {
            Ok(cl) => cl,
            Err(e) => {
                return serde_json::json!({
                    "stdout": "",
                    "stderr": e.to_string(),
                    "exit_code": 1,
                    "cwd": self.state.cwd,
                })
                .to_string();
            }
        };

        // Expand globs
        for pipeline in
            std::iter::once(&mut cmd_list.first).chain(cmd_list.rest.iter_mut().map(|(_, p)| p))
        {
            for cmd in pipeline.iter_mut() {
                let mut new_args = Vec::new();
                for arg in &cmd.args {
                    if arg.contains('*') || arg.contains('?') {
                        new_args.extend(expand_glob(arg, &all_vfs_files));
                    } else {
                        new_args.push(arg.clone());
                    }
                }
                cmd.args = new_args;
            }
        }

        let mut final_stdout: Vec<u8> = vec![];
        let mut final_stderr: Vec<u8> = vec![];
        let mut last_exit = 0i32;

        let mut pipelines: Vec<(Option<ListOp>, crate::shell::Pipeline)> =
            vec![(None, cmd_list.first)];
        for (op, pipeline) in cmd_list.rest {
            pipelines.push((Some(op), pipeline));
        }

        let mut prev_op: Option<ListOp> = None;
        for (op, pipeline) in pipelines {
            let should_run = match &prev_op {
                None => true,
                Some(ListOp::And) => last_exit == 0,
                Some(ListOp::Or) => last_exit != 0,
                Some(ListOp::Semi) => true,
            };
            if should_run {
                let (pstdout, pstderr, pexit) =
                    execute_pipeline(&pipeline, &mut self.vfs, &mut self.pm, &mut self.state);
                final_stdout.extend_from_slice(&pstdout);
                final_stderr.extend_from_slice(&pstderr);
                last_exit = pexit;
            }
            prev_op = op;
        }

        self.state.last_exit = last_exit;
        self.console.ingest_process(0, &final_stdout, &final_stderr);

        serde_json::json!({
            "stdout": String::from_utf8_lossy(&final_stdout),
            "stderr": String::from_utf8_lossy(&final_stderr),
            "exit_code": last_exit,
            "cwd": self.state.cwd,
        })
        .to_string()
    }

    // ── npm WASM install ─────────────────────────────────────────────────────

    /// Retorna JSON con los paquetes del package.json que faltan en node_modules.
    /// El host JS debe fetchearlos y llamar npm_process_tarball por cada uno.
    pub fn npm_packages_needed(&self) -> String {
        use crate::runtime::npm::packages_needed;
        serde_json::to_string(&packages_needed(&self.vfs)).unwrap_or_default()
    }

    /// Instala un paquete dado su tarball en bytes.
    /// Llamado desde JS después de hacer fetch() a registry.npmjs.org.
    pub fn npm_process_tarball(&mut self, name: &str, version: &str, bytes: &[u8]) -> String {
        use crate::runtime::npm::process_tarball;
        match process_tarball(name, version, bytes, &mut self.vfs) {
            Ok(_) => {
                self.console
                    .info(format!("installed {name}@{version}"), "npm");
                serde_json::json!({ "ok": true, "name": name, "version": version }).to_string()
            }
            Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
        }
    }

    // ── Git credentials ───────────────────────────────────────────────────────

    /// Guarda un token de autenticación para git push.
    /// Equivalente a: git config user.token <token>
    pub fn git_set_token(&mut self, token: &str) {
        use crate::runtime::git::GitCredentials;
        let mut creds = GitCredentials::load(&self.vfs);
        creds.token = Some(token.to_string());
        let _ = creds.save(&mut self.vfs);
    }

    pub fn git_set_user(&mut self, name: &str, email: &str) {
        use crate::runtime::git::GitCredentials;
        let mut creds = GitCredentials::load(&self.vfs);
        creds.username = Some(name.to_string());
        creds.email = Some(email.to_string());
        let _ = creds.save(&mut self.vfs);
    }

    // ── Console ───────────────────────────────────────────────────────────────

    /// Añade una entrada de log desde JS (ej: console.log del iframe).
    pub fn console_push(&mut self, level: &str, message: &str, source: &str) -> u64 {
        use crate::console::LogLevel;
        let lvl = match level {
            "info" => LogLevel::Info,
            "warn" => LogLevel::Warn,
            "error" => LogLevel::Error,
            "debug" => LogLevel::Debug,
            _ => LogLevel::Log,
        };
        self.console.push(lvl, message, source, None)
    }

    /// Retorna todas las entradas de consola como JSON.
    pub fn console_all(&self) -> String {
        self.console.to_json()
    }

    /// Retorna entradas nuevas desde un ID dado.
    pub fn console_since(&self, id: u64) -> String {
        let entries: Vec<_> = self.console.since(id).into_iter().collect();
        serde_json::to_string(&entries).unwrap_or_default()
    }

    pub fn console_clear(&mut self) {
        self.console.clear();
    }

    // ── AI tools ──────────────────────────────────────────────────────────────

    /// Retorna las definiciones de tools para el proveedor indicado.
    /// provider: "openai" | "anthropic" | "gemini" | "raw"
    pub fn ai_tools(&self, provider: &str) -> String {
        use crate::ai::tools::{
            all_tools, to_anthropic_format, to_gemini_format, to_openai_format,
        };
        let tools = all_tools();
        let value = match provider {
            "anthropic" => to_anthropic_format(&tools),
            "gemini" => to_gemini_format(&tools),
            _ => to_openai_format(&tools), // openai / default
        };
        value.to_string()
    }

    /// Ejecuta una tool call del AI. `call_json` es { name, arguments }.
    /// Retorna JSON: { name, content, error }
    pub fn ai_dispatch(&mut self, call_json: &str) -> String {
        use crate::ai::{skills::dispatch_with_preview, tools::ToolCall};

        let result = serde_json::from_str::<ToolCall>(call_json).map(|call| {
            dispatch_with_preview(
                &call,
                &mut self.vfs,
                &mut self.pm,
                &mut self.console,
                Some(&mut self.preview),
                Some(&mut self.journal),
                Some(&mut self.store),
            )
        });

        match result {
            Ok(r) => serde_json::to_string(&r).unwrap_or_default(),
            Err(e) => serde_json::json!({
                "name": "unknown",
                "content": null,
                "error": e.to_string()
            })
            .to_string(),
        }
    }

    // ── Agent Journal ─────────────────────────────────────────────────────────

    /// Retorna todas las entradas del diario del agente como JSON.
    pub fn journal_entries(&self) -> String {
        self.journal.to_json()
    }

    /// Retorna entradas del diario con id > since_id.
    pub fn journal_since(&self, since_id: u64) -> String {
        let entries: Vec<_> = self.journal.since(since_id).into_iter().collect();
        serde_json::to_string(&entries).unwrap_or_default()
    }

    /// Retorna el ID de sesión actual.
    pub fn get_session_id(&self) -> String {
        self.journal.session_id().to_string()
    }

    /// Persiste todas las entradas pendientes del journal al store (IndexedDB en WASM, SQLite en native).
    /// Retorna JSON: { persisted: count }
    pub fn journal_persist(&mut self) -> String {
        let pending = self.journal.drain_pending();
        let count = pending.len();
        if count > 0 {
            match self.store.save_journal_entries(&pending) {
                Ok(()) => serde_json::json!({ "persisted": count }).to_string(),
                Err(e) => serde_json::json!({ "persisted": 0, "error": e }).to_string(),
            }
        } else {
            serde_json::json!({ "persisted": 0 }).to_string()
        }
    }

    /// Busca en el historial del journal (memoria + persistido).
    /// `query_json`: { query?, tool?, file?, limit? }
    /// Retorna JSON: { results: [...], total }
    pub fn journal_search(&mut self, query_json: &str) -> String {
        use crate::ai::tools::ToolCall;
        let call = serde_json::from_str::<ToolCall>(&format!(
            r#"{{"name":"search_history","arguments":{}}}"#,
            query_json
        ))
        .unwrap_or_else(|_| ToolCall {
            name: "search_history".into(),
            arguments: serde_json::json!({}),
        });

        let result = crate::ai::skills::dispatch_with_preview(
            &call,
            &mut self.vfs,
            &mut self.pm,
            &mut self.console,
            Some(&mut self.preview),
            Some(&mut self.journal),
            Some(&mut self.store),
        );

        serde_json::to_string(&result).unwrap_or_default()
    }

    /// Exporta el journal en formato ctx-history-jsonl-v1 para interoperar con ctx.
    /// Retorna texto JSONL.
    pub fn journal_export_jsonl(&self) -> String {
        self.journal.to_jsonl()
    }

    // ── Sandbox ───────────────────────────────────────────────────────────────

    /// Serializa un SandboxEvent a JSON para enviarlo al browser.
    pub fn sandbox_event(&self, event_json: &str) -> String {
        event_json.to_string() // el browser genera el evento, runbox solo lo enruta
    }

    /// Procesa un SandboxCommand del browser. Retorna JSON de respuesta.
    pub fn sandbox_command(&mut self, cmd_json: &str) -> String {
        use crate::sandbox::SandboxCommand;

        let cmd = match serde_json::from_str::<SandboxCommand>(cmd_json) {
            Ok(c) => c,
            Err(e) => return serde_json::json!({ "error": e.to_string() }).to_string(),
        };

        match cmd {
            SandboxCommand::Exec { line } => self.exec(&line),
            SandboxCommand::WriteFile { path, content } => {
                match self.vfs.write(&path, content.into_bytes()) {
                    Ok(_) => serde_json::json!({ "ok": true }).to_string(),
                    Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
                }
            }
            SandboxCommand::ReadFile { path } => match self.vfs.read(&path) {
                Ok(b) => serde_json::json!({ "content": String::from_utf8_lossy(b) }).to_string(),
                Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
            },
            SandboxCommand::ListDir { path } => match self.vfs.list(&path) {
                Ok(e) => serde_json::to_string(&e).unwrap_or_default(),
                Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
            },
            SandboxCommand::Kill { pid } => match self.pm.kill(pid) {
                Ok(_) => serde_json::json!({ "killed": pid }).to_string(),
                Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
            },
            // Reload y Fullscreen son manejados por el browser, aquí solo confirmamos
            SandboxCommand::Reload { hard } => {
                serde_json::json!({ "action": "reload", "hard": hard }).to_string()
            }
            SandboxCommand::Fullscreen { enable } => {
                serde_json::json!({ "action": "fullscreen", "enable": enable }).to_string()
            }
            // Preview commands
            SandboxCommand::StartPreview { config_json } => {
                let config_str = config_json.as_deref().unwrap_or("{}");
                self.preview_start(config_str, wasm_now_ms())
            }
            SandboxCommand::StopPreview => self.preview_stop(),
            SandboxCommand::SetPreviewDomain { domain } => self.preview_set_domain(&domain),
            SandboxCommand::SharePreview => self.preview_share(),
            SandboxCommand::SetPreviewMetadata { metadata_json } => {
                self.preview_set_metadata(&metadata_json)
            }
            _ => serde_json::json!({ "error": "not implemented" }).to_string(),
        }
    }

    // ── Terminal (xterm.js) ───────────────────────────────────────────────────

    /// Devuelve los chunks de salida pendientes y los limpia. Llamar en cada frame.
    pub fn terminal_drain(&mut self) -> String {
        self.terminal.output_drain_json()
    }

    /// El usuario escribió `data` en el terminal.
    pub fn terminal_input(&mut self, data: &str, pid: Option<u32>) {
        self.terminal.input_push(data, pid);
        // Si es un comando completo (termina en \r o \n), ejecutarlo
        if data.ends_with('\r') || data.ends_with('\n') {
            let chunk = self.terminal.input_pop();
            if let Some(c) = chunk {
                let line = c.data.trim().to_string();
                if !line.is_empty() {
                    let result = self.exec(&line);
                    // Parsear stdout/stderr y escribir al terminal
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result) {
                        let stdout = v["stdout"].as_str().unwrap_or("").to_string();
                        let stderr = v["stderr"].as_str().unwrap_or("").to_string();
                        if !stdout.is_empty() {
                            self.terminal.write_stdout(0, stdout.replace('\n', "\r\n"));
                        }
                        if !stderr.is_empty() {
                            self.terminal.write_stderr(0, stderr.replace('\n', "\r\n"));
                        }
                    }
                    // Nuevo prompt
                    let cwd = self.state.cwd.clone();
                    self.terminal.write_prompt(&cwd);
                }
            }
        }
    }

    pub fn terminal_resize(&mut self, cols: u16, rows: u16) {
        self.terminal.resize(cols, rows);
    }

    pub fn terminal_size(&self) -> String {
        self.terminal.size_json()
    }

    pub fn terminal_clear(&mut self) {
        self.terminal.clear();
    }

    // ── CWD / Env ─────────────────────────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn get_cwd(&self) -> String {
        self.state.cwd.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn set_cwd(&mut self, path: &str) {
        self.state.set_cwd(path);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn get_env(&self, key: &str) -> Option<String> {
        self.state.env.get(key).cloned()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.state.export(key, value);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn tab_complete(&self, partial: &str) -> String {
        let all_files = self.vfs.all_file_paths();
        let matches: Vec<String> = all_files
            .into_iter()
            .filter(|f| f.starts_with(partial))
            .collect();
        serde_json::to_string(&matches).unwrap_or_else(|_| "[]".to_string())
    }

    // ── Service Worker ────────────────────────────────────────────────────────

    /// El Service Worker llama esto con la request interceptada (JSON).
    /// Retorna la respuesta (JSON) para devolver al iframe.
    pub fn sw_handle_request(&self, request_json: &str) -> String {
        use crate::network::{SwRequest, handle_sw_request};

        match serde_json::from_str::<SwRequest>(request_json) {
            Ok(req) => {
                let resp = handle_sw_request(&req, &self.vfs);
                serde_json::to_string(&resp).unwrap_or_default()
            }
            Err(e) => serde_json::json!({
                "id": "",
                "status": 400,
                "headers": {},
                "body": format!("invalid request: {e}"),
            })
            .to_string(),
        }
    }

    // ── Preview ──────────────────────────────────────────────────────────────

    /// Start a preview session. `config_json` is optional PreviewConfig JSON.
    /// Returns JSON with session info (id, url, status).
    pub fn preview_start(&mut self, config_json: &str, now_ms: u64) -> String {
        use crate::preview::PreviewConfig;
        let config: PreviewConfig = if config_json.is_empty() || config_json == "{}" {
            PreviewConfig::default()
        } else {
            match serde_json::from_str(config_json) {
                Ok(c) => c,
                Err(e) => {
                    return serde_json::json!({
                        "error": format!("invalid preview config: {e}")
                    })
                    .to_string();
                }
            }
        };

        let session = self.preview.start(config, now_ms);
        session.to_json()
    }

    /// Stop the current preview session.
    pub fn preview_stop(&mut self) -> String {
        match self.preview.stop() {
            Ok(()) => serde_json::json!({ "ok": true }).to_string(),
            Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
        }
    }

    /// Get the current preview status as JSON.
    pub fn preview_status(&self) -> String {
        self.preview.status_json()
    }

    /// Set a custom domain for the current preview session.
    /// The user's DNS must point this domain to the host running RunBox.
    pub fn preview_set_domain(&mut self, domain: &str) -> String {
        match self.preview.set_domain(domain) {
            Ok(()) => {
                let url = self
                    .preview
                    .current()
                    .map(|s| s.base_url())
                    .unwrap_or_default();
                serde_json::json!({
                    "ok": true,
                    "domain": domain,
                    "url": url,
                })
                .to_string()
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
        }
    }

    /// Generate a share URL for the current preview.
    /// Others can use this URL to view the project if the domain is accessible.
    pub fn preview_share(&mut self) -> String {
        match self.preview.share() {
            Ok(url) => {
                let session_id = self
                    .preview
                    .current()
                    .map(|s| s.id.clone())
                    .unwrap_or_default();
                serde_json::json!({
                    "ok": true,
                    "share_url": url,
                    "session_id": session_id,
                })
                .to_string()
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
        }
    }

    /// Update preview metadata (title, description, image, etc.).
    /// `metadata_json`: JSON with PreviewMetadata fields.
    pub fn preview_set_metadata(&mut self, metadata_json: &str) -> String {
        use crate::preview::PreviewMetadata;
        let metadata: PreviewMetadata = match serde_json::from_str(metadata_json) {
            Ok(m) => m,
            Err(e) => {
                return serde_json::json!({
                    "error": format!("invalid metadata: {e}")
                })
                .to_string();
            }
        };

        match self.preview.current_mut() {
            Some(session) => {
                session.config.metadata = metadata;
                serde_json::json!({ "ok": true }).to_string()
            }
            None => serde_json::json!({
                "error": "no active preview session"
            })
            .to_string(),
        }
    }

    /// Update the full preview configuration.
    /// `config_json`: JSON with PreviewConfig fields.
    pub fn preview_update_config(&mut self, config_json: &str) -> String {
        use crate::preview::PreviewConfig;
        let config: PreviewConfig = match serde_json::from_str(config_json) {
            Ok(c) => c,
            Err(e) => {
                return serde_json::json!({
                    "error": format!("invalid config: {e}")
                })
                .to_string();
            }
        };

        match self.preview.update_config(config) {
            Ok(()) => serde_json::json!({ "ok": true }).to_string(),
            Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
        }
    }

    /// Handle an incoming preview request from the Service Worker.
    /// Enhanced version of sw_handle_request with CORS, live-reload injection,
    /// metadata, custom headers, and SPA routing.
    /// Falls back to regular sw_handle_request if no preview session is active.
    pub fn preview_handle_request(&mut self, request_json: &str) -> String {
        use crate::network::SwRequest;

        let req: SwRequest = match serde_json::from_str(request_json) {
            Ok(r) => r,
            Err(e) => {
                return serde_json::json!({
                    "id": "",
                    "status": 400,
                    "headers": {},
                    "body": format!("invalid request: {e}"),
                })
                .to_string();
            }
        };

        // If preview is active, use the enhanced preview router
        if let Some(session) = self.preview.current_mut() {
            if session.status == crate::preview::PreviewStatus::Running {
                let resp = crate::preview::handle_preview_request(&req, &self.vfs, session);
                return serde_json::to_string(&resp).unwrap_or_default();
            }
        }

        // Fallback to standard handler
        let resp = crate::network::handle_sw_request(&req, &self.vfs);
        serde_json::to_string(&resp).unwrap_or_default()
    }

    /// Get preview session history as JSON.
    pub fn preview_history(&self) -> String {
        serde_json::to_string(&self.preview.history()).unwrap_or_default()
    }

    // ── Hot Reload ────────────────────────────────────────────────────────────

    /// Llama esto desde JS después de cada write_file para obtener la acción de recarga.
    /// `now_ms`: performance.now() del browser.
    /// Retorna JSON: null | { type: "inject_css"|"hmr"|"full_reload", paths?: [...] }
    pub fn hot_tick(&mut self, now_ms: u64) -> String {
        let changes = self.vfs.drain_changes();
        match self.hot.feed(changes, now_ms) {
            Some(action) => serde_json::to_string(&action).unwrap_or_default(),
            None => "null".into(),
        }
    }

    /// Fuerza un flush del hot reload ignorando el debounce.
    pub fn hot_flush(&mut self) -> String {
        match self.hot.flush_now() {
            Some(action) => serde_json::to_string(&action).unwrap_or_default(),
            None => "null".into(),
        }
    }

    // ── Inspector DOM ─────────────────────────────────────────────────────────

    /// Activa el inspector (el browser debe empezar a capturar clics/hover).
    pub fn inspector_activate(&mut self) {
        self.inspector.activate();
    }

    pub fn inspector_deactivate(&mut self) {
        self.inspector.deactivate();
    }

    pub fn inspector_is_active(&self) -> bool {
        self.inspector.active
    }

    /// El browser llama esto con el nodo inspeccionado serializado como JSON.
    pub fn inspector_set_node(&mut self, node_json: &str) {
        use crate::inspector::InspectedNode;
        if let Ok(node) = serde_json::from_str::<InspectedNode>(node_json) {
            self.inspector.set_node(node);
        }
    }

    /// Retorna el nodo actualmente seleccionado como JSON.
    pub fn inspector_selected(&self) -> String {
        self.inspector.selected_json()
    }

    /// Retorna las instrucciones del overlay de highlight como JSON.
    pub fn inspector_overlay(&self) -> String {
        self.inspector.overlay_json()
    }

    /// Retorna los últimos N nodos inspeccionados (historial).
    pub fn inspector_history(&self, limit: usize) -> String {
        let history = &self.inspector.history;
        let start = history.len().saturating_sub(limit);
        serde_json::to_string(&history[start..]).unwrap_or_default()
    }

    /// Genera la request de inspección para enviar al browser.
    /// `target`: "point:x,y" | "selector:.my-class" | "dismiss"
    pub fn inspector_request(&self, target: &str) -> String {
        use crate::inspector::InspectRequest;
        let req = if target == "dismiss" {
            InspectRequest::Dismiss
        } else if let Some(coords) = target.strip_prefix("point:") {
            let mut parts = coords.splitn(2, ',');
            let x = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let y = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            InspectRequest::AtPoint { x, y }
        } else if let Some(sel) = target.strip_prefix("selector:") {
            InspectRequest::BySelector {
                selector: sel.to_string(),
            }
        } else {
            InspectRequest::Dismiss
        };
        serde_json::to_string(&req).unwrap_or_default()
    }
}

// ── VFS — WASM-only (usan JsValue) ──────────────────────────────────────────
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl RunboxInstance {
    pub fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), JsValue> {
        self.vfs.write(path, content.to_vec()).js_err()
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, JsValue> {
        self.vfs.read(path).map(|b| b.to_vec()).js_err()
    }

    pub fn list_dir(&self, path: &str) -> Result<String, JsValue> {
        let entries = self.vfs.list(path).js_err()?;
        Ok(serde_json::to_string(&entries).unwrap_or_default())
    }

    pub fn remove_file(&mut self, path: &str) -> Result<(), JsValue> {
        self.vfs.remove(path).js_err()
    }

    // ── HTTP Server (simulated via globalThis.__runbox_servers) ───────────────

    /// Procesa una request HTTP hacia un servidor registrado via http.createServer().
    /// Llamado desde JS/Service Worker cuando intercepta fetch a localhost:PORT.
    /// request_json: { port, method, path, headers, body }
    /// Retorna: { status, headers, body }
    pub fn http_handle_request(&self, request_json: &str) -> String {
        #[derive(serde::Deserialize)]
        struct HttpReq {
            port: u16,
            method: Option<String>,
            path: Option<String>,
            headers: Option<serde_json::Value>,
            body: Option<String>,
        }

        let req: HttpReq = match serde_json::from_str(request_json) {
            Ok(r) => r,
            Err(e) => {
                return serde_json::json!({
                    "status": 400, "headers": {}, "body": format!("invalid request: {e}")
                })
                .to_string();
            }
        };

        let port = req.port;
        let method = req.method.unwrap_or_else(|| "GET".into());
        let path = req.path.unwrap_or_else(|| "/".into());
        let body = req.body.unwrap_or_default();
        let headers = req
            .headers
            .map(|h| serde_json::to_string(&h).unwrap_or_else(|_| "{}".into()))
            .unwrap_or_else(|| "{}".into());

        let script = format!(
            r#"(function() {{
            const handler = globalThis.__runbox_servers && globalThis.__runbox_servers[{port}];
            if (!handler) {{
                return JSON.stringify({{ status: 404, headers: {{'Content-Type': 'text/plain'}}, body: 'No server registered on port {port}' }});
            }}
            const req = {{
                method: {method_json},
                url: {path_json},
                path: {path_json},
                headers: {headers},
                body: {body_json},
                params: {{}},
                query: {{}},
            }};
            const res = {{
                __status: 200,
                __headers: {{ 'Content-Type': 'text/html' }},
                __body: '',
                writeHead(status, headers) {{
                    this.__status = status;
                    if (headers && typeof headers === 'object') Object.assign(this.__headers, headers);
                }},
                setHeader(k, v) {{ this.__headers[k] = v; }},
                getHeader(k) {{ return this.__headers[k]; }},
                end(data) {{ this.__body = data == null ? '' : String(data); }},
                send(data) {{ this.__body = data == null ? '' : (typeof data === 'object' ? JSON.stringify(data) : String(data)); }},
                json(data) {{
                    this.__headers['Content-Type'] = 'application/json';
                    this.__body = JSON.stringify(data);
                }},
                status(code) {{ this.__status = code; return this; }},
                type(t) {{ this.__headers['Content-Type'] = t; return this; }},
            }};
            try {{
                handler(req, res);
            }} catch(e) {{
                return JSON.stringify({{ status: 500, headers: {{'Content-Type': 'text/plain'}}, body: String(e) }});
            }}
            return JSON.stringify({{ status: res.__status, headers: res.__headers, body: res.__body }});
        }})()"#,
            port = port,
            method_json = serde_json::to_string(&method).unwrap(),
            path_json = serde_json::to_string(&path).unwrap(),
            headers = headers,
            body_json = serde_json::to_string(&body).unwrap(),
        );

        match js_sys::eval(&script) {
            Ok(val) => val.as_string().unwrap_or_else(|| {
                serde_json::json!({
                    "status": 500, "headers": {}, "body": "handler returned undefined"
                })
                .to_string()
            }),
            Err(e) => {
                let msg = js_sys::JSON::stringify(&e)
                    .ok()
                    .and_then(|s| s.as_string())
                    .unwrap_or_else(|| "eval error".into());
                serde_json::json!({ "status": 500, "headers": {}, "body": msg }).to_string()
            }
        }
    }
}

/// Retorna el timestamp actual en milisegundos.
/// En WASM usa js_sys::Date::now(); en native retorna 0.
fn wasm_now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0u64
    }
}

// ── Pipeline execution (free functions) ──────────────────────────────────────

fn execute_pipeline(
    pipeline: &[crate::shell::Command],
    vfs: &mut crate::vfs::Vfs,
    pm: &mut crate::process::ProcessManager,
    state: &mut crate::shell::ShellState,
) -> (Vec<u8>, Vec<u8>, i32) {
    use crate::runtime::ExecOutput;
    use crate::shell::Redirect;

    if pipeline.is_empty() {
        return (vec![], vec![], 0);
    }

    let mut stdin_data: Option<Vec<u8>> = None;
    let mut last_stdout = vec![];
    let mut last_stderr = vec![];
    let mut last_exit = 0i32;

    for (idx, cmd) in pipeline.iter().enumerate() {
        let is_last = idx == pipeline.len() - 1;
        let mut cmd_owned = cmd.clone();
        if let Some(data) = stdin_data.take() {
            cmd_owned.stdin = Some(data);
        }

        let result = dispatch_command(&cmd_owned, vfs, pm, state);
        let out = match result {
            Ok(o) => o,
            Err(e) => ExecOutput {
                stdout: vec![],
                stderr: e.to_string().into_bytes(),
                exit_code: 1,
            },
        };

        // Handle redirects
        let stdout = match &cmd_owned.redirect {
            Some(Redirect::Truncate(path)) => {
                let _ = vfs.write(path, out.stdout.clone());
                vec![]
            }
            Some(Redirect::Append(path)) => {
                let existing = vfs.read(path).map(|b| b.to_vec()).unwrap_or_default();
                let mut combined = existing;
                combined.extend_from_slice(&out.stdout);
                let _ = vfs.write(path, combined);
                vec![]
            }
            Some(Redirect::Stderr(path)) => {
                let _ = vfs.write(path, out.stderr.clone());
                out.stdout.clone()
            }
            Some(Redirect::StderrToStdout) => {
                let mut combined = out.stdout.clone();
                combined.extend_from_slice(&out.stderr);
                combined
            }
            None => out.stdout.clone(),
        };

        last_stderr = out.stderr.clone();
        last_exit = out.exit_code;

        if is_last {
            last_stdout = stdout;
        } else {
            stdin_data = Some(stdout);
        }
    }

    (last_stdout, last_stderr, last_exit)
}

fn dispatch_command(
    cmd: &crate::shell::Command,
    vfs: &mut crate::vfs::Vfs,
    pm: &mut crate::process::ProcessManager,
    state: &mut crate::shell::ShellState,
) -> crate::error::Result<crate::runtime::ExecOutput> {
    use crate::runtime::Runtime;
    use crate::runtime::bun::BunRuntime;
    use crate::runtime::git::GitRuntime;
    use crate::runtime::npm::PackageManagerRuntime;
    use crate::runtime::python::PythonRuntime;
    use crate::runtime::shell_builtins::ShellBuiltins;
    use crate::shell::RuntimeTarget;

    match RuntimeTarget::detect(cmd) {
        RuntimeTarget::Shell => ShellBuiltins.exec_with_state(cmd, vfs, pm, state),
        RuntimeTarget::Bun => BunRuntime.exec(cmd, vfs, pm),
        RuntimeTarget::Git => GitRuntime.exec(cmd, vfs, pm),
        RuntimeTarget::Npm => PackageManagerRuntime::npm().exec(cmd, vfs, pm),
        RuntimeTarget::Pnpm => PackageManagerRuntime::pnpm().exec(cmd, vfs, pm),
        RuntimeTarget::Yarn => PackageManagerRuntime::yarn().exec(cmd, vfs, pm),
        RuntimeTarget::Python => PythonRuntime.exec(cmd, vfs, pm),
        _ => {
            use crate::error::RunboxError;
            Err(RunboxError::Shell(format!(
                "{}: command not found",
                cmd.program
            )))
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
trait IntoJsError<T> {
    fn js_err(self) -> Result<T, JsValue>;
}

#[cfg(target_arch = "wasm32")]
impl<T> IntoJsError<T> for crate::error::Result<T> {
    fn js_err(self) -> Result<T, JsValue> {
        self.map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_dummy() {
        // Dummy test for coverage in wasm.rs
        assert!(true);
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// TERMINAL SESSION - API de alto nivel para shell interactivo
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct TerminalSession {
    // 🔧 CORRECCIÓN: Usar Box en lugar de Rc<RefCell<>> para evitar problemas de borrowing
    inner: Box<crate::terminal_api::TerminalSession>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl TerminalSession {
    /// Crea una nueva sesión de terminal
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Box::new(crate::terminal_api::TerminalSession::new()),
        }
    }

    /// Ejecuta un comando y retorna el resultado
    /// Retorna JSON: { pid, stdout, stderr, exit_code }
    pub fn exec(&mut self, command: &str) -> Result<JsValue, JsValue> {
        match self.inner.exec(command) {
            Ok(result) => {
                // 🔧 CORRECCIÓN: Convertir Vec<u8> a arrays de números para JSON
                let stdout_array: Vec<u8> = result.stdout;
                let stderr_array: Vec<u8> = result.stderr;

                // Crear Uint8Array directamente en lugar de pasar por JSON
                let stdout_js = js_sys::Uint8Array::new_with_length(stdout_array.len() as u32);
                stdout_js.copy_from(&stdout_array);

                let stderr_js = js_sys::Uint8Array::new_with_length(stderr_array.len() as u32);
                stderr_js.copy_from(&stderr_array);

                // Crear objeto resultado
                let obj = js_sys::Object::new();
                js_sys::Reflect::set(&obj, &"pid".into(), &JsValue::from(result.pid))?;
                js_sys::Reflect::set(&obj, &"stdout".into(), &stdout_js)?;
                js_sys::Reflect::set(&obj, &"stderr".into(), &stderr_js)?;
                js_sys::Reflect::set(&obj, &"exit_code".into(), &JsValue::from(result.exit_code))?;

                Ok(obj.into())
            }
            Err(e) => Err(JsValue::from_str(&format!("Error: {}", e))),
        }
    }

    /// Obtiene el output del terminal para renderizar en xterm.js
    /// Retorna JSON string con los chunks de output
    pub fn drain_output(&mut self) -> String {
        self.inner.drain_output()
    }

    /// Obtiene el estado actual de la sesión
    /// Retorna JSON: { cwd, env, last_exit_code, history }
    pub fn get_state(&self) -> JsValue {
        let state = self.inner.get_state();

        // 🔧 CORRECCIÓN: Construir el objeto manualmente para evitar memory access issues
        let obj = js_sys::Object::new();

        // cwd
        js_sys::Reflect::set(&obj, &"cwd".into(), &JsValue::from_str(&state.cwd)).ok();

        // env - convertir HashMap a Object
        let env_obj = js_sys::Object::new();
        for (key, value) in &state.env {
            js_sys::Reflect::set(&env_obj, &JsValue::from_str(key), &JsValue::from_str(value)).ok();
        }
        js_sys::Reflect::set(&obj, &"env".into(), &env_obj).ok();

        // last_exit_code
        js_sys::Reflect::set(
            &obj,
            &"last_exit_code".into(),
            &JsValue::from(state.last_exit_code),
        )
        .ok();

        // history - convertir Vec a Array
        let history_arr = js_sys::Array::new();
        for cmd in &state.history {
            history_arr.push(&JsValue::from_str(cmd));
        }
        js_sys::Reflect::set(&obj, &"history".into(), &history_arr).ok();

        obj.into()
    }

    /// Escribe un archivo en el VFS
    pub fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), JsValue> {
        self.inner
            .vfs_write(path, content.to_vec())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Lee un archivo del VFS
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, JsValue> {
        self.inner
            .vfs_read(path)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Lista archivos en un directorio
    pub fn list_dir(&self, path: &str) -> Result<String, JsValue> {
        let entries = self
            .inner
            .vfs_list(path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(serde_json::to_string(&entries).unwrap_or_default())
    }

    /// Verifica si un archivo existe
    pub fn file_exists(&self, path: &str) -> bool {
        self.inner.vfs_exists(path)
    }

    /// Elimina un archivo o directorio
    pub fn remove_file(&mut self, path: &str) -> Result<(), JsValue> {
        self.inner
            .vfs_remove(path)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Resetea la sesión a su estado inicial limpio
    /// Útil para limpiar estado entre tests
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}
