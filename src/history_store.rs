use crate::agent_journal::{JournalEntry, entries_to_ctx_jsonl};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Query para búsqueda en el historial
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: Option<String>,
    pub tool: Option<String>,
    pub file: Option<String>,
    pub session_id: Option<String>,
    pub since_id: Option<u64>,
    pub limit: usize,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: None,
            tool: None,
            file: None,
            session_id: None,
            since_id: None,
            limit: 20,
        }
    }
}

/// Resultado de búsqueda rankeado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub entry: JournalEntry,
    pub score: f64,
    pub snippet: String,
    pub matched_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOutput {
    pub results: Vec<SearchResult>,
    pub total: usize,
    pub query: SearchQuery,
}

/// Interfaz abstracta de almacenamiento de historial
pub trait HistoryStore {
    fn save_journal_entry(&mut self, entry: &JournalEntry) -> Result<(), String>;
    fn save_journal_entries(&mut self, entries: &[JournalEntry]) -> Result<(), String>;
    fn get_journal_entries(&self, since_id: u64, limit: usize)
    -> Result<Vec<JournalEntry>, String>;
    fn search(&self, query: &SearchQuery) -> Result<SearchOutput, String>;
    fn export_jsonl(&self) -> Result<String, String>;
    fn clear(&mut self) -> Result<(), String>;
}

/// Backend nulo (sin persistencia). Usado como fallback.
pub struct NullStore;

impl HistoryStore for NullStore {
    fn save_journal_entry(&mut self, _entry: &JournalEntry) -> Result<(), String> {
        Ok(())
    }
    fn save_journal_entries(&mut self, _entries: &[JournalEntry]) -> Result<(), String> {
        Ok(())
    }
    fn get_journal_entries(
        &self,
        _since_id: u64,
        _limit: usize,
    ) -> Result<Vec<JournalEntry>, String> {
        Ok(vec![])
    }
    fn search(&self, _query: &SearchQuery) -> Result<SearchOutput, String> {
        Ok(SearchOutput {
            results: vec![],
            total: 0,
            query: SearchQuery::default(),
        })
    }
    fn export_jsonl(&self) -> Result<String, String> {
        Ok(String::new())
    }
    fn clear(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Ruta por defecto del historial SQLite nativo.
/// Override: `RUNBOX_HISTORY_DB=/path/to/history.sqlite`
pub fn default_history_db_path() -> PathBuf {
    if let Ok(custom) = std::env::var("RUNBOX_HISTORY_DB")
        && !custom.trim().is_empty()
    {
        return PathBuf::from(custom);
    }

    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());

    PathBuf::from(home).join(".runbox").join("history.sqlite")
}

// ── Backend nativo: SQLite ──────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub mod native_sqlite {
    use super::*;
    use rusqlite::{Connection, params};

    pub struct SqliteStore {
        conn: Connection,
    }

    impl SqliteStore {
        pub fn new(path: &str) -> Result<Self, String> {
            if let Some(parent) = std::path::Path::new(path).parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create history dir: {e}"))?;
            }
            let conn = Connection::open(path).map_err(|e| format!("failed to open DB: {e}"))?;
            let store = Self { conn };
            store.init_schema()?;
            Ok(store)
        }

        pub fn in_memory() -> Result<Self, String> {
            let conn = Connection::open_in_memory()
                .map_err(|e| format!("failed to open in-memory DB: {e}"))?;
            let store = Self { conn };
            store.init_schema()?;
            Ok(store)
        }

        pub fn from_connection(conn: Connection) -> Result<Self, String> {
            let store = Self { conn };
            store.init_schema()?;
            Ok(store)
        }

