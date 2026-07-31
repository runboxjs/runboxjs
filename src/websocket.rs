/// WebSocket — canal de comunicación bidireccional para preview en tiempo real.
///
/// Reemplaza el polling de postMessage con un protocolo WebSocket para:
/// - Sincronización de estado del VFS entre host y viewers
/// - Indicador de conexión (conectado/reconectando/offline)
/// - Reconexión automática con backoff exponencial
/// - Propagación de cambios a todos los viewers conectados
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Connection State ────────────────────────────────────────────────────────

/// Estado de la conexión WebSocket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// Conectado y listo para enviar/recibir mensajes.
    Connected,
    /// Intentando reconectar después de una desconexión.
    Reconnecting { attempt: u32, next_retry_ms: u64 },
    /// Desconectado sin intentos de reconexión activos.
    Disconnected,
    /// Offline — no hay conexión de red disponible.
    Offline,
}

impl ConnectionState {
    pub fn is_connected(&self) -> bool {
        matches!(self, ConnectionState::Connected)
    }

    pub fn is_reconnecting(&self) -> bool {
        matches!(self, ConnectionState::Reconnecting { .. })
    }

    /// Genera el script JS para mostrar el indicador de conexión en la UI.
    pub fn to_indicator_script(&self) -> String {
        let (status, color, text) = match self {
            ConnectionState::Connected => ("connected", "#50fa7b", "●"),
            ConnectionState::Reconnecting { attempt, .. } => {
                let _ = attempt;
                ("reconnecting", "#f1fa8c", "◐")
            }
            ConnectionState::Disconnected => ("disconnected", "#ff5555", "○"),
            ConnectionState::Offline => ("offline", "#6272a4", "✕"),
        };

        format!(
            r#"(function(){{
    var indicator = document.getElementById('runbox-ws-indicator');
    if (!indicator) {{
        indicator = document.createElement('div');
        indicator.id = 'runbox-ws-indicator';
        indicator.style.cssText = 'position:fixed;bottom:8px;right:8px;z-index:99997;padding:4px 8px;border-radius:4px;font-size:11px;font-family:monospace;background:rgba(0,0,0,0.7);color:{color};cursor:default;user-select:none;';
        document.body.appendChild(indicator);
    }}
    indicator.textContent = '{text} {status}';
    indicator.style.color = '{color}';
    indicator.title = 'RunBox: {status}';
}})();"#
        )
    }
}

// ── Backoff Strategy ────────────────────────────────────────────────────────

/// Estrategia de backoff exponencial para reconexión.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackoffConfig {
    /// Delay inicial en ms.
    pub initial_delay_ms: u64,
    /// Multiplicador por cada intento.
    pub multiplier: f64,
    /// Delay máximo en ms.
    pub max_delay_ms: u64,
    /// Jitter máximo en ms (aleatorio añadido al delay).
    pub max_jitter_ms: u64,
    /// Número máximo de intentos antes de rendirse.
    pub max_retries: u32,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay_ms: 1000,
            multiplier: 2.0,
            max_delay_ms: 30_000,
            max_jitter_ms: 1000,
            max_retries: 10,
        }
    }
}

impl BackoffConfig {
    /// Calcula el delay para el intento dado.
    pub fn delay_for_attempt(&self, attempt: u32) -> u64 {
        let base = (self.initial_delay_ms as f64) * self.multiplier.powi(attempt as i32);
        let clamped = base.min(self.max_delay_ms as f64).max(0.0) as u64;
        // In WASM no tenemos random real, usar un jitter determinista basado en attempt
        let jitter = if self.max_jitter_ms > 0 {
            (attempt as u64 * 37) % self.max_jitter_ms
        } else {
            0
        };
        clamped + jitter
    }

    /// Retorna true si se han agotado los reintentos.
    pub fn exhausted(&self, attempt: u32) -> bool {
        attempt >= self.max_retries
    }
}

// ── WebSocket Message Protocol ──────────────────────────────────────────────

