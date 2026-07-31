use crate::agent_journal::AgentJournal;
use crate::ai::tools::{ToolCall, ToolResult};
use crate::console::Console;
use crate::error::RunboxError;
use crate::history_store::{HistoryStore, SearchQuery};
use crate::preview::PreviewManager;
use crate::process::ProcessManager;
use crate::runtime::Runtime;
use crate::runtime::bun::BunRuntime;
use crate::runtime::git::GitRuntime;
use crate::runtime::npm::PackageManagerRuntime;
use crate::runtime::python::PythonRuntime;
use crate::runtime::shell_builtins::ShellBuiltins;
use crate::shell::{Command, RuntimeTarget};
use crate::vfs::Vfs;
/// Implementación de skills — ejecuta las tool calls que pide el AI.
use serde_json::{Value, json};

/// Ejecuta una tool call del AI y retorna el resultado.
pub fn dispatch(
    call: &ToolCall,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    console: &mut Console,
) -> ToolResult {
    dispatch_with_preview(call, vfs, pm, console, None, None, None)
}

/// Ejecuta una tool call con acceso opcional al PreviewManager, AgentJournal y HistoryStore.
pub fn dispatch_with_preview(
    call: &ToolCall,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    console: &mut Console,
    preview: Option<&mut PreviewManager>,
    journal: Option<&mut AgentJournal>,
    store: Option<&mut Box<dyn HistoryStore>>,
) -> ToolResult {
    let content = match call.name.as_str() {
        "read_file" => skill_read_file(call, vfs),
        "write_file" => skill_write_file(call, vfs),
        "list_dir" => skill_list_dir(call, vfs),
        "exec_command" => skill_exec_command(call, vfs, pm, console),
        "search_code" => skill_search_code(call, vfs),
        "get_console_logs" => skill_get_console_logs(call, console),
        "reload_sandbox" => skill_reload(call),
        "install_packages" => skill_install_packages(call, vfs, pm, console),
        "get_file_tree" => skill_file_tree(call, vfs),
        "preview_start" => skill_preview_start(call, preview),
        "preview_stop" => skill_preview_stop(preview),
        "preview_configure" => skill_preview_configure(call, preview),
        "preview_share" => skill_preview_share(preview),
        "patch_file" => skill_patch_file(call, vfs),
        "fetch_url" => skill_fetch_url(call),
        "scaffold_project" => skill_scaffold_project(call, vfs),
        "debug_error" => skill_debug_error(call, vfs),
        "explain_project" => skill_explain_project(call, vfs),
        "refactor_code" => skill_refactor_code(call, vfs),
        "generate_tests" => skill_generate_tests(call, vfs),
        "agent_memo" => skill_agent_memo(call, journal, store),
        "get_agent_journal" => skill_get_agent_journal(call, journal),
        "search_history" => skill_search_history(call, journal, store),
        other => Err(RunboxError::Runtime(format!("unknown skill: {other}"))),
    };

    match content {
        Ok(value) => ToolResult {
            name: call.name.clone(),
            content: value,
            error: None,
        },
        Err(e) => ToolResult {
            name: call.name.clone(),
            content: json!(null),
            error: Some(e.to_string()),
        },
    }
}

// ── Skills ────────────────────────────────────────────────────────────────────

fn skill_read_file(call: &ToolCall, vfs: &Vfs) -> crate::error::Result<Value> {
    let path = str_arg(&call.arguments, "path")?;
    if path.contains("..") {
        return Err(RunboxError::Runtime(
            "path traversal not allowed".to_string(),
        ));
    }
    let bytes = vfs.read(path)?;
    let content = String::from_utf8_lossy(bytes).to_string();
    Ok(json!({ "path": path, "content": content, "size": bytes.len() }))
}