        fn init_schema(&self) -> Result<(), String> {
            // Si existe el schema viejo (PK = id), resetear para soportar multi-sesión.
            let has_entry_id = self
                .conn
                .prepare("SELECT entry_id FROM runbox_journal LIMIT 0")
                .is_ok();
            let table_exists = self
                .conn
                .prepare(
                    "SELECT 1 FROM sqlite_master WHERE type='table' AND name='runbox_journal' LIMIT 1",
                )
                .and_then(|mut stmt| stmt.exists([]))
                .unwrap_or(false);

            if table_exists && !has_entry_id {
                self.conn
                    .execute_batch(
                        "DROP TABLE IF EXISTS journal_fts; DROP TABLE IF EXISTS runbox_journal;",
                    )
                    .map_err(|e| format!("schema migrate error: {e}"))?;
            }

            self.conn
                .execute_batch(
                    "
                CREATE TABLE IF NOT EXISTS runbox_journal (
                    pk INTEGER PRIMARY KEY AUTOINCREMENT,
                    entry_id INTEGER NOT NULL,
                    session_id TEXT NOT NULL,
                    timestamp_ms INTEGER NOT NULL,
                    tool TEXT NOT NULL,
                    reason TEXT NOT NULL,
                    result_summary TEXT NOT NULL DEFAULT '',
                    files_affected TEXT NOT NULL DEFAULT '[]',
                    UNIQUE(session_id, entry_id)
                );

                CREATE INDEX IF NOT EXISTS idx_journal_session ON runbox_journal(session_id);
                CREATE INDEX IF NOT EXISTS idx_journal_tool ON runbox_journal(tool);
                CREATE INDEX IF NOT EXISTS idx_journal_timestamp ON runbox_journal(timestamp_ms);

                CREATE VIRTUAL TABLE IF NOT EXISTS journal_fts USING fts5(
                    tool, reason, result_summary, files_affected,
                    content='runbox_journal',
                    content_rowid='pk'
                );
                ",
                )
                .map_err(|e| format!("schema init error: {e}"))?;

            self.conn
                .execute(
                    "INSERT INTO journal_fts(journal_fts) VALUES ('rebuild')",
                    [],
                )
                .ok();
            Ok(())
        }

        fn upsert_entry(conn: &Connection, entry: &JournalEntry) -> Result<(), String> {
            let files_json =
                serde_json::to_string(&entry.files_affected).unwrap_or_else(|_| "[]".to_string());

            conn.execute(
                "INSERT INTO runbox_journal (entry_id, session_id, timestamp_ms, tool, reason, result_summary, files_affected)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(session_id, entry_id) DO UPDATE SET
                    timestamp_ms=excluded.timestamp_ms,
                    tool=excluded.tool,
                    reason=excluded.reason,
                    result_summary=excluded.result_summary,
                    files_affected=excluded.files_affected",
                params![
                    entry.id as i64,
                    entry.session_id,
                    entry.timestamp_ms as i64,
                    entry.tool,
                    entry.reason,
                    entry.result_summary,
                    files_json,
                ],
            )
            .map_err(|e| format!("insert error: {e}"))?;

            let pk: i64 = conn
                .query_row(
                    "SELECT pk FROM runbox_journal WHERE session_id = ?1 AND entry_id = ?2",
                    params![entry.session_id, entry.id as i64],
                    |row| row.get(0),
                )
                .map_err(|e| format!("pk lookup error: {e}"))?;

            let _ = conn.execute(
                "INSERT INTO journal_fts(journal_fts, rowid) VALUES('delete', ?1)",
                params![pk],
            );
            conn.execute(
                "INSERT INTO journal_fts(rowid, tool, reason, result_summary, files_affected)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    pk,
                    entry.tool,
                    entry.reason,
                    entry.result_summary,
                    files_json,
                ],
            )
            .map_err(|e| format!("fts insert error: {e}"))?;