/// Mensajes del protocolo WebSocket de RunBox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    // ── Host → Viewer ────────────────────────────────────────────────────
    /// Notifica un cambio en el VFS.
    VfsChange {
        path: String,
        kind: String,
        content: Option<String>,
    },
    /// Recarga completa del preview.
    Reload { hard: bool },
    /// Inyecta CSS actualizado.
    InjectCss { paths: Vec<String> },
    /// Hot Module Replacement.
    Hmr {
        paths: Vec<String>,
        framework: Option<String>,
    },
    /// Muestra un error overlay.
    Error {
        message: String,
        file: Option<String>,
        line: Option<u32>,
    },
    /// Sincronización completa del VFS (snapshot).
    VfsSnapshot { files: HashMap<String, String> },
    /// Información de estado de la sesión.
    SessionInfo {
        session_id: String,
        viewers: u32,
        permissions: String,
    },

    // ── Viewer → Host ────────────────────────────────────────────────────
    /// El viewer está listo.
    ViewerReady { viewer_id: String },
    /// El viewer solicita un archivo.
    FileRequest { path: String },
    /// Acción del terminal (si tiene permisos `interact`).
    TerminalInput { command: String },
    /// El viewer modificó un archivo (si tiene permisos `edit`).
    FileEdit { path: String, content: String },
    /// Heartbeat/ping.
    Ping { timestamp: u64 },
    /// Respuesta al ping.
    Pong { timestamp: u64, server_time: u64 },
}

impl WsMessage {
    /// Serializa el mensaje a JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Deserializa un mensaje desde JSON.
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

// ── WebSocket Channel ───────────────────────────────────────────────────────

/// Canal WebSocket que gestiona la conexión y los viewers conectados.
#[derive(Debug)]
pub struct WsChannel {
    /// Estado actual de la conexión.
    pub state: ConnectionState,
    /// Configuración de backoff para reconexión.
    pub backoff: BackoffConfig,
    /// Viewers conectados (viewer_id → info).
    viewers: HashMap<String, ViewerInfo>,
    /// Cola de mensajes pendientes de envío (cuando hay desconexión).
    pending_messages: Vec<WsMessage>,
    /// Tamaño máximo de la cola de mensajes pendientes.
    max_pending: usize,
    /// Historial de latencias (ms) para métricas.
    latency_history: Vec<u64>,
    /// Número de intentos de reconexión actual.
    reconnect_attempt: u32,
    /// Timestamp del último mensaje recibido.
    last_message_at: u64,
    /// Timestamp del último ping enviado.
    last_ping_at: u64,
}

/// Información de un viewer conectado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewerInfo {
    pub viewer_id: String,
    pub connected_at: u64,
    pub last_active: u64,
    pub permissions: ViewerPermission,
    pub ip_hint: Option<String>,
}

/// Nivel de permisos de un viewer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ViewerPermission {
    /// Solo lectura — puede ver el preview.
    #[default]
    View,
    /// Puede usar el terminal.
    Interact,
    /// Puede modificar archivos.
    Edit,
}

