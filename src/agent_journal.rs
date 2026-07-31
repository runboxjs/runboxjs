/// Agent Journal — registro estructurado de decisiones y razonamientos de agentes.
///
/// Cada entrada captura: qué tool se usó, por qué, qué archivos afectó,
/// y un resumen del resultado. Esto permite que otro agente (o el futuro tú)
/// entienda el historial de intenciones, no solo el historial de cambios.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

/// Source id fijo para export ctx-history-jsonl-v1.
pub const CTX_SOURCE_ID: &str = "runbox-local";
/// Provider key para custom import de ctx.
pub const CTX_PROVIDER_KEY: &str = "runbox";
/// Formato de fuente declarado en el export.
pub const CTX_SOURCE_FORMAT: &str = "runbox-journal-v1";

/// Una entrada en el diario del agente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// ID incremental dentro de la sesión.
    pub id: u64,
    /// Timestamp absoluto en ms (Unix epoch). Si se pasa `0` a `record`, se rellena con now.
    pub timestamp_ms: u64,
    /// Nombre de la tool ejecutada (write_file, exec_command, patch_file, etc.)
    pub tool: String,
    /// Razonamiento o intención detrás de la acción (libre, escrito por el agente).
    pub reason: String,
    /// Resumen del resultado (stdout, error, etc.)
    pub result_summary: String,
    /// Archivos que fueron afectados por esta acción (si aplica).
    pub files_affected: Vec<String>,
    /// ID de la sesión a la que pertenece esta entrada.
    pub session_id: String,
}

/// Unix epoch millis.
pub fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ms_to_rfc3339(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into())
}

/// Exporta entradas al formato público `ctx-history-jsonl-v1`.
pub fn entries_to_ctx_jsonl(entries: &[JournalEntry]) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(
        serde_json::json!({
            "record_type": "manifest",
            "schema_version": "ctx-history-jsonl-v1",
            "metadata": { "exporter": "runbox", "source_format": CTX_SOURCE_FORMAT }
        })
        .to_string(),
    );

    lines.push(
        serde_json::json!({
            "record_type": "source",
            "source_id": CTX_SOURCE_ID,
            "provider_key": CTX_PROVIDER_KEY,
            "source_format": CTX_SOURCE_FORMAT,
            "metadata": { "kind": "agent_journal" }
        })
        .to_string(),
    );

    let mut by_session: BTreeMap<&str, Vec<&JournalEntry>> = BTreeMap::new();
    for entry in entries {
        by_session
            .entry(entry.session_id.as_str())
            .or_default()
            .push(entry);
    }

    for (session_id, session_entries) in by_session {
        let started_ms = session_entries
            .iter()
            .map(|e| e.timestamp_ms)
            .min()
            .unwrap_or(0);
        let ended_ms = session_entries
            .iter()
            .map(|e| e.timestamp_ms)
            .max()
            .unwrap_or(started_ms);

        lines.push(
            serde_json::json!({
                "record_type": "session",
                "source_id": CTX_SOURCE_ID,
                "session_id": session_id,
                "native_session_id": session_id,
                "started_at": ms_to_rfc3339(started_ms),
                "ended_at": ms_to_rfc3339(ended_ms),
                "agent_type": "primary",
                "role_hint": "developer",
                "is_primary": true,
                "status": "completed",
                "metadata": { "provider": CTX_PROVIDER_KEY }
            })
            .to_string(),
        );

        let mut touch_index: u64 = 0;
        for entry in session_entries {
            let preview = if entry.reason.len() > 240 {
                format!("{}…", entry.reason.chars().take(240).collect::<String>())
            } else {
                entry.reason.clone()
            };
            let occurred_at = ms_to_rfc3339(entry.timestamp_ms);

            lines.push(
                serde_json::json!({
                    "record_type": "event",
                    "source_id": CTX_SOURCE_ID,
                    "session_id": session_id,
                    "event_index": entry.id,
                    "event_id": format!("runbox-ev-{}-{}", session_id, entry.id),
                    "event_type": "tool_use",
                    "role": "assistant",
                    "occurred_at": &occurred_at,
                    "preview": preview,
                    "payload": {
                        "tool": entry.tool,
                        "reason": entry.reason,
                        "result_summary": entry.result_summary,
                        "files_affected": entry.files_affected,
                    },
                    "native_cursor": format!("entry:{}", entry.id),
                    "metadata": { "tool": entry.tool }
                })
                .to_string(),
            );

            for path in &entry.files_affected {
                lines.push(
                    serde_json::json!({
                        "record_type": "file_touch",
                        "source_id": CTX_SOURCE_ID,
                        "session_id": session_id,
                        "touch_index": touch_index,
                        "event_index": entry.id,
                        "path": path,
                        "change_kind": "modified",
                        "confidence": "high",
                        "occurred_at": &occurred_at,
                    })
                    .to_string(),
                );
                touch_index += 1;
            }
        }
    }

    lines.join("\n")
}

