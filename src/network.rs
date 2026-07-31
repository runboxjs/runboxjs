use crate::error::{Result, RunboxError};
/// Capa de red — HTTP nativo (reqwest) y protocolo Service Worker para WASM.
/// Centraliza toda la I/O de red del sandbox.
use serde::{Deserialize, Serialize};

// ── Tipos comunes ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn body_str(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap_or("[binary content]")
    }

    pub fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        serde_json::from_slice(&self.body)
            .map_err(|e| RunboxError::Runtime(format!("JSON parse error: {e}")))
    }
}

// ── HTTP cliente nativo ───────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub fn http_get(url: &str) -> Result<HttpResponse> {
    let resp = reqwest::blocking::get(url)
        .map_err(|e| RunboxError::Runtime(format!("HTTP GET {url}: {e}")))?;

    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body = resp
        .bytes()
        .map_err(|e| RunboxError::Runtime(format!("read response body: {e}")))?
        .to_vec();

    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn http_post(url: &str, content_type: &str, body: Vec<u8>) -> Result<HttpResponse> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(url)
        .header("Content-Type", content_type)
        .body(body)
        .send()
        .map_err(|e| RunboxError::Runtime(format!("HTTP POST {url}: {e}")))?;

    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body = resp
        .bytes()
        .map_err(|e| RunboxError::Runtime(format!("read response body: {e}")))?
        .to_vec();

    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

#[cfg(target_arch = "wasm32")]
pub fn http_get(_url: &str) -> Result<HttpResponse> {
    Err(RunboxError::Runtime(
        "network not available in WASM — use service_worker_fetch()".into(),
    ))
}

#[cfg(target_arch = "wasm32")]
pub fn http_post(_url: &str, _content_type: &str, _body: Vec<u8>) -> Result<HttpResponse> {
    Err(RunboxError::Runtime(
        "network not available in WASM — use service_worker_fetch()".into(),
    ))
}

// ── Service Worker — protocolo de intercepción de red ─────────────────────────
//
// El Service Worker intercepta todos los fetch() del iframe y los enruta a
// RunBox para que decida la respuesta. El flujo es:
//
//   iframe fetch("http://localhost:3000/api/data")
//     → Service Worker (intercepta)
//     → postMessage al main thread / WASM
//     → RunBox.service_worker_handle(request)
//     → postMessage respuesta
//     → Service Worker responde al iframe

/// Request interceptado por el Service Worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwRequest {
    /// ID único para correlacionar request/response.
    pub id: String,
    pub method: String,
    pub url: String,
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

/// Respuesta que RunBox devuelve al Service Worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwResponse {
    pub id: String,
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: String,
}

impl SwResponse {
    pub fn ok(id: impl Into<String>, body: impl Into<String>, content_type: &str) -> Self {
        let mut headers = std::collections::HashMap::new();
        headers.insert("content-type".into(), content_type.into());
        Self {
            id: id.into(),
            status: 200,
            headers,
            body: body.into(),
        }
    }

    pub fn not_found(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: 404,
            headers: Default::default(),
            body: "Not Found".into(),
        }
    }

    pub fn error(id: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: 500,
            headers: Default::default(),
            body: msg.into(),
        }
    }
}

/// RunBox decide cómo responder a una petición interceptada por el SW.
/// Consulta el VFS y los servidores en ejecución.
pub fn handle_sw_request(req: &SwRequest, vfs: &crate::vfs::Vfs) -> SwResponse {
    let path = extract_path(&req.url);

    // Primero buscar en el VFS como fichero estático
    if let Ok(bytes) = vfs.read(&path) {
        let ct = mime_for_path(&path);
        return SwResponse::ok(&req.id, String::from_utf8_lossy(bytes), ct);
    }

    // Intentar con index.html para SPA routing
    if !path.contains('.')
        && let Ok(bytes) = vfs.read("/index.html")
    {
        return SwResponse::ok(&req.id, String::from_utf8_lossy(bytes), "text/html");
    }

    SwResponse::not_found(&req.id)
}

pub fn extract_path(url: &str) -> String {
    // "http://localhost:3000/src/app.js" → "/src/app.js"
    if let Some(pos) = url.find("://") {
        let after = &url[pos + 3..];
        if let Some(slash) = after.find('/') {
            return after[slash..].split('?').next().unwrap_or("/").to_string();
        }
    }
    "/".to_string()
}

fn mime_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

// ── VFS materialization (nativo) ──────────────────────────────────────────────
// Escribe los archivos del VFS en un directorio temporal del sistema de archivos
// real para poder ejecutar procesos que necesitan archivos reales.

#[cfg(not(target_arch = "wasm32"))]
pub fn materialize_vfs(vfs: &crate::vfs::Vfs, root: &std::path::Path) -> std::io::Result<()> {
    materialize_dir(vfs, "/", root)
}

#[cfg(not(target_arch = "wasm32"))]
fn materialize_dir(
    vfs: &crate::vfs::Vfs,
    vfs_path: &str,
    fs_path: &std::path::Path,
) -> std::io::Result<()> {
    let entries = vfs.list(vfs_path).unwrap_or_default();
    for entry in entries {
        let child_vfs = if vfs_path == "/" {
            format!("/{entry}")
        } else {
            format!("{vfs_path}/{entry}")
        };
        let child_fs = fs_path.join(&entry);

        if let Ok(bytes) = vfs.read(&child_vfs) {
            if let Some(parent) = child_fs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&child_fs, bytes)?;
        } else {
            std::fs::create_dir_all(&child_fs)?;
            materialize_dir(vfs, &child_vfs, &child_fs)?;
        }
    }
    Ok(())
}

// ── Tarball extraction (nativo, para npm) ─────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub fn extract_tgz(bytes: &[u8], dest: &std::path::Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(decoder);
    archive
        .unpack(dest)
        .map_err(|e| RunboxError::Runtime(format!("tarball extraction failed: {e}")))
}

/// Extrae un tarball de npm al VFS bajo `/node_modules/<pkg>/`.
#[cfg(not(target_arch = "wasm32"))]
pub fn extract_tgz_to_vfs(bytes: &[u8], pkg_name: &str, vfs: &mut crate::vfs::Vfs) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::io::{Cursor, Read};
    use tar::Archive;

    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(decoder);

    for entry in archive
        .entries()
        .map_err(|e: std::io::Error| RunboxError::Runtime(e.to_string()))?
    {
        let mut entry = entry.map_err(|e: std::io::Error| RunboxError::Runtime(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e: std::io::Error| RunboxError::Runtime(e.to_string()))?;
        let path_str = path.to_string_lossy().into_owned();

        // Los tarballs de npm tienen el prefijo "package/"
        let rel = path_str.strip_prefix("package/").unwrap_or(&path_str);
        if rel.is_empty() {
            continue;
        }

        let vfs_path = format!("/node_modules/{pkg_name}/{rel}");
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|e| RunboxError::Runtime(e.to_string()))?;
        vfs.write(&vfs_path, content)?;
    }
    Ok(())
}