impl WsChannel {
    pub fn new() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            backoff: BackoffConfig::default(),
            viewers: HashMap::new(),
            pending_messages: Vec::new(),
            max_pending: 1000,
            latency_history: Vec::new(),
            reconnect_attempt: 0,
            last_message_at: 0,
            last_ping_at: 0,
        }
    }

    /// Marca la conexión como establecida.
    pub fn on_connected(&mut self) {
        self.state = ConnectionState::Connected;
        self.reconnect_attempt = 0;
    }

    /// Marca la conexión como desconectada e inicia reconexión.
    pub fn on_disconnected(&mut self) {
        if self.backoff.exhausted(self.reconnect_attempt) {
            self.state = ConnectionState::Disconnected;
        } else {
            let delay = self.backoff.delay_for_attempt(self.reconnect_attempt);
            self.state = ConnectionState::Reconnecting {
                attempt: self.reconnect_attempt,
                next_retry_ms: delay,
            };
            self.reconnect_attempt += 1;
        }
    }

    /// Marca como offline.
    pub fn on_offline(&mut self) {
        self.state = ConnectionState::Offline;
    }

    /// Registra un nuevo viewer conectado.
    pub fn add_viewer(&mut self, viewer_id: &str, now_ms: u64, permission: ViewerPermission) {
        self.viewers.insert(
            viewer_id.to_string(),
            ViewerInfo {
                viewer_id: viewer_id.to_string(),
                connected_at: now_ms,
                last_active: now_ms,
                permissions: permission,
                ip_hint: None,
            },
        );
    }

    /// Elimina un viewer desconectado.
    pub fn remove_viewer(&mut self, viewer_id: &str) {
        self.viewers.remove(viewer_id);
    }

    /// Actualiza la última actividad de un viewer.
    pub fn touch_viewer(&mut self, viewer_id: &str, now_ms: u64) {
        if let Some(v) = self.viewers.get_mut(viewer_id) {
            v.last_active = now_ms;
        }
    }

    /// Retorna el número de viewers conectados.
    pub fn viewer_count(&self) -> usize {
        self.viewers.len()
    }

    /// Retorna info de todos los viewers.
    pub fn viewers(&self) -> &HashMap<String, ViewerInfo> {
        &self.viewers
    }

    /// Encola un mensaje. Si está conectado, lo marca como listo para enviar.
    pub fn send(&mut self, msg: WsMessage) {
        if self.pending_messages.len() >= self.max_pending {
            // Drop oldest messages when queue is full
            self.pending_messages.remove(0);
        }
        self.pending_messages.push(msg);
    }

    /// Broadcast: envía un cambio del VFS a todos los viewers.
    pub fn broadcast_vfs_change(&mut self, path: &str, kind: &str, content: Option<&str>) {
        self.send(WsMessage::VfsChange {
            path: path.to_string(),
            kind: kind.to_string(),
            content: content.map(|s| s.to_string()),
        });
    }

    /// Broadcast: hot reload.
    pub fn broadcast_reload(&mut self, hard: bool) {
        self.send(WsMessage::Reload { hard });
    }

    /// Drain mensajes pendientes para enviar.
    pub fn drain_pending(&mut self) -> Vec<WsMessage> {
        std::mem::take(&mut self.pending_messages)
    }

    /// Procesa un mensaje recibido de un viewer.
    pub fn handle_message(&mut self, msg: &WsMessage, now_ms: u64) -> Option<WsMessage> {
        self.last_message_at = now_ms;
        match msg {
            WsMessage::ViewerReady { viewer_id } => {
                self.add_viewer(viewer_id, now_ms, ViewerPermission::View);
                None
            }
            WsMessage::Ping { timestamp } => {
                self.last_ping_at = now_ms;
                Some(WsMessage::Pong {
                    timestamp: *timestamp,
                    server_time: now_ms,
                })
            }
            WsMessage::Pong { timestamp, .. } => {
                let latency = now_ms.saturating_sub(*timestamp);
                self.latency_history.push(latency);
                if self.latency_history.len() > 100 {
                    self.latency_history.remove(0);
                }
                None
            }
            _ => None,
        }
    }

    /// Retorna la latencia promedio en ms.
    pub fn avg_latency_ms(&self) -> u64 {
        if self.latency_history.is_empty() {
            return 0;
        }
        let sum: u64 = self.latency_history.iter().sum();
        sum / self.latency_history.len() as u64
    }

    /// Genera el script JS para el cliente WebSocket de live-reload.
    pub fn client_script(&self, ws_url: &str) -> String {
        format!(
            r#"<script data-runbox-ws>
(function() {{
  'use strict';
  var RUNBOX_WS = window.__RUNBOX_WS || {{}};
  var ws = null;
  var reconnectAttempt = 0;
  var maxRetries = {max_retries};
  var baseDelay = {base_delay};

  function connect() {{
    ws = new WebSocket('{ws_url}');

    ws.onopen = function() {{
      RUNBOX_WS.connected = true;
      reconnectAttempt = 0;
      ws.send(JSON.stringify({{ type: 'viewer_ready', viewer_id: RUNBOX_WS.viewerId }}));
      updateIndicator('connected');
      console.log('[RunBox WS] Connected');
    }};

    ws.onmessage = function(event) {{
      try {{
        var msg = JSON.parse(event.data);
        handleMessage(msg);
      }} catch(e) {{}}
    }};

    ws.onclose = function() {{
      RUNBOX_WS.connected = false;
      if (reconnectAttempt < maxRetries) {{
        var delay = Math.min(baseDelay * Math.pow(2, reconnectAttempt), 30000);
        updateIndicator('reconnecting');
        setTimeout(connect, delay);
        reconnectAttempt++;
      }} else {{
        updateIndicator('disconnected');
      }}
    }};

    ws.onerror = function() {{
      ws.close();
    }};
  }}

  function handleMessage(msg) {{
    switch(msg.type) {{
      case 'reload':
        if (msg.hard) window.location.reload();
        else softReload();
        break;
      case 'inject_css':
        injectCss(msg.paths || []);
        break;
      case 'vfs_change':
        if (msg.path && msg.path.endsWith('.css')) {{
          injectCss([msg.path]);
        }}
        break;
      case 'error':
        showError(msg);
        break;
      case 'pong':
        break;
    }}
  }}

  function softReload() {{
    var links = document.querySelectorAll('link[rel="stylesheet"]');
    links.forEach(function(link) {{
      if (link.href) {{
        var url = new URL(link.href);
        url.searchParams.set('_rb_t', Date.now());
        link.href = url.toString();
      }}
    }});
  }}

  function injectCss(paths) {{
    paths.forEach(function(path) {{
      var links = document.querySelectorAll('link[rel="stylesheet"]');
      links.forEach(function(link) {{
        if (link.href && link.href.includes(path)) {{
          var url = new URL(link.href);
          url.searchParams.set('_rb_t', Date.now());
          link.href = url.toString();
        }}
      }});
    }});
  }}

  function showError(msg) {{
    var old = document.getElementById('runbox-error-overlay');
    if (old) old.remove();
    var div = document.createElement('div');
    div.id = 'runbox-error-overlay';
    div.style.cssText = 'position:fixed;inset:0;z-index:99999;background:rgba(0,0,0,0.85);color:#fff;font-family:monospace;padding:2rem;overflow:auto;';
    div.innerHTML = '<div style="max-width:800px;margin:0 auto;"><h2 style="color:#ff5555;">⚠ ' + (msg.message || 'Error') + '</h2>' +
      (msg.file ? '<p style="color:#888;">' + msg.file + (msg.line ? ':' + msg.line : '') + '</p>' : '') +
      '<button onclick="this.closest(\'#runbox-error-overlay\').remove()" style="background:none;border:1px solid #666;color:#fff;padding:4px 12px;cursor:pointer;border-radius:4px;">Close</button></div>';
    document.body.appendChild(div);
  }}

  function updateIndicator(status) {{
    var colors = {{ connected: '#50fa7b', reconnecting: '#f1fa8c', disconnected: '#ff5555', offline: '#6272a4' }};
    var icons = {{ connected: '●', reconnecting: '◐', disconnected: '○', offline: '✕' }};
    var el = document.getElementById('runbox-ws-indicator');
    if (!el) {{
      el = document.createElement('div');
      el.id = 'runbox-ws-indicator';
      el.style.cssText = 'position:fixed;bottom:8px;right:8px;z-index:99997;padding:4px 8px;border-radius:4px;font-size:11px;font-family:monospace;background:rgba(0,0,0,0.7);cursor:default;user-select:none;';
      document.body.appendChild(el);
    }}
    el.textContent = (icons[status] || '?') + ' ' + status;
    el.style.color = colors[status] || '#fff';
  }}

  // Heartbeat
  setInterval(function() {{
    if (ws && ws.readyState === 1) {{
      ws.send(JSON.stringify({{ type: 'ping', timestamp: Date.now() }}));
    }}
  }}, 30000);

  RUNBOX_WS.viewerId = 'v_' + Math.random().toString(36).substr(2, 9);
  window.__RUNBOX_WS = RUNBOX_WS;
  connect();
}})();
</script>"#,
            ws_url = ws_url,
            max_retries = self.backoff.max_retries,
            base_delay = self.backoff.initial_delay_ms,
        )
    }

    /// Retorna info del canal como JSON.
    pub fn info_json(&self) -> String {
        serde_json::json!({
            "state": self.state,
            "viewers": self.viewer_count(),
            "pending_messages": self.pending_messages.len(),
            "avg_latency_ms": self.avg_latency_ms(),
            "reconnect_attempt": self.reconnect_attempt,
            "last_message_at": self.last_message_at,
            "last_ping_at": self.last_ping_at,
        })
        .to_string()
    }
}