/// Resultado de búsqueda simple (in-memory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSearchMatch {
    pub entry: JournalEntry,
    pub score: f64,
    pub snippet: String,
}

/// Diario del agente — buffer circular de entradas.
pub struct AgentJournal {
    entries: VecDeque<JournalEntry>,
    capacity: usize,
    next_id: u64,
    session_id: String,
    session_started_at_ms: u64,
    pending_persist: Vec<JournalEntry>,
}

impl Default for AgentJournal {
    fn default() -> Self {
        Self::new(500)
    }
}

impl AgentJournal {
    pub fn new(capacity: usize) -> Self {
        let session_id = Self::generate_session_id();
        let session_started_at_ms = unix_now_ms();
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
            next_id: 0,
            session_id,
            session_started_at_ms,
            pending_persist: Vec::new(),
        }
    }

    fn generate_session_id() -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        unix_now_ms().hash(&mut hasher);
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
        format!("runbox-{:016x}", hasher.finish())
    }

    /// Retorna el ID de sesión actual.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Inicio de sesión en Unix epoch millis.
    pub fn session_started_at_ms(&self) -> u64 {
        self.session_started_at_ms
    }

    /// Registra una nueva entrada en el diario.
    /// Si `timestamp_ms == 0`, usa el reloj del sistema (Unix epoch ms).
    pub fn record(
        &mut self,
        tool: impl Into<String>,
        reason: impl Into<String>,
        result_summary: impl Into<String>,
        files_affected: Vec<String>,
        timestamp_ms: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }

        let ts = if timestamp_ms == 0 {
            unix_now_ms()
        } else {
            timestamp_ms
        };

        let entry = JournalEntry {
            id,
            timestamp_ms: ts,
            tool: tool.into(),
            reason: reason.into(),
            result_summary: result_summary.into(),
            files_affected,
            session_id: self.session_id.clone(),
        };

        self.pending_persist.push(entry.clone());
        self.entries.push_back(entry);

        id
    }

    /// Retorna todas las entradas que no han sido persistidas aún y las limpia de la cola.
    pub fn drain_pending(&mut self) -> Vec<JournalEntry> {
        std::mem::take(&mut self.pending_persist)
    }

    /// Retorna todas las entradas.
    pub fn all(&self) -> Vec<&JournalEntry> {
        self.entries.iter().collect()
    }

    /// Retorna entradas con id > since_id.
    pub fn since(&self, since_id: u64) -> Vec<&JournalEntry> {
        self.entries.iter().filter(|e| e.id > since_id).collect()
    }

    /// Retorna entradas que mencionan un archivo específico.
    pub fn for_file(&self, path: &str) -> Vec<&JournalEntry> {
        self.entries
            .iter()
            .filter(|e| e.files_affected.iter().any(|f| f == path))
            .collect()
    }

    /// Búsqueda full-text sobre las entradas en memoria.
    pub fn search(
        &self,
        query: &str,
        tool_filter: Option<&str>,
        file_filter: Option<&str>,
        limit: usize,
    ) -> Vec<JournalSearchMatch> {
        let lower_query = query.to_lowercase();
        let has_query = !query.is_empty();
        let mut results: Vec<JournalSearchMatch> = vec![];

        for entry in &self.entries {
            // Filtros
            if let Some(t) = tool_filter
                && !entry.tool.to_lowercase().contains(&t.to_lowercase())
            {
                continue;
            }
            if let Some(f) = file_filter {
                let all_files = entry.files_affected.join(" ").to_lowercase();
                if !all_files.contains(&f.to_lowercase()) {
                    continue;
                }
            }

            // Scoring
            let mut score = if has_query { 0.0 } else { 1.0 };
            let mut matched = false;

            if has_query {
                let reason_lower = entry.reason.to_lowercase();
                let result_lower = entry.result_summary.to_lowercase();
                let tool_lower = entry.tool.to_lowercase();
                let files_lower = entry.files_affected.join(" ").to_lowercase();

                if reason_lower.contains(&lower_query) {
                    score += 3.0;
                    matched = true;
                }
                if result_lower.contains(&lower_query) {
                    score += 2.0;
                    matched = true;
                }
                if tool_lower.contains(&lower_query) {
                    score += 2.0;
                    matched = true;
                }
                if files_lower.contains(&lower_query) {
                    score += 1.0;
                    matched = true;
                }

                if !matched {
                    continue;
                }
            }

            let snippet = if has_query {
                build_snippet(&entry.reason, &lower_query)
            } else {
                entry.reason.chars().take(200).collect()
            };

            results.push(JournalSearchMatch {
                entry: entry.clone(),
                score,
                snippet,
            });
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
    }

    /// Serializa todas las entradas a JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.entries).unwrap_or_default()
    }

    /// Exporta en formato `ctx-history-jsonl-v1` (importable por `ctx import`).
    pub fn to_jsonl(&self) -> String {
        let entries: Vec<JournalEntry> = self.entries.iter().cloned().collect();
        entries_to_ctx_jsonl(&entries)
    }

    /// Número de entradas actuales.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Número de entradas pendientes de persistir.
    pub fn pending_count(&self) -> usize {
        self.pending_persist.len()
    }
}

