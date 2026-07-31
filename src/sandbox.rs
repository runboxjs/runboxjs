/// Sandbox controls — reload, fullscreen, inspector, eventos hacia el browser.
use serde::{Deserialize, Serialize};

/// Eventos que runbox envía al browser (via postMessage / callback JS).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SandboxEvent {
    /// El proceso terminó.
    ProcessExit { pid: u32, code: i32 },
    /// Nueva entrada de consola disponible.
    ConsoleEntry { id: u64 },
    /// El sandbox solicita un reload del iframe.
    Reload { hard: bool },
    /// Activar/desactivar fullscreen.
    Fullscreen { enable: bool },
    /// Inspeccionar un elemento DOM.
    InspectElement { selector: String },
    /// El VFS cambió (para hot-reload).
    FileChanged { path: String },
    /// El servidor interno levantó en un puerto.
    ServerReady { port: u16, url: String },
    /// Error fatal del sandbox.
    FatalError { message: String },
    /// Preview session started.
    PreviewStarted { url: String, session_id: String },
    /// Preview session stopped.
    PreviewStopped { session_id: String },
    /// Preview share URL generated.
    PreviewShared {
        share_url: String,
        session_id: String,
    },
    /// Custom domain configured for preview.
    PreviewDomainSet { domain: String, session_id: String },
}

/// Comandos que el browser puede enviar al sandbox (input del usuario).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SandboxCommand {
    /// Ejecutar una línea de shell.
    Exec {
        line: String,
    },
    /// Escribir en el stdin de un proceso.
    Stdin {
        pid: u32,
        data: String,
    },
    /// Matar un proceso.
    Kill {
        pid: u32,
    },
    /// Recargar el sandbox.
    Reload {
        hard: bool,
    },
    /// Entrar/salir de fullscreen.
    Fullscreen {
        enable: bool,
    },
    /// Inspeccionar el elemento bajo las coordenadas dadas.
    InspectAt {
        x: f64,
        y: f64,
    },
    /// Escribir/leer un archivo del VFS.
    WriteFile {
        path: String,
        content: String,
    },
    ReadFile {
        path: String,
    },
    ListDir {
        path: String,
    },
    /// Start a preview session.
    StartPreview {
        #[serde(default)]
        config_json: Option<String>,
    },
    /// Stop the current preview session.
    StopPreview,
    /// Configure the preview domain.
    SetPreviewDomain {
        domain: String,
    },
    /// Generate a share URL for the current preview.
    SharePreview,
    /// Update preview metadata (title, description, image, etc.).
    SetPreviewMetadata {
        metadata_json: String,
    },
}

/// Estado observable del sandbox, serializable a JSON para el browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxStatus {
    pub running_pids: Vec<u32>,
    pub server_port: Option<u16>,
    pub is_fullscreen: bool,
}

/// Inspector de elementos — describe la estructura de un nodo DOM inspeccionado.
/// (La información real viene del browser; esto es el formato de intercambio.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectedElement {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attributes: Vec<(String, String)>,
    pub computed_styles: Vec<(String, String)>,
    pub inner_text: Option<String>,
    pub children_count: usize,
    pub bounding_box: BoundingBox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Serializa un evento a JSON para enviarlo al browser.
pub fn event_to_json(event: &SandboxEvent) -> String {
    serde_json::to_string(event).unwrap_or_default()
}

/// Parsea un comando recibido desde el browser.
pub fn command_from_json(json: &str) -> Result<SandboxCommand, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_event_json() {
        let event = SandboxEvent::Fullscreen { enable: true };
        let json = event_to_json(&event);
        assert_eq!(json, r#"{"type":"fullscreen","enable":true}"#);
    }

    #[test]
    fn test_sandbox_command_json() {
        let json = r#"{"type":"kill","pid":123}"#;
        let cmd = command_from_json(json).unwrap();
        match cmd {
            SandboxCommand::Kill { pid } => assert_eq!(pid, 123),
            _ => panic!("Wrong command parsed"),
        }
    }
}
