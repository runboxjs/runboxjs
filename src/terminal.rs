use crate::process::Pid;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Terminal — integración con xterm.js.
/// Gestiona el buffer de salida, cola de entrada, historial y redimensionado.
/// El lado JS (xterm.js) lee `output_drain_json()` y escribe con `input_push()`.
pub struct Terminal {
    output_buf: VecDeque<OutputChunk>,
    input_buf: VecDeque<InputChunk>,
    pub size: TerminalSize,
    capacity: usize,
    history: Vec<String>,
    max_history: usize,
    prompt: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

#[derive(Debug, Clone)]
pub struct InputChunk {
    pub pid: Option<Pid>,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputChunk {
    pub pid: Pid,
    pub data: String,
    pub stream: Stream,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Terminal {
    pub fn new(capacity: usize) -> Self {
        Self {
            output_buf: VecDeque::with_capacity(capacity),
            input_buf: VecDeque::new(),
            size: TerminalSize::default(),
            capacity,
            history: Vec::new(),
            max_history: 1000,
            prompt: "user@runbox".to_string(),
        }
    }

    /// Cambia el prompt (ej: "user@runbox", "root@server", etc.)
    pub fn set_prompt(&mut self, prompt: impl Into<String>) {
        self.prompt = prompt.into();
    }

    // ── Historial ─────────────────────────────────────────────────────────────

    pub fn add_history(&mut self, cmd: String) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if let Some(last) = self.history.last()
            && last == cmd
        {
            return; // evita duplicados consecutivos
        }
        self.history.push(cmd.to_string());
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    pub fn get_history(&self) -> Vec<String> {
        self.history.clone()
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Búsqueda en historial (estilo Ctrl+R)
    pub fn search_history(&self, query: &str) -> Vec<String> {
        if query.is_empty() {
            return self.history.clone();
        }
        self.history
            .iter()
            .filter(|cmd| cmd.contains(query))
            .cloned()
            .collect()
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    // ── Output ────────────────────────────────────────────────────────────────

    pub fn write_stdout(&mut self, pid: Pid, data: impl Into<String>) {
        self.push_output(pid, data.into(), Stream::Stdout);
    }

    pub fn write_stderr(&mut self, pid: Pid, data: impl Into<String>) {
        let raw = data.into();
        let colored = format!("\x1b[91m{raw}\x1b[0m"); // rojo más brillante
        self.push_output(pid, colored, Stream::Stderr);
    }

    pub fn ingest_output(&mut self, pid: Pid, stdout: &[u8], stderr: &[u8]) {
        if !stdout.is_empty() {
            self.write_stdout(pid, String::from_utf8_lossy(stdout));
        }
        if !stderr.is_empty() {
            self.write_stderr(pid, String::from_utf8_lossy(stderr));
        }
    }

    fn push_output(&mut self, pid: Pid, data: String, stream: Stream) {
        if self.output_buf.len() >= self.capacity {
            self.output_buf.pop_front();
        }
        self.output_buf.push_back(OutputChunk { pid, data, stream });
    }

    pub fn output_drain(&mut self) -> Vec<OutputChunk> {
        self.output_buf.drain(..).collect()
    }

    pub fn output_drain_json(&mut self) -> String {
        serde_json::to_string(&self.output_drain()).unwrap_or_default()
    }

    pub fn clear_output(&mut self) {
        self.output_buf.clear();
    }

    pub fn write_prompt(&mut self, cwd: &str) {
        let prompt = format!("\x1b[32m{}\x1b[0m:\x1b[34m{cwd}\x1b[0m$ ", self.prompt);
        self.push_output(0, prompt, Stream::Stdout);
    }

    pub fn write_banner(&mut self) {
        let banner = concat!(
            "\x1b[1;35m  RunBox\x1b[0m — sandbox de desarrollo\r\n",
            "  Runtimes: bun · node · python · git · npm · pnpm · yarn\r\n",
            "  Escribe un comando para empezar.\r\n\r\n",
        );
        self.push_output(0, banner.to_string(), Stream::Stdout);
    }

    // ── Input ─────────────────────────────────────────────────────────────────

    pub fn input_push(&mut self, data: impl Into<String>, pid: Option<Pid>) {
        self.input_buf.push_back(InputChunk {
            pid,
            data: data.into(),
        });
    }

    pub fn input_pop(&mut self) -> Option<InputChunk> {
        self.input_buf.pop_front()
    }

    pub fn input_pending(&self) -> bool {
        !self.input_buf.is_empty()
    }

    // ── Resize & Control ──────────────────────────────────────────────────────

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.size = TerminalSize { cols, rows };
    }

    pub fn size_json(&self) -> String {
        serde_json::to_string(&self.size).unwrap_or_default()
    }

    pub fn clear(&mut self) {
        self.push_output(0, "\x1b[2J\x1b[H".to_string(), Stream::Stdout);
    }

    pub fn move_cursor(&mut self, row: u16, col: u16) {
        self.push_output(0, format!("\x1b[{row};{col}H"), Stream::Stdout);
    }

    /// Emite un beep (xterm.js lo reproduce como sonido)
    pub fn bell(&mut self) {
        self.push_output(0, "\x07".to_string(), Stream::Stdout);
    }

    /// Cambia la capacidad máxima del buffer de salida
    pub fn set_capacity(&mut self, new_capacity: usize) {
        self.capacity = new_capacity;
        while self.output_buf.len() > new_capacity {
            self.output_buf.pop_front();
        }
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new(4096)
    }
}

// ── JavaScript glue (igual que antes, pero con nuevos métodos) ───────────────
//
// xterm.onData(data => runbox.terminal_input(data, null));
// runbox.terminal_drain_json()
// runbox.terminal_search_history("ls")   ← nuevo!
// runbox.terminal_bell()
// runbox.terminal_clear_output()

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_history() {
        let mut term = Terminal::new(100);
        term.add_history("ls".into());
        term.add_history("ls".into());
        term.add_history("pwd".into());
        term.add_history("   ".into());
        assert_eq!(term.history_len(), 2);
        assert_eq!(term.get_history(), vec!["ls", "pwd"]);
    }

    #[test]
    fn test_history_search() {
        let mut term = Terminal::new(100);
        term.add_history("ls -la".into());
        term.add_history("cd /src".into());
        term.add_history("ls --help".into());

        let results = term.search_history("ls");
        assert_eq!(results.len(), 2);
        assert!(results.contains(&"ls -la".to_string()));
    }

    #[test]
    fn test_terminal_output() {
        let mut term = Terminal::new(10);
        term.write_stdout(1, "hello");
        term.write_stderr(1, "error");

        let chunks = term.output_drain();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].stream, Stream::Stdout);
        assert!(chunks[1].data.contains("\x1b[91m"));
    }

    #[test]
    fn test_bell_and_clear() {
        let mut term = Terminal::new(10);
        term.bell();
        term.clear_output();
        assert!(term.output_drain().is_empty());
    }

    #[test]
    fn test_custom_prompt() {
        let mut term = Terminal::new(10);
        term.set_prompt("root@server");
        term.write_prompt("/etc");
        let chunks = term.output_drain();
        assert!(chunks[0].data.contains("root@server"));
    }
}