fn build_snippet(text: &str, query: &str) -> String {
    if let Some(pos) = text.to_lowercase().find(query) {
        let start = pos.saturating_sub(60);
        let end = (pos + query.len() + 60).min(text.len());
        let snippet = &text[start..end];
        if start > 0 {
            format!("...{}...", snippet)
        } else {
            snippet.to_string()
        }
    } else {
        text.chars().take(200).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_retrieve() {
        let mut j = AgentJournal::new(10);
        j.record(
            "write_file",
            "fix auth bug",
            "written 240 bytes",
            vec!["/src/auth.ts".into()],
            100,
        );
        j.record(
            "exec_command",
            "run tests to verify fix",
            "exit 0",
            vec![],
            200,
        );

        assert_eq!(j.len(), 2);
        assert_eq!(j.all()[0].tool, "write_file");
        assert_eq!(j.all()[0].session_id.len(), 23); // "runbox-" + 16 hex chars
        assert_eq!(j.since(0).len(), 1);
        assert_eq!(j.for_file("/src/auth.ts").len(), 1);
    }

    #[test]
    fn capacity_evicts_oldest() {
        let mut j = AgentJournal::new(3);
        for i in 0..5u64 {
            j.record("t", "r", "ok", vec![], i * 10);
        }
        assert_eq!(j.len(), 3);
        assert_eq!(j.all()[0].id, 2);
    }

    #[test]
    fn pending_persist_tracking() {
        let mut j = AgentJournal::new(10);
        assert_eq!(j.pending_count(), 0);

        j.record("write_file", "add feature", "done", vec![], 100);
        assert_eq!(j.pending_count(), 1);

        j.record("exec_command", "build", "ok", vec![], 200);
        assert_eq!(j.pending_count(), 2);

        let drained = j.drain_pending();
        assert_eq!(drained.len(), 2);
        assert_eq!(j.pending_count(), 0);
    }

    #[test]
    fn search_empty() {
        let j = AgentJournal::new(10);
        let results = j.search("anything", None, None, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn search_by_reason() {
        let mut j = AgentJournal::new(50);
        j.record(
            "write_file",
            "Add authentication middleware",
            "written",
            vec!["/src/auth.ts".into()],
            100,
        );
        j.record(
            "exec_command",
            "Run database migration",
            "exit 0",
            vec![],
            200,
        );
        j.record(
            "write_file",
            "Fix CSS layout bug in header",
            "patched",
            vec!["/src/header.css".into()],
            300,
        );

        let results = j.search("migration", None, None, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.tool, "exec_command");

        let results = j.search("auth", None, None, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.tool, "write_file");
    }

    #[test]
    fn search_with_filters() {
        let mut j = AgentJournal::new(50);
        j.record(
            "write_file",
            "Add login page",
            "created",
            vec!["/src/login.tsx".into()],
            100,
        );
        j.record("exec_command", "Install deps", "done", vec![], 200);
        j.record(
            "write_file",
            "Fix login bug",
            "patched",
            vec!["/src/login.tsx".into()],
            300,
        );

        // Filter by tool
        let results = j.search("", Some("exec_command"), None, 10);
        assert_eq!(results.len(), 1);

        // Filter by file
        let results = j.search("", None, Some("/src/login.tsx"), 10);
        assert_eq!(results.len(), 2);

        // Combined
        let results = j.search("login", Some("write_file"), None, 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_limit() {
        let mut j = AgentJournal::new(50);
        for i in 0..10u64 {
            j.record("tool", "test entry", "ok", vec![], i * 10);
        }

        let results = j.search("test", None, None, 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn to_jsonl_format() {
        let mut j = AgentJournal::new(10);
        j.record(
            "write_file",
            "init project",
            "done",
            vec!["/package.json".into()],
            1_700_000_000_000,
        );

        let jsonl = j.to_jsonl();
        assert!(jsonl.contains("\"record_type\":\"manifest\""));
        assert!(jsonl.contains("\"schema_version\":\"ctx-history-jsonl-v1\""));
        assert!(jsonl.contains("\"record_type\":\"source\""));
        assert!(jsonl.contains("\"provider_key\":\"runbox\""));
        assert!(jsonl.contains("\"record_type\":\"session\""));
        assert!(jsonl.contains("\"record_type\":\"event\""));
        assert!(jsonl.contains("\"record_type\":\"file_touch\""));
        assert!(jsonl.contains("\"tool\":\"write_file\""));
        assert!(jsonl.contains("\"reason\":\"init project\""));
        assert!(jsonl.contains("/package.json"));
    }

    #[test]
    fn record_zero_timestamp_uses_unix_now() {
        let mut j = AgentJournal::new(10);
        j.record("t", "r", "ok", vec![], 0);
        let ts = j.all()[0].timestamp_ms;
        assert!(ts > 1_000_000_000_000, "expected unix epoch ms, got {ts}");
    }
}