            Ok(())
        }

        fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<JournalEntry> {
            let files_str: String = row.get(6)?;
            let files: Vec<String> = serde_json::from_str(&files_str).unwrap_or_default();
            Ok(JournalEntry {
                id: row.get::<_, i64>(0)? as u64,
                session_id: row.get(1)?,
                timestamp_ms: row.get::<_, i64>(2)? as u64,
                tool: row.get(3)?,
                reason: row.get(4)?,
                result_summary: row.get(5)?,
                files_affected: files,
            })
        }
    }

    impl HistoryStore for SqliteStore {
        fn save_journal_entry(&mut self, entry: &JournalEntry) -> Result<(), String> {
            Self::upsert_entry(&self.conn, entry)
        }

        fn save_journal_entries(&mut self, entries: &[JournalEntry]) -> Result<(), String> {
            if entries.is_empty() {
                return Ok(());
            }
            let tx = self
                .conn
                .transaction()
                .map_err(|e| format!("tx begin error: {e}"))?;
            for entry in entries {
                Self::upsert_entry(&tx, entry)?;
            }
            tx.commit().map_err(|e| format!("tx commit error: {e}"))?;
            Ok(())
        }

        fn get_journal_entries(
            &self,
            since_id: u64,
            limit: usize,
        ) -> Result<Vec<JournalEntry>, String> {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT entry_id, session_id, timestamp_ms, tool, reason, result_summary, files_affected
                     FROM runbox_journal WHERE entry_id > ?1 ORDER BY timestamp_ms ASC, entry_id ASC LIMIT ?2",
                )
                .map_err(|e| format!("prepare error: {e}"))?;

            let rows = stmt
                .query_map(params![since_id as i64, limit as i64], Self::row_to_entry)
                .map_err(|e| format!("query error: {e}"))?;

            let mut entries = vec![];
            for row in rows {
                entries.push(row.map_err(|e| format!("row error: {e}"))?);
            }
            Ok(entries)
        }

        fn search(&self, query: &SearchQuery) -> Result<SearchOutput, String> {
            let limit = query.limit.min(200);

            if let Some(ref text) = query.text {
                if text.is_empty() {
                    return Ok(SearchOutput {
                        results: vec![],
                        total: 0,
                        query: query.clone(),
                    });
                }

                let sanitized: String = text
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c.is_whitespace() || c == '-' || c == '_' {
                            c
                        } else {
                            ' '
                        }
                    })
                    .collect();

                let terms: Vec<&str> = sanitized.split_whitespace().collect();
                if terms.is_empty() {
                    return Ok(SearchOutput {
                        results: vec![],
                        total: 0,
                        query: query.clone(),
                    });
                }

                let fts_query = terms
                    .iter()
                    .map(|t| format!("\"{}\"", t.replace('"', "")))
                    .collect::<Vec<_>>()
                    .join(" OR ");

                let sql = format!(
                    "SELECT j.entry_id, j.session_id, j.timestamp_ms, j.tool, j.reason, j.result_summary, j.files_affected,
                            rank
                     FROM journal_fts f
                     JOIN runbox_journal j ON j.pk = f.rowid
                     WHERE journal_fts MATCH ?1
                     {}
                     ORDER BY rank ASC
                     LIMIT ?2",
                    build_extra_filters(query),
                );

                let mut stmt = self
                    .conn
                    .prepare(&sql)
                    .map_err(|e| format!("fts prepare error: {e}"))?;

                let rows = stmt
                    .query_map(params![fts_query, limit as i64], |row| {
                        let entry = Self::row_to_entry(row)?;
                        let score: f64 = 1.0 - (row.get::<_, f64>(7).unwrap_or(0.0) / 100.0);
                        Ok((entry, score))
                    })
                    .map_err(|e| format!("fts query error: {e}"))?;

                let mut results = vec![];
                for row in rows {
                    let (entry, score) = row.map_err(|e| format!("row error: {e}"))?;
                    let snippet = crate::history_store::build_snippet_inner(
                        &entry,
                        query.text.as_deref().unwrap_or(""),
                    );
                    results.push(SearchResult {
                        entry,
                        score: score.clamp(0.0, 1.0),
                        snippet,
                        matched_fields: vec!["reason".to_string()],
                    });
                }

                let total = results.len();
                return Ok(SearchOutput {
                    results,
                    total,
                    query: query.clone(),
                });
            }

            let mut conditions = vec!["1=1".to_string()];
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];

            if let Some(ref tool) = query.tool {
                param_values.push(Box::new(tool.clone()));
                conditions.push(format!("tool = ?{}", param_values.len()));
            }
            if let Some(ref file) = query.file {
                param_values.push(Box::new(format!("%{}%", file)));
                conditions.push(format!("files_affected LIKE ?{}", param_values.len()));
            }
            if let Some(ref sid) = query.session_id {
                param_values.push(Box::new(sid.clone()));
                conditions.push(format!("session_id = ?{}", param_values.len()));
            }
            if let Some(sid) = query.since_id {
                param_values.push(Box::new(sid as i64));
                conditions.push(format!("entry_id > ?{}", param_values.len()));
            }

            param_values.push(Box::new(limit as i64));

            let sql = format!(
                "SELECT entry_id, session_id, timestamp_ms, tool, reason, result_summary, files_affected
                 FROM runbox_journal
                 WHERE {}
                 ORDER BY timestamp_ms DESC
                 LIMIT ?{}",
                conditions.join(" AND "),
                param_values.len(),
            );

            let mut stmt = self
                .conn
                .prepare(&sql)
                .map_err(|e| format!("prepare error: {e}"))?;

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p.as_ref()).collect();

            let rows = stmt
                .query_map(params_refs.as_slice(), Self::row_to_entry)
                .map_err(|e| format!("query error: {e}"))?;

            let mut entries = vec![];
            for row in rows {
                entries.push(row.map_err(|e| format!("row error: {e}"))?);
            }

            let results: Vec<SearchResult> = entries
                .into_iter()
                .map(|entry| SearchResult {
                    snippet: entry.reason.chars().take(200).collect(),
                    score: 1.0,
                    matched_fields: vec![],
                    entry,
                })
                .collect();

            let total = results.len();
            Ok(SearchOutput {
                results,
                total,
                query: query.clone(),
            })
        }

        fn export_jsonl(&self) -> Result<String, String> {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT entry_id, session_id, timestamp_ms, tool, reason, result_summary, files_affected
                     FROM runbox_journal ORDER BY timestamp_ms ASC, entry_id ASC",
                )
                .map_err(|e| format!("prepare error: {e}"))?;

            let rows = stmt
                .query_map([], Self::row_to_entry)
                .map_err(|e| format!("query error: {e}"))?;

            let mut entries = vec![];
            for row in rows {
                entries.push(row.map_err(|e| format!("row error: {e}"))?);
            }
            Ok(entries_to_ctx_jsonl(&entries))
        }

        fn clear(&mut self) -> Result<(), String> {
            self.conn
                .execute_batch(
                    "DELETE FROM runbox_journal; INSERT INTO journal_fts(journal_fts) VALUES ('rebuild');",
                )
                .map_err(|e| format!("clear error: {e}"))?;
            Ok(())
        }
    }

    fn build_extra_filters(query: &SearchQuery) -> String {
        let mut filters = vec![];
        if let Some(ref tool) = query.tool {
            let escaped = tool.replace('\'', "''");
            filters.push(format!("AND j.tool = '{}'", escaped));
        }
        if let Some(ref file) = query.file {
            let escaped = file.replace('\'', "''");
            filters.push(format!("AND j.files_affected LIKE '%{}%'", escaped));
        }
        if let Some(ref sid) = query.session_id {
            let escaped = sid.replace('\'', "''");
            filters.push(format!("AND j.session_id = '{}'", escaped));
        }
        filters.join(" ")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn persist_two_sessions_without_id_collision() {
            let mut store = SqliteStore::in_memory().unwrap();
            let a = JournalEntry {
                id: 0,
                timestamp_ms: 100,
                tool: "write_file".into(),
                reason: "session a".into(),
                result_summary: "ok".into(),
                files_affected: vec!["/a.ts".into()],
                session_id: "runbox-aaa".into(),
            };
            let b = JournalEntry {
                id: 0,
                timestamp_ms: 200,
                tool: "write_file".into(),
                reason: "session b".into(),
                result_summary: "ok".into(),
                files_affected: vec!["/b.ts".into()],
                session_id: "runbox-bbb".into(),
            };
            store.save_journal_entry(&a).unwrap();
            store.save_journal_entry(&b).unwrap();

            let out = store
                .search(&SearchQuery {
                    text: Some("session".into()),
                    limit: 10,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(out.results.len(), 2);

            let jsonl = store.export_jsonl().unwrap();
            assert!(jsonl.contains("ctx-history-jsonl-v1"));
            assert!(jsonl.contains("runbox-aaa"));
            assert!(jsonl.contains("runbox-bbb"));
            assert!(jsonl.contains("\"record_type\":\"file_touch\""));
        }

        #[test]
        fn default_db_path_under_runbox_dir() {
            let path = default_history_db_path();
            assert!(
                path.ends_with("history.sqlite")
                    || path.to_string_lossy().contains("history.sqlite")
            );
        }
    }
}

pub(crate) fn build_snippet_inner(entry: &JournalEntry, query: &str) -> String {
    let lower = query.to_lowercase();
    let fields = [&entry.reason, &entry.result_summary, &entry.tool];
    for field in fields {
        if let Some(pos) = field.to_lowercase().find(&lower) {
            let start = pos.saturating_sub(60);
            let end = (pos + query.len() + 60).min(field.len());
            let snippet = &field[start..end];
            if start > 0 {
                return format!("...{}...", snippet);
            }
            return snippet.to_string();
        }
    }
    entry.reason.chars().take(200).collect()
}

/// Single-entry helper; prefer `entries_to_ctx_jsonl` for full corpora.
#[allow(dead_code)]
pub(crate) fn entry_to_jsonl(entry: &JournalEntry) -> String {
    entries_to_ctx_jsonl(std::slice::from_ref(entry))
}

// ── Backend WASM: localStorage bridge ───────────────────────────────────────

#[cfg(target_arch = "wasm32")]
pub mod wasm_storage {
    use super::*;

    const STORAGE_KEY: &str = "runbox_journal_all";

    fn storage() -> Option<web_sys::Storage> {
        let window = web_sys::window()?;
        window.local_storage().ok()?
    }

    pub struct LocalStorageStore;

    impl LocalStorageStore {
        pub fn new() -> Self {
            Self
        }

        fn read_all() -> Result<Vec<JournalEntry>, String> {
            let storage = storage().ok_or("localStorage not available")?;
            let raw = storage
                .get_item(STORAGE_KEY)
                .map_err(|e| format!("read error: {:?}", e))?
                .unwrap_or_default();
            if raw.is_empty() {
                return Ok(vec![]);
            }
            serde_json::from_str(&raw).map_err(|e| format!("parse error: {e}"))
        }

        fn write_all(entries: &[JournalEntry]) -> Result<(), String> {
            let storage = storage().ok_or("localStorage not available")?;
            let json =
                serde_json::to_string(entries).map_err(|e| format!("serialize error: {e}"))?;
            storage
                .set_item(STORAGE_KEY, &json)
                .map_err(|e| format!("write error: {:?}", e))
        }
    }

    impl HistoryStore for LocalStorageStore {
        fn save_journal_entry(&mut self, entry: &JournalEntry) -> Result<(), String> {
            let mut all = Self::read_all()?;
            // Upsert by (session_id, id) to avoid duplicates across persists.
            if let Some(existing) = all
                .iter_mut()
                .find(|e| e.session_id == entry.session_id && e.id == entry.id)
            {
                *existing = entry.clone();
            } else {
                all.push(entry.clone());
            }
            Self::write_all(&all)
        }

        fn save_journal_entries(&mut self, entries: &[JournalEntry]) -> Result<(), String> {
            if entries.is_empty() {
                return Ok(());
            }
            let mut all = Self::read_all()?;
            for entry in entries {
                if let Some(existing) = all
                    .iter_mut()
                    .find(|e| e.session_id == entry.session_id && e.id == entry.id)
                {
                    *existing = entry.clone();
                } else {
                    all.push(entry.clone());
                }
            }
            Self::write_all(&all)
        }

        fn get_journal_entries(
            &self,
            since_id: u64,
            limit: usize,
        ) -> Result<Vec<JournalEntry>, String> {
            let all = Self::read_all()?;
            Ok(all
                .into_iter()
                .filter(|e| e.id > since_id)
                .take(limit)
                .collect())
        }

        fn search(&self, query: &SearchQuery) -> Result<SearchOutput, String> {
            let all = Self::read_all()?;
            let limit = query.limit.min(200);
            let lower_query = query.text.as_ref().map(|t| t.to_lowercase());
            let tool_filter = query.tool.as_ref().map(|t| t.to_lowercase());
            let file_filter = query.file.as_ref().map(|f| f.to_lowercase());
            let has_text = lower_query.is_some() && !lower_query.as_ref().unwrap().is_empty();

            let mut results: Vec<SearchResult> = vec![];

            for entry in &all {
                if let Some(ref sid) = query.session_id {
                    if entry.session_id != *sid {
                        continue;
                    }
                }
                if let Some(ref tool) = tool_filter {
                    if !entry.tool.to_lowercase().contains(tool) {
                        continue;
                    }
                }
                if let Some(ref file) = file_filter {
                    let files = entry.files_affected.join(" ").to_lowercase();
                    if !files.contains(file) {
                        continue;
                    }
                }
                if let Some(sid) = query.since_id {
                    if entry.id <= sid {
                        continue;
                    }
                }

                let mut score = if has_text { 0.0 } else { 1.0 };
                let mut matched = false;
                let mut matched_fields = vec![];

                if has_text {
                    let q = lower_query.as_ref().unwrap();
                    if entry.reason.to_lowercase().contains(q) {
                        score += 3.0;
                        matched = true;
                        matched_fields.push("reason".to_string());
                    }
                    if entry.result_summary.to_lowercase().contains(q) {
                        score += 2.0;
                        matched = true;
                        matched_fields.push("result_summary".to_string());
                    }
                    if entry.tool.to_lowercase().contains(q) {
                        score += 2.0;
                        matched = true;
                        matched_fields.push("tool".to_string());
                    }
                    let files = entry.files_affected.join(" ").to_lowercase();
                    if files.contains(q) {
                        score += 1.0;
                        matched = true;
                        matched_fields.push("files".to_string());
                    }
                    if !matched {
                        continue;
                    }
                }

                let snippet = if has_text {
                    let q = lower_query.as_ref().unwrap();
                    crate::history_store::build_snippet_inner(entry, q)
                } else {
                    entry.reason.chars().take(200).collect()
                };

                results.push(SearchResult {
                    entry: entry.clone(),
                    score,
                    snippet,
                    matched_fields,
                });
            }

            results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            results.truncate(limit);

            let total = results.len();
            Ok(SearchOutput {
                results,
                total,
                query: query.clone(),
            })
        }

        fn export_jsonl(&self) -> Result<String, String> {
            let all = Self::read_all()?;
            Ok(entries_to_ctx_jsonl(&all))
        }

        fn clear(&mut self) -> Result<(), String> {
            let storage = storage().ok_or("localStorage not available")?;
            storage
                .remove_item(STORAGE_KEY)
                .map_err(|e| format!("clear error: {:?}", e))
        }
    }
}

/// Persiste entradas pendientes del journal al store.
pub fn persist_pending(
    journal: &mut crate::agent_journal::AgentJournal,
    store: &mut Box<dyn HistoryStore>,
) -> Result<usize, String> {
    let pending = journal.drain_pending();
    let count = pending.len();
    if count > 0 {
        store.save_journal_entries(&pending)?;
    }
    Ok(count)
}

/// Crea el HistoryStore apropiado según la plataforma.
pub fn create_store() -> Box<dyn HistoryStore> {
    #[cfg(target_arch = "wasm32")]
    {
        Box::new(crate::history_store::wasm_storage::LocalStorageStore::new())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = default_history_db_path();
        let path_str = path.to_string_lossy().to_string();
        match crate::history_store::native_sqlite::SqliteStore::new(&path_str) {
            Ok(store) => Box::new(store),
            Err(err) => {
                tracing::warn!(
                    "failed to open history db at {path_str}: {err}; falling back to in-memory"
                );
                match crate::history_store::native_sqlite::SqliteStore::in_memory() {
                    Ok(store) => Box::new(store),
                    Err(_) => Box::new(NullStore),
                }
            }
        }
    }
}
