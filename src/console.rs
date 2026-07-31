use crate::process::Pid;
#[cfg(target_arch = "wasm32")]
use js_sys;
use serde::{Deserialize, Serialize};
/// Sistema de logs de consola — captura output estructurado de todos los procesos.
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Log,
    Info,
    Warn,
    Error,
    Debug,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            LogLevel::Log => "log",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
            LogLevel::Debug => "debug",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleEntry {
    pub id: u64,
    pub level: LogLevel,
    pub message: String,
    /// Archivo o proceso que generó el log.
    pub source: String,
    /// PID del proceso (si aplica).
    pub pid: Option<Pid>,
    /// Timestamp en milisegundos desde el inicio.
    pub timestamp_ms: u64,
}

/// Buffer circular de logs con capacidad configurable.
pub struct Console {
    entries: VecDeque<ConsoleEntry>,
    capacity: usize,
    next_id: u64,
    start_ms: u64,
}

impl Console {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
            next_id: 0,
            start_ms: now_ms(),
        }
    }

    pub fn push(
        &mut self,
        level: LogLevel,
        message: impl Into<String>,
        source: impl Into<String>,
        pid: Option<Pid>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }

        self.entries.push_back(ConsoleEntry {
            id,
            level,
            message: message.into(),
            source: source.into(),
            pid,
            timestamp_ms: now_ms().saturating_sub(self.start_ms),
        });

        id
    }

    pub fn log(&mut self, msg: impl Into<String>, src: impl Into<String>) -> u64 {
        self.push(LogLevel::Log, msg, src, None)
    }
    pub fn info(&mut self, msg: impl Into<String>, src: impl Into<String>) -> u64 {
        self.push(LogLevel::Info, msg, src, None)
    }
    pub fn warn(&mut self, msg: impl Into<String>, src: impl Into<String>) -> u64 {
        self.push(LogLevel::Warn, msg, src, None)
    }
    pub fn error(&mut self, msg: impl Into<String>, src: impl Into<String>) -> u64 {
        self.push(LogLevel::Error, msg, src, None)
    }
    pub fn debug(&mut self, msg: impl Into<String>, src: impl Into<String>) -> u64 {
        self.push(LogLevel::Debug, msg, src, None)
    }

    /// Ingesta la salida de un proceso (stdout/stderr) como entradas de log.
    pub fn ingest_process(&mut self, pid: Pid, stdout: &[u8], stderr: &[u8]) {
        if !stdout.is_empty() {
            let text = String::from_utf8_lossy(stdout).to_string();
            for line in text.lines() {
                self.push(LogLevel::Log, line, format!("pid:{pid}"), Some(pid));
            }
        }
        if !stderr.is_empty() {
            let text = String::from_utf8_lossy(stderr).to_string();
            for line in text.lines() {
                self.push(LogLevel::Error, line, format!("pid:{pid}"), Some(pid));
            }
        }
    }

    pub fn all(&self) -> Vec<&ConsoleEntry> {
        self.entries.iter().collect()
    }

    pub fn by_level(&self, level: &LogLevel) -> Vec<&ConsoleEntry> {
        self.entries.iter().filter(|e| &e.level == level).collect()
    }

    pub fn since(&self, id: u64) -> Vec<&ConsoleEntry> {
        self.entries.iter().filter(|e| e.id > id).collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.all()).unwrap_or_default()
    }
}

impl Default for Console {
    fn default() -> Self {
        Self::new(1000)
    }
}

fn now_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
}