fn skill_write_file(call: &ToolCall, vfs: &mut Vfs) -> crate::error::Result<Value> {
    let path = str_arg(&call.arguments, "path")?;
    if path.contains("..") {
        return Err(RunboxError::Runtime(
            "path traversal not allowed".to_string(),
        ));
    }
    let content = str_arg(&call.arguments, "content")?;
    vfs.write(path, content.as_bytes().to_vec())?;
    Ok(json!({ "path": path, "written": content.len() }))
}

fn skill_list_dir(call: &ToolCall, vfs: &Vfs) -> crate::error::Result<Value> {
    let path = call.arguments["path"].as_str().unwrap_or("/");
    let mut entries = vfs.list(path)?;
    entries.sort();
    Ok(json!({ "path": path, "entries": entries }))
}

fn skill_exec_command(
    call: &ToolCall,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    console: &mut Console,
) -> crate::error::Result<Value> {
    let line = str_arg(&call.arguments, "command")?;
    let cmd = Command::parse(line)?;

    let output = match RuntimeTarget::detect(&cmd) {
        RuntimeTarget::Bun => BunRuntime.exec(&cmd, vfs, pm),
        RuntimeTarget::Python => PythonRuntime.exec(&cmd, vfs, pm),
        RuntimeTarget::Git => GitRuntime.exec(&cmd, vfs, pm),
        RuntimeTarget::Shell => ShellBuiltins.exec(&cmd, vfs, pm),
        RuntimeTarget::Npm => PackageManagerRuntime::npm().exec(&cmd, vfs, pm),
        RuntimeTarget::Pnpm => PackageManagerRuntime::pnpm().exec(&cmd, vfs, pm),
        RuntimeTarget::Yarn => PackageManagerRuntime::yarn().exec(&cmd, vfs, pm),
        _ => Err(RunboxError::Shell(format!(
            "{}: command not found",
            cmd.program
        ))),
    }?;

    // Ingestar output en consola
    let pid = pm.running().last().map(|p| p.pid);
    if let Some(pid) = pid {
        console.ingest_process(pid, &output.stdout, &output.stderr);
    }

    Ok(json!({
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "exit_code": output.exit_code,
    }))
}

fn skill_search_code(call: &ToolCall, vfs: &Vfs) -> crate::error::Result<Value> {
    let query = str_arg(&call.arguments, "query")?;
    let root = call.arguments["path"].as_str().unwrap_or("/");
    let extension = call.arguments["extension"].as_str();

    let mut matches: Vec<Value> = vec![];
    search_recursive(vfs, root, query, extension, &mut matches);

    Ok(json!({ "query": query, "matches": matches, "total": matches.len() }))
}

fn search_recursive(vfs: &Vfs, path: &str, query: &str, ext: Option<&str>, out: &mut Vec<Value>) {
    let entries = match vfs.list(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        let full_path = if path == "/" {
            format!("/{entry}")
        } else {
            format!("{path}/{entry}")
        };

        if let Ok(bytes) = vfs.read(&full_path) {
            // Es un archivo
            if let Some(ext_filter) = ext
                && !entry.ends_with(ext_filter)
            {
                continue;
            }
            let text = String::from_utf8_lossy(bytes);
            for (i, line) in text.lines().enumerate() {
                if line.contains(query) {
                    out.push(json!({
                        "file": full_path,
                        "line": i + 1,
                        "text": line.trim(),
                    }));
                }
            }
        } else {
            // Es un directorio, recursión
            search_recursive(vfs, &full_path, query, ext, out);
        }
    }
}

fn skill_get_console_logs(call: &ToolCall, console: &Console) -> crate::error::Result<Value> {
    let since_id = call.arguments["since_id"].as_u64();
    let level_str = call.arguments["level"].as_str();

    let entries: Vec<_> = match (since_id, level_str) {
        (Some(id), _) => console.since(id).into_iter().cloned().collect(),
        (None, Some(l)) => {
            use crate::console::LogLevel;
            let level = match l {
                "info" => LogLevel::Info,
                "warn" => LogLevel::Warn,
                "error" => LogLevel::Error,
                "debug" => LogLevel::Debug,
                _ => LogLevel::Log,
            };
            console.by_level(&level).into_iter().cloned().collect()
        }
        _ => console.all().into_iter().cloned().collect(),
    };

    Ok(json!({ "entries": entries, "count": entries.len() }))
}

