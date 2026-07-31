/// Transport layer — stdio, HTTP/SSE y WebSocket.
use crate::error::{Result, RunboxError};

pub trait McpTransport: Send {
    fn send(&mut self, message: &str) -> Result<String>;
    fn close(&mut self);
}

// ── Stdio ─────────────────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub struct StdioTransport {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

#[cfg(not(target_arch = "wasm32"))]
impl StdioTransport {
    pub fn spawn(
        command: &str,
        args: &[String],
        env: &std::collections::HashMap<String, String>,
    ) -> Result<Self> {
        use std::io::BufReader;
        use std::process::{Command, Stdio};

        let mut cmd = Command::new(command);
        cmd.args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| RunboxError::Runtime(format!("failed to spawn '{command}': {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| RunboxError::Runtime("stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RunboxError::Runtime("stdout unavailable".into()))?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl McpTransport for StdioTransport {
    fn send(&mut self, message: &str) -> Result<String> {
        use std::io::{BufRead, Write};
        writeln!(self.stdin, "{message}")
            .map_err(|e| RunboxError::Runtime(format!("write to MCP server: {e}")))?;
        self.stdin.flush().ok();
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .map_err(|e| RunboxError::Runtime(format!("read from MCP server: {e}")))?;
        Ok(line)
    }
    fn close(&mut self) {
        let _ = self.child.kill();
    }
}

// ── HTTP/SSE ──────────────────────────────────────────────────────────────────
//
// El protocolo MCP sobre SSE funciona así:
//   1. Cliente abre GET {base_url}/sse  (stream de eventos)
//   2. Servidor emite eventos SSE con los mensajes JSON
//   3. Cliente envía mensajes via POST {base_url}/message
//
// Ref: https://spec.modelcontextprotocol.io/specification/basic/transports/#http-with-sse

#[cfg(not(target_arch = "wasm32"))]
pub struct SseTransport {
    base_url: String,
    headers: std::collections::HashMap<String, String>,
    client: reqwest::blocking::Client,
}

#[cfg(not(target_arch = "wasm32"))]
impl SseTransport {
    pub fn new(
        base_url: impl Into<String>,
        headers: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            headers,
            client: reqwest::blocking::Client::new(),
        }
    }

    fn post_message(&self, message: &str) -> Result<String> {
        let url = format!("{}/message", self.base_url);
        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(message.to_string());

        for (k, v) in &self.headers {
            req = req.header(k, v);
        }

        let resp = req
            .send()
            .map_err(|e| RunboxError::Runtime(format!("SSE POST {url}: {e}")))?;

        let text = resp
            .text()
            .map_err(|e| RunboxError::Runtime(format!("SSE read response: {e}")))?;
        Ok(text)
    }

    /// Lee un evento SSE del stream. Retorna el JSON del campo `data:`.
    fn read_sse_event(stream: &str) -> Option<String> {
        for line in stream.lines() {
            if let Some(data) = line.strip_prefix("data: ")
                && !data.trim().is_empty()
            {
                return Some(data.trim().to_string());
            }
        }
        None
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl McpTransport for SseTransport {
    fn send(&mut self, message: &str) -> Result<String> {
        // 1. Enviar el request
        self.post_message(message)?;

        // 2. Leer la respuesta del stream SSE
        let sse_url = format!("{}/sse", self.base_url);
        let mut req = self
            .client
            .get(&sse_url)
            .header("Accept", "text/event-stream");
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        let resp = req
            .send()
            .map_err(|e| RunboxError::Runtime(format!("SSE GET {sse_url}: {e}")))?;
        let body = resp
            .text()
            .map_err(|e| RunboxError::Runtime(format!("SSE read: {e}")))?;

        Self::read_sse_event(&body)
            .ok_or_else(|| RunboxError::Runtime("SSE: no data event received".into()))
    }

    fn close(&mut self) {}
}

// Stub para WASM
#[cfg(target_arch = "wasm32")]
pub struct SseTransport {
    pub base_url: String,
    pub headers: std::collections::HashMap<String, String>,
}

#[cfg(target_arch = "wasm32")]
impl SseTransport {
    pub fn new(
        base_url: impl Into<String>,
        headers: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            headers,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl McpTransport for SseTransport {
    fn send(&mut self, _message: &str) -> Result<String> {
        Err(RunboxError::Runtime(
            "SSE transport in WASM: use the JS EventSource API directly".into(),
        ))
    }
    fn close(&mut self) {}
}

// ── WebSocket ─────────────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub struct WebSocketTransport {
    stream: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    url: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl WebSocketTransport {
    pub fn connect(url: impl Into<String>) -> Result<Self> {
        let url = url.into();
        let (stream, _) = tungstenite::connect(&url)
            .map_err(|e| RunboxError::Runtime(format!("WebSocket connect {url}: {e}")))?;
        Ok(Self { stream, url })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl McpTransport for WebSocketTransport {
    fn send(&mut self, message: &str) -> Result<String> {
        use tungstenite::Message;

        self.stream
            .send(Message::Text(message.to_string()))
            .map_err(|e| RunboxError::Runtime(format!("WebSocket send {}: {e}", self.url)))?;

        loop {
            match self.stream.read() {
                Ok(Message::Text(text)) => return Ok(text.to_string()),
                Ok(Message::Ping(data)) => {
                    let _ = self.stream.send(Message::Pong(data));
                }
                Ok(Message::Close(_)) => {
                    return Err(RunboxError::Runtime("WebSocket closed by server".into()));
                }
                Err(e) => return Err(RunboxError::Runtime(format!("WebSocket read: {e}"))),
                _ => continue,
            }
        }
    }

    fn close(&mut self) {
        let _ = self.stream.close(None);
    }
}

// Stub para WASM
#[cfg(target_arch = "wasm32")]
pub struct WebSocketTransport {
    pub url: String,
}

#[cfg(target_arch = "wasm32")]
impl WebSocketTransport {
    pub fn connect(url: impl Into<String>) -> Result<Self> {
        Ok(Self { url: url.into() })
    }
}

#[cfg(target_arch = "wasm32")]
impl McpTransport for WebSocketTransport {
    fn send(&mut self, _message: &str) -> Result<String> {
        Err(RunboxError::Runtime(
            "WebSocket transport in WASM: use the JS WebSocket API directly".into(),
        ))
    }
    fn close(&mut self) {}
}

// ── In-process ────────────────────────────────────────────────────────────────

pub struct InProcessTransport {
    #[allow(clippy::type_complexity)]
    pub handler: Box<dyn FnMut(&str) -> Option<String> + Send>,
}

impl InProcessTransport {
    pub fn new<F>(handler: F) -> Self
    where
        F: FnMut(&str) -> Option<String> + Send + 'static,
    {
        Self {
            handler: Box::new(handler),
        }
    }
}

impl McpTransport for InProcessTransport {
    fn send(&mut self, message: &str) -> Result<String> {
        (self.handler)(message)
            .ok_or_else(|| RunboxError::Runtime("in-process server: no response".into()))
    }
    fn close(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_process_transport() {
        let mut transport = InProcessTransport::new(|msg| {
            if msg == "ping" {
                Some("pong".to_string())
            } else {
                None
            }
        });

        let res = transport.send("ping").unwrap();
        assert_eq!(res, "pong");

        let err = transport.send("hello").unwrap_err();
        match err {
            RunboxError::Runtime(msg) => assert_eq!(msg, "in-process server: no response"),
            _ => panic!("Expected Runtime error"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_read_sse_event() {
        let stream = "id: 1\nevent: message\ndata: {\"hello\":\"world\"}\n\n";
        let data = SseTransport::read_sse_event(stream);
        assert_eq!(data, Some("{\"hello\":\"world\"}".to_string()));

        let stream_empty = "event: ping\n\n";
        let data_empty = SseTransport::read_sse_event(stream_empty);
        assert_eq!(data_empty, None);
    }
}
