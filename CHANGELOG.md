# Changelog

All notable changes to RunboxJS are documented here.

---

## [Unreleased]

### Added

#### Agent Journal (`src/agent_journal.rs`)
- New `AgentJournal` module — a structured circular buffer (500 entries) that records agent reasoning separate from the runtime console.
- `JournalEntry` struct: `{ id, timestamp_ms, tool, reason, result_summary, files_affected }`.
- Query methods: `all()`, `since(id)`, `for_file(path)` — allows any agent to resume work with full decision context.

#### VFS change provenance (`src/vfs.rs`)
- `FileChange` now includes an optional `reason: Option<String>` field.
- Serialized only when present (`skip_serializing_if`), so existing consumers are unaffected.

#### New AI tools (`src/ai/tools.rs` + `src/ai/skills.rs`)
- `agent_memo` — agents call this before acting to record `{ tool, reason, files_affected, result_summary }` in the journal. Enables future agents (or developers) to understand *why* a change happened, not just *what* changed.
- `get_agent_journal` — reads journal entries, filterable by `since_id` (cursor) or `file` path.

#### WASM API (`src/wasm.rs`)
- `RunboxInstance` now holds an `AgentJournal` instance.
- New methods: `journal_entries() -> String` (JSON), `journal_since(id: u64) -> String` (JSON).
- `ai_dispatch` passes the journal into `dispatch_with_preview` so all tool calls have access to it.

---

## Previous releases

See git history for changes prior to this entry.