fn skill_reload(call: &ToolCall) -> crate::error::Result<Value> {
    let hard = call.arguments["hard"].as_bool().unwrap_or(false);
    Ok(json!({ "action": "reload", "hard": hard }))
}

fn skill_install_packages(
    call: &ToolCall,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    console: &mut Console,
) -> crate::error::Result<Value> {
    let packages: Vec<String> = call.arguments["packages"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let dev = call.arguments["dev"].as_bool().unwrap_or(false);

    let manager = call.arguments["manager"]
        .as_str()
        .unwrap_or_else(|| detect_package_manager(vfs));

    let cmd_str = if packages.is_empty() {
        format!("{manager} install")
    } else {
        let dev_flag = if dev {
            match manager {
                "npm" => " --save-dev",
                "pnpm" | "yarn" => " -D",
                _ => " --dev",
            }
        } else {
            ""
        };
        format!("{manager} add{dev_flag} {}", packages.join(" "))
    };

    let cmd = Command::parse(&cmd_str)?;
    let runtime: Box<dyn Runtime> = match manager {
        "pnpm" => Box::new(PackageManagerRuntime::pnpm()),
        "yarn" => Box::new(PackageManagerRuntime::yarn()),
        "bun" => Box::new(BunRuntime),
        _ => Box::new(PackageManagerRuntime::npm()),
    };

    let output = runtime.exec(&cmd, vfs, pm)?;
    console.ingest_process(0, &output.stdout, &output.stderr);

    Ok(json!({
        "manager": manager,
        "packages": packages,
        "stdout": String::from_utf8_lossy(&output.stdout),
        "exit_code": output.exit_code,
    }))
}

fn skill_file_tree(call: &ToolCall, vfs: &Vfs) -> crate::error::Result<Value> {
    let root = call.arguments["path"].as_str().unwrap_or("/");
    let depth = call.arguments["depth"].as_u64().unwrap_or(5) as usize;
    Ok(build_tree(vfs, root, depth))
}

fn build_tree(vfs: &Vfs, path: &str, depth: usize) -> Value {
    if depth == 0 {
        return json!(null);
    }

    let entries = match vfs.list(path) {
        Ok(e) => e,
        Err(_) => return json!({ "path": path, "type": "file" }),
    };

    let children: Vec<Value> = entries
        .iter()
        .map(|name| {
            let full = if path == "/" {
                format!("/{name}")
            } else {
                format!("{path}/{name}")
            };
            if vfs.read(&full).is_ok() {
                json!({ "name": name, "path": full, "type": "file" })
            } else {
                let mut node = json!({ "name": name, "path": full, "type": "dir" });
                node["children"] = build_tree(vfs, &full, depth - 1);
                node
            }
        })
        .collect();

    json!({ "path": path, "type": "dir", "children": children })
}

// ── Phase 5.4 Advanced Agent Skills ───────────────────────────────────────────

fn skill_patch_file(call: &ToolCall, vfs: &mut Vfs) -> crate::error::Result<Value> {
    let path = str_arg(&call.arguments, "path")?;
    let target = str_arg(&call.arguments, "target_content")?;
    let replacement = str_arg(&call.arguments, "replacement_content")?;

    let bytes = vfs.read(path)?;
    let content = String::from_utf8_lossy(bytes).to_string();

    if !content.contains(target) {
        return Err(RunboxError::Runtime(
            "target_content not found in file".into(),
        ));
    }

    let patched = content.replace(target, replacement);
    vfs.write(path, patched.into_bytes())?;

    Ok(json!({ "path": path, "patched": true }))
}

fn skill_fetch_url(call: &ToolCall) -> crate::error::Result<Value> {
    let url = str_arg(&call.arguments, "url")?;
    // En WASM, `reqwest::blocking` no es soportado. Se delega al cliente o se devuelve metadata instruction.
    Ok(json!({
        "status": "pending_host_fetch",
        "url": url,
        "message": "En el entorno WASM la red directa es limitada a Promesas asíncronas. Por favor invoca esta tool usando la API delegada del dashboard."
    }))
}

fn skill_scaffold_project(call: &ToolCall, vfs: &mut Vfs) -> crate::error::Result<Value> {
    let template = str_arg(&call.arguments, "template")?;
    let base = call.arguments["path"].as_str().unwrap_or("/");

    let pkg_json = match template {
        "react" => r#"{"name":"react-app","dependencies":{"react":"latest","react-dom":"latest"}}"#,
        "express" => r#"{"name":"api","dependencies":{"express":"latest"}}"#,
        _ => r#"{"name":"demo","version":"1.0.0"}"#,
    };

    let p = if base == "/" {
        "/package.json".into()
    } else {
        format!("{}/package.json", base)
    };
    vfs.write(&p, pkg_json.into())?;

    Ok(json!({ "template": template, "scaffolded": true, "path": base }))
}

fn skill_debug_error(call: &ToolCall, vfs: &Vfs) -> crate::error::Result<Value> {
    let error_msg = str_arg(&call.arguments, "error_message")?;
    let file = call.arguments["related_file"].as_str().unwrap_or("/");
    let ctx = vfs
        .read(file)
        .map(|b| String::from_utf8_lossy(b).to_string())
        .unwrap_or_default();

    Ok(json!({
        "debug_context": format!("Analizando error: {}\nArchivo ({}) contiene:\n{}", error_msg, file, ctx),
        "hint": "Genera el fix o envía patch_file"
    }))
}

fn skill_explain_project(_call: &ToolCall, vfs: &Vfs) -> crate::error::Result<Value> {
    let entries = build_tree(vfs, "/", 3);
    Ok(json!({
        "structure": entries,
        "hint": "Esta es la arquitectura estructural del repositorio al nivel superior."
    }))
}

fn skill_refactor_code(call: &ToolCall, _vfs: &mut Vfs) -> crate::error::Result<Value> {
    let path = str_arg(&call.arguments, "path")?;
    let req = str_arg(&call.arguments, "instructions")?;
    Ok(
        json!({ "status": "acknowledged", "path": path, "request": req, "hint": "Genera los patches con patch_file o write_file siguiendo el código." }),
    )
}

fn skill_generate_tests(call: &ToolCall, _vfs: &Vfs) -> crate::error::Result<Value> {
    let path = str_arg(&call.arguments, "path")?;
    Ok(
        json!({ "status": "acknowledged", "path": path, "message": "Procede a usar write_file para el archivo .test.ts" }),
    )
}

// ── Agent Journal Skills ──────────────────────────────────────────────────────

fn skill_agent_memo(
    call: &ToolCall,
    journal: Option<&mut AgentJournal>,
    store: Option<&mut Box<dyn HistoryStore>>,
) -> crate::error::Result<Value> {
    let tool = str_arg(&call.arguments, "tool")?;
    let reason = str_arg(&call.arguments, "reason")?;
    let result_summary = call.arguments["result_summary"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let files_affected: Vec<String> = call.arguments["files_affected"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    match journal {
        Some(j) => {
            let id = j.record(tool, reason, result_summary, files_affected, 0);
            let mut persisted = 0usize;
            let mut persist_error = None;
            if let Some(s) = store {
                match crate::history_store::persist_pending(j, s) {
                    Ok(n) => persisted = n,
                    Err(e) => persist_error = Some(e),
                }
            }
            let mut out = json!({ "recorded": true, "id": id, "persisted": persisted });
            if let Some(e) = persist_error {
                out["persist_error"] = json!(e);
            }
            Ok(out)
        }
        None => Err(RunboxError::Runtime(
            "agent journal not available in this context".into(),
        )),
    }
}

fn skill_get_agent_journal(
    call: &ToolCall,
    journal: Option<&mut AgentJournal>,
) -> crate::error::Result<Value> {
    match journal {
        Some(j) => {
            let file = call.arguments["file"].as_str();
            let since_id = call.arguments["since_id"].as_u64();

            let entries: Vec<_> = match (file, since_id) {
                (Some(f), _) => j.for_file(f).into_iter().cloned().collect(),
                (None, Some(id)) => j.since(id).into_iter().cloned().collect(),
                _ => j.all().into_iter().cloned().collect(),
            };

            Ok(json!({ "entries": entries, "count": entries.len() }))
        }
        None => Err(RunboxError::Runtime(
            "agent journal not available in this context".into(),
        )),
    }
}

// ── History Search Skill ──────────────────────────────────────────────────────

fn skill_search_history(
    call: &ToolCall,
    journal: Option<&mut AgentJournal>,
    store: Option<&mut Box<dyn HistoryStore>>,
) -> crate::error::Result<Value> {
    let query_text = call.arguments["query"].as_str().map(|s| s.to_string());
    let tool_filter = call.arguments["tool"].as_str().map(|s| s.to_string());
    let file_filter = call.arguments["file"].as_str().map(|s| s.to_string());
    let limit = call.arguments["limit"].as_u64().unwrap_or(10) as usize;

    let mut all_results = vec![];

    // Buscar en journal en memoria
    if let Some(j) = journal {
        let local = j.search(
            query_text.as_deref().unwrap_or(""),
            tool_filter.as_deref(),
            file_filter.as_deref(),
            limit,
        );
        for m in local {
            all_results.push(serde_json::json!({
                "id": m.entry.id,
                "session_id": m.entry.session_id,
                "timestamp_ms": m.entry.timestamp_ms,
                "tool": m.entry.tool,
                "reason": m.entry.reason,
                "result_summary": m.entry.result_summary,
                "files_affected": m.entry.files_affected,
                "score": m.score,
                "snippet": m.snippet,
                "source": "current_session",
            }));
        }
    }

    // Buscar en store persistido (IndexedDB / SQLite)
    if let Some(s) = store {
        let query = SearchQuery {
            text: query_text.clone(),
            tool: tool_filter.clone(),
            file: file_filter.clone(),
            session_id: None,
            since_id: None,
            limit,
        };
        if let Ok(output) = s.search(&query) {
            for r in output.results {
                all_results.push(serde_json::json!({
                    "id": r.entry.id,
                    "session_id": r.entry.session_id,
                    "timestamp_ms": r.entry.timestamp_ms,
                    "tool": r.entry.tool,
                    "reason": r.entry.reason,
                    "result_summary": r.entry.result_summary,
                    "files_affected": r.entry.files_affected,
                    "score": r.score,
                    "snippet": r.snippet,
                    "source": "persisted",
                }));
            }
        }
    }

    // Ordenar por score descendente y limitar
    all_results.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&a["score"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_results.truncate(limit);

    Ok(json!({
        "results": all_results,
        "total": all_results.len(),
        "query": query_text,
    }))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn str_arg<'a>(args: &'a Value, key: &str) -> crate::error::Result<&'a str> {
    args[key]
        .as_str()
        .ok_or_else(|| RunboxError::Runtime(format!("missing argument: {key}")))
}

fn detect_package_manager(vfs: &Vfs) -> &'static str {
    if vfs.exists("/bun.lockb") {
        return "bun";
    }
    if vfs.exists("/pnpm-lock.yaml") {
        return "pnpm";
    }
    if vfs.exists("/yarn.lock") {
        return "yarn";
    }
    "npm"
}

// ── Preview skills ───────────────────────────────────────────────────────────

fn skill_preview_start(
    call: &ToolCall,
    preview: Option<&mut PreviewManager>,
) -> crate::error::Result<Value> {
    let preview = preview
        .ok_or_else(|| RunboxError::Runtime("preview not available in this context".into()))?;

    let mut config = crate::preview::PreviewConfig::default();

    // Apply optional overrides from arguments
    if let Some(domain) = call.arguments["domain"].as_str() {
        config.domain = Some(domain.to_string());
    }
    if let Some(port) = call.arguments["port"].as_u64() {
        config.port = u16::try_from(port).unwrap_or(3000);
    }
    if let Some(base) = call.arguments["base_path"].as_str() {
        config.base_path = base.to_string();
    }
    if let Some(https) = call.arguments["https"].as_bool() {
        config.https = https;
    }
    if let Some(spa) = call.arguments["spa"].as_bool() {
        config.spa = spa;
    }
    if let Some(lr) = call.arguments["live_reload"].as_bool() {
        config.live_reload = lr;
    }
    if let Some(title) = call.arguments["title"].as_str() {
        config.metadata.title = title.to_string();
    }
    if let Some(desc) = call.arguments["description"].as_str() {
        config.metadata.description = desc.to_string();
    }

    // Use current timestamp (0 as fallback — the WASM layer provides real time)
    let session = preview.start(config, 0);
    Ok(json!({
        "session_id": session.id,
        "status": "running",
        "url": session.base_url(),
        "share_url": session.share_url(),
    }))
}

fn skill_preview_stop(preview: Option<&mut PreviewManager>) -> crate::error::Result<Value> {
    let preview = preview
        .ok_or_else(|| RunboxError::Runtime("preview not available in this context".into()))?;

    preview.stop()?;
    Ok(json!({ "stopped": true }))
}

fn skill_preview_configure(
    call: &ToolCall,
    preview: Option<&mut PreviewManager>,
) -> crate::error::Result<Value> {
    let preview = preview
        .ok_or_else(|| RunboxError::Runtime("preview not available in this context".into()))?;

    let session = preview
        .current_mut()
        .ok_or_else(|| RunboxError::Runtime("no active preview session".into()))?;

    // Apply configuration updates
    if let Some(domain) = call.arguments["domain"].as_str() {
        session.config.domain = Some(domain.to_string());
    }
    if let Some(title) = call.arguments["title"].as_str() {
        session.config.metadata.title = title.to_string();
    }
    if let Some(desc) = call.arguments["description"].as_str() {
        session.config.metadata.description = desc.to_string();
    }
    if let Some(image) = call.arguments["image"].as_str() {
        session.config.metadata.image = image.to_string();
    }
    if let Some(favicon) = call.arguments["favicon"].as_str() {
        session.config.metadata.favicon = favicon.to_string();
    }
    if let Some(origins) = call.arguments["cors_origins"].as_array() {
        session.config.cors.allowed_origins = origins
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    if let Some(spa) = call.arguments["spa"].as_bool() {
        session.config.spa = spa;
    }
    if let Some(lr) = call.arguments["live_reload"].as_bool() {
        session.config.live_reload = lr;
    }

    Ok(json!({
        "configured": true,
        "url": session.base_url(),
        "domain": session.config.domain,
    }))
}

fn skill_preview_share(preview: Option<&mut PreviewManager>) -> crate::error::Result<Value> {
    let preview = preview
        .ok_or_else(|| RunboxError::Runtime("preview not available in this context".into()))?;

    let share_url = preview.share()?;
    let session = preview
        .current()
        .ok_or_else(|| RunboxError::Runtime("no active preview session".into()))?;

    Ok(json!({
        "share_url": share_url,
        "session_id": session.id,
        "domain": session.config.domain,
    }))
}