impl Default for WsChannel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_lifecycle() {
        let mut ch = WsChannel::new();
        assert!(!ch.state.is_connected());

        ch.on_connected();
        assert!(ch.state.is_connected());

        ch.on_disconnected();
        assert!(ch.state.is_reconnecting());

        ch.on_connected();
        assert!(ch.state.is_connected());
        assert_eq!(ch.reconnect_attempt, 0);
    }

    #[test]
    fn backoff_exponential() {
        let config = BackoffConfig::default();
        let d0 = config.delay_for_attempt(0);
        let d1 = config.delay_for_attempt(1);
        let d2 = config.delay_for_attempt(2);
        assert!(d1 > d0, "delay should increase: d0={d0}, d1={d1}");
        assert!(d2 > d1, "delay should increase: d1={d1}, d2={d2}");
    }

    #[test]
    fn backoff_max_delay() {
        let config = BackoffConfig {
            max_delay_ms: 5000,
            max_jitter_ms: 0,
            ..Default::default()
        };
        let d = config.delay_for_attempt(100);
        assert!(d <= 5000, "delay should be capped: {d}");
    }

    #[test]
    fn backoff_exhaustion() {
        let config = BackoffConfig {
            max_retries: 3,
            ..Default::default()
        };
        assert!(!config.exhausted(0));
        assert!(!config.exhausted(2));
        assert!(config.exhausted(3));
        assert!(config.exhausted(10));
    }

    #[test]
    fn viewer_management() {
        let mut ch = WsChannel::new();
        ch.add_viewer("v1", 1000, ViewerPermission::View);
        ch.add_viewer("v2", 2000, ViewerPermission::Edit);
        assert_eq!(ch.viewer_count(), 2);

        ch.touch_viewer("v1", 3000);
        assert_eq!(ch.viewers().get("v1").unwrap().last_active, 3000);

        ch.remove_viewer("v1");
        assert_eq!(ch.viewer_count(), 1);
    }

    #[test]
    fn message_queue() {
        let mut ch = WsChannel::new();
        ch.broadcast_vfs_change("/style.css", "modified", Some("body{}"));
        ch.broadcast_reload(false);
        assert_eq!(ch.pending_messages.len(), 2);

        let msgs = ch.drain_pending();
        assert_eq!(msgs.len(), 2);
        assert!(ch.pending_messages.is_empty());
    }

    #[test]
    fn message_serialization() {
        let msg = WsMessage::VfsChange {
            path: "/app.css".to_string(),
            kind: "modified".to_string(),
            content: Some("body{}".to_string()),
        };
        let json = msg.to_json();
        assert!(json.contains("vfs_change"));
        let parsed = WsMessage::from_json(&json);
        assert!(parsed.is_some());
    }

    #[test]
    fn ping_pong() {
        let mut ch = WsChannel::new();
        let ping = WsMessage::Ping { timestamp: 1000 };
        let response = ch.handle_message(&ping, 1050);
        assert!(matches!(
            response,
            Some(WsMessage::Pong {
                timestamp: 1000,
                ..
            })
        ));
    }

    #[test]
    fn connection_indicator_script() {
        let state = ConnectionState::Connected;
        let script = state.to_indicator_script();
        assert!(script.contains("connected"));
        assert!(script.contains("runbox-ws-indicator"));
    }

    #[test]
    fn viewer_ready_adds_viewer() {
        let mut ch = WsChannel::new();
        let msg = WsMessage::ViewerReady {
            viewer_id: "v_test".to_string(),
        };
        ch.handle_message(&msg, 5000);
        assert_eq!(ch.viewer_count(), 1);
        assert!(ch.viewers().contains_key("v_test"));
    }
}
