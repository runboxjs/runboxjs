/// Agent Journal — registro estructurado de decisiones y razonamientos de agentes.
///
/// Cada entrada captura: qué tool se usó, por qué, qué archivos afectó,
/// y un resumen del resultado. Esto permite que otro agente (o el futuro tú)
/// entienda el historial de intenciones, no solo el historial de cambios.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Una entrada en el diario del agente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// ID incremental.
    pub id: u64,
    /// Timestamp en ms desde el inicio de la sesión.
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
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
            next_id: 0,
            session_id,
            pending_persist: Vec::new(),
        }
    }

    fn generate_session_id() -> String {
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
        format!("runbox-{:016x}", hasher.finish())
    }

    /// Retorna el ID de sesión actual.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Registra una nueva entrada en el diario.
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

        let entry = JournalEntry {
            id,
            timestamp_ms,
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
    pub fn search(&self, query: &str, tool_filter: Option<&str>, file_filter: Option<&str>, limit: usize) -> Vec<JournalSearchMatch> {
        let lower_query = query.to_lowercase();
        let has_query = !query.is_empty();
        let mut results: Vec<JournalSearchMatch> = vec![];

        for entry in &self.entries {
            // Filtros
            if let Some(t) = tool_filter {
                if !entry.tool.to_lowercase().contains(&t.to_lowercase()) {
                    continue;
                }
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

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    /// Serializa todas las entradas a JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.entries).unwrap_or_default()
    }

    /// Exporta en formato ctx-history-jsonl-v1
    pub fn to_jsonl(&self) -> String {
        self.entries
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "type": "event",
                    "version": 1,
                    "provider": "runbox",
                    "session_id": format!("runbox-{}", entry.session_id),
                    "event_id": format!("runbox-ev-{}", entry.id),
                    "timestamp_ms": entry.timestamp_ms,
                    "tool": entry.tool,
                    "reason": entry.reason,
                    "result_summary": entry.result_summary,
                    "files_affected": entry.files_affected,
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
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
        j.record("write_file", "Add authentication middleware", "written", vec!["/src/auth.ts".into()], 100);
        j.record("exec_command", "Run database migration", "exit 0", vec![], 200);
        j.record("write_file", "Fix CSS layout bug in header", "patched", vec!["/src/header.css".into()], 300);

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
        j.record("write_file", "Add login page", "created", vec!["/src/login.tsx".into()], 100);
        j.record("exec_command", "Install deps", "done", vec![], 200);
        j.record("write_file", "Fix login bug", "patched", vec!["/src/login.tsx".into()], 300);

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
        j.record("write_file", "init project", "done", vec!["/package.json".into()], 100);

        let jsonl = j.to_jsonl();
        assert!(jsonl.contains("\"provider\":\"runbox\""));
        assert!(jsonl.contains("\"tool\":\"write_file\""));
        assert!(jsonl.contains("\"reason\":\"init project\""));
        assert!(jsonl.contains("\"type\":\"event\""));
    }
}
