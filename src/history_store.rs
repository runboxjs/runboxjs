use crate::agent_journal::JournalEntry;
use serde::{Deserialize, Serialize};

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
    fn get_journal_entries(&self, since_id: u64, limit: usize) -> Result<Vec<JournalEntry>, String>;
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
    fn get_journal_entries(&self, _since_id: u64, _limit: usize) -> Result<Vec<JournalEntry>, String> {
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

// ── Backend nativo: SQLite ──────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub mod native_sqlite {
    use super::*;
    use rusqlite::{params, Connection};

    pub struct SqliteStore {
        conn: Connection,
    }

    impl SqliteStore {
        pub fn new(path: &str) -> Result<Self, String> {
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
            self.conn
                .execute_batch(
                    "
                CREATE TABLE IF NOT EXISTS runbox_journal (
                    id INTEGER PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    timestamp_ms INTEGER NOT NULL,
                    tool TEXT NOT NULL,
                    reason TEXT NOT NULL,
                    result_summary TEXT NOT NULL DEFAULT '',
                    files_affected TEXT NOT NULL DEFAULT '[]'
                );

                CREATE INDEX IF NOT EXISTS idx_journal_session ON runbox_journal(session_id);
                CREATE INDEX IF NOT EXISTS idx_journal_tool ON runbox_journal(tool);
                CREATE INDEX IF NOT EXISTS idx_journal_timestamp ON runbox_journal(timestamp_ms);

                CREATE VIRTUAL TABLE IF NOT EXISTS journal_fts USING fts5(
                    tool, reason, result_summary, files_affected,
                    content='runbox_journal',
                    content_rowid='id'
                );
                ",
                )
                .map_err(|e| format!("schema init error: {e}"))?;

            self.conn
                .execute("INSERT INTO journal_fts(journal_fts) VALUES ('rebuild')", [])
                .ok();
            Ok(())
        }

        fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<JournalEntry> {
            let files_str: String = row.get(6)?;
            let files: Vec<String> = serde_json::from_str(&files_str).unwrap_or_default();
            Ok(JournalEntry {
                id: row.get(0)?,
                timestamp_ms: row.get(2)?,
                tool: row.get(3)?,
                reason: row.get(4)?,
                result_summary: row.get(5)?,
                files_affected: files,
                session_id: row.get(1)?,
            })
        }
    }

    impl HistoryStore for SqliteStore {
        fn save_journal_entry(&mut self, entry: &JournalEntry) -> Result<(), String> {
            let files_json =
                serde_json::to_string(&entry.files_affected).unwrap_or_else(|_| "[]".to_string());
            self.conn
                .execute(
                    "INSERT OR REPLACE INTO runbox_journal (id, session_id, timestamp_ms, tool, reason, result_summary, files_affected)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        entry.id,
                        entry.session_id,
                        entry.timestamp_ms,
                        entry.tool,
                        entry.reason,
                        entry.result_summary,
                        files_json,
                    ],
                )
                .map_err(|e| format!("insert error: {e}"))?;

            // Update FTS
            self.conn
                .execute(
                    "INSERT INTO journal_fts(rowid, tool, reason, result_summary, files_affected)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        entry.id,
                        entry.tool,
                        entry.reason,
                        entry.result_summary,
                        files_json,
                    ],
                )
                .ok();
            Ok(())
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
                let files_json =
                    serde_json::to_string(&entry.files_affected).unwrap_or_else(|_| "[]".to_string());
                tx.execute(
                    "INSERT OR REPLACE INTO runbox_journal (id, session_id, timestamp_ms, tool, reason, result_summary, files_affected)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        entry.id,
                        entry.session_id,
                        entry.timestamp_ms,
                        entry.tool,
                        entry.reason,
                        entry.result_summary,
                        files_json,
                    ],
                )
                .map_err(|e| format!("insert error: {e}"))?;

                tx.execute(
                    "INSERT INTO journal_fts(rowid, tool, reason, result_summary, files_affected)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        entry.id,
                        entry.tool,
                        entry.reason,
                        entry.result_summary,
                        files_json,
                    ],
                )
                .ok();
            }
            tx.commit().map_err(|e| format!("tx commit error: {e}"))?;
            Ok(())
        }

        fn get_journal_entries(&self, since_id: u64, limit: usize) -> Result<Vec<JournalEntry>, String> {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, session_id, timestamp_ms, tool, reason, result_summary, files_affected
                     FROM runbox_journal WHERE id > ?1 ORDER BY id ASC LIMIT ?2",
                )
                .map_err(|e| format!("prepare error: {e}"))?;

            let rows = stmt
                .query_map(params![since_id, limit as i64], Self::row_to_entry)
                .map_err(|e| format!("query error: {e}"))?;

            let mut entries = vec![];
            for row in rows {
                entries.push(row.map_err(|e| format!("row error: {e}"))?);
            }
            Ok(entries)
        }

        fn search(&self, query: &SearchQuery) -> Result<SearchOutput, String> {
            let limit = query.limit.min(200);

            // Si hay texto de búsqueda, usar FTS5
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
                    "SELECT j.id, j.session_id, j.timestamp_ms, j.tool, j.reason, j.result_summary, j.files_affected,
                            rank
                     FROM journal_fts f
                     JOIN runbox_journal j ON j.id = f.rowid
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
                        let files_str: String = row.get(6)?;
                        let files: Vec<String> = serde_json::from_str(&files_str).unwrap_or_default();
                        let entry = JournalEntry {
                            id: row.get(0)?,
                            timestamp_ms: row.get(2)?,
                            tool: row.get(3)?,
                            reason: row.get(4)?,
                            result_summary: row.get(5)?,
                            files_affected: files,
                            session_id: row.get(1)?,
                        };
                        let score: f64 = 1.0 - (row.get::<_, f64>(7).unwrap_or(0.0) / 100.0);
                        Ok((entry, score))
                    })
                    .map_err(|e| format!("fts query error: {e}"))?;

                let mut results = vec![];
                for row in rows {
                    let (entry, score) = row.map_err(|e| format!("row error: {e}"))?;
                    let snippet = crate::history_store::build_snippet_inner(&entry, query.text.as_deref().unwrap_or(""));
                    results.push(SearchResult {
                        entry,
                        score: score.max(0.0).min(1.0),
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

            // Sin texto: filtrar por tool / file / session_id
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
                conditions.push(format!("id > ?{}", param_values.len()));
            }

            param_values.push(Box::new(limit as i64));

            let sql = format!(
                "SELECT id, session_id, timestamp_ms, tool, reason, result_summary, files_affected
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
                    "SELECT id, session_id, timestamp_ms, tool, reason, result_summary, files_affected
                     FROM runbox_journal ORDER BY timestamp_ms ASC",
                )
                .map_err(|e| format!("prepare error: {e}"))?;

            let rows = stmt
                .query_map([], Self::row_to_entry)
                .map_err(|e| format!("query error: {e}"))?;

            let mut lines = vec![];
            for row in rows {
                let entry = row.map_err(|e| format!("row error: {e}"))?;
                lines.push(crate::history_store::entry_to_jsonl(&entry));
            }
            Ok(lines.join("\n"))
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

pub(crate) fn entry_to_jsonl(entry: &JournalEntry) -> String {
    serde_json::json!({
        "type": "event",
        "version": 1,
        "provider": "runbox",
        "session_id": entry.session_id,
        "event_id": format!("runbox-ev-{}", entry.id),
        "timestamp_ms": entry.timestamp_ms,
        "tool": entry.tool,
        "reason": entry.reason,
        "result_summary": entry.result_summary,
        "files_affected": entry.files_affected,
    })
    .to_string()
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
            all.push(entry.clone());
            Self::write_all(&all)
        }

        fn save_journal_entries(&mut self, entries: &[JournalEntry]) -> Result<(), String> {
            if entries.is_empty() {
                return Ok(());
            }
            let mut all = Self::read_all()?;
            all.extend_from_slice(entries);
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
            let lines: Vec<String> = all.iter().map(crate::history_store::entry_to_jsonl).collect();
            Ok(lines.join("\n"))
        }

        fn clear(&mut self) -> Result<(), String> {
            let storage = storage().ok_or("localStorage not available")?;
            storage
                .remove_item(STORAGE_KEY)
                .map_err(|e| format!("clear error: {:?}", e))
        }
    }
}

/// Crea el HistoryStore apropiado según la plataforma
pub fn create_store() -> Box<dyn HistoryStore> {
    #[cfg(target_arch = "wasm32")]
    {
        Box::new(crate::history_store::wasm_storage::LocalStorageStore::new())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        match crate::history_store::native_sqlite::SqliteStore::in_memory() {
            Ok(store) => Box::new(store),
            Err(_) => Box::new(NullStore),
        }
    }
}
