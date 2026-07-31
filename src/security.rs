/// Seguridad y Aislamiento — controles de seguridad para el sandbox.
///
/// Provee:
/// - Límites de recursos (archivos, tamaño, procesos, memoria)
/// - Timeout de ejecución
/// - Aislamiento de red (whitelist/blacklist de dominios)
/// - Generación de headers CSP (Content Security Policy)
/// - Sanitización de HTML
/// - Rate limiting por operación
/// - Audit log para operaciones sensibles
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Resource Limits ─────────────────────────────────────────────────────────

/// Límites de recursos configurables para el sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Número máximo de archivos en el VFS.
    pub max_files: usize,
    /// Tamaño máximo de un archivo individual (bytes).
    pub max_file_size: usize,
    /// Tamaño total máximo del VFS (bytes).
    pub max_total_size: usize,
    /// Número máximo de procesos simultáneos.
    pub max_processes: usize,
    /// Timeout de ejecución por comando (ms).
    pub exec_timeout_ms: u64,
    /// Tamaño máximo de la consola (entradas).
    pub max_console_entries: usize,
    /// Profundidad máxima de directorios.
    pub max_dir_depth: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_files: 10_000,
            max_file_size: 10 * 1024 * 1024,   // 10 MB
            max_total_size: 500 * 1024 * 1024, // 500 MB
            max_processes: 64,
            exec_timeout_ms: 30_000, // 30 seconds
            max_console_entries: 5_000,
            max_dir_depth: 50,
        }
    }
}

/// Resultado de una verificación de límites.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitCheck {
    pub allowed: bool,
    pub reason: Option<String>,
    pub current: usize,
    pub limit: usize,
}

impl LimitCheck {
    pub fn ok() -> Self {
        Self {
            allowed: true,
            reason: None,
            current: 0,
            limit: 0,
        }
    }

    pub fn denied(reason: impl Into<String>, current: usize, limit: usize) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
            current,
            limit,
        }
    }
}

// ── Network Isolation ───────────────────────────────────────────────────────

/// Política de aislamiento de red.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicy {
    /// Modo de filtrado: whitelist (solo permite listados) o blacklist (bloquea listados).
    pub mode: NetworkFilterMode,
    /// Lista de dominios/patrones.
    pub domains: Vec<String>,
    /// Permitir localhost.
    pub allow_localhost: bool,
    /// Permitir acceso a registry.npmjs.org.
    pub allow_npm_registry: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkFilterMode {
    /// Solo permite los dominios listados.
    Whitelist,
    /// Bloquea los dominios listados, permite el resto.
    Blacklist,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            mode: NetworkFilterMode::Blacklist,
            domains: Vec::new(),
            allow_localhost: true,
            allow_npm_registry: true,
        }
    }
}

impl NetworkPolicy {
    /// Verifica si un dominio está permitido por la política.
    pub fn is_allowed(&self, domain: &str) -> bool {
        // Localhost siempre según config
        if is_localhost(domain) {
            return self.allow_localhost;
        }

        // npm registry siempre según config
        if domain == "registry.npmjs.org" || domain == "registry.yarnpkg.com" {
            return self.allow_npm_registry;
        }

        let matches = self
            .domains
            .iter()
            .any(|pattern| domain_matches(domain, pattern));

        match self.mode {
            NetworkFilterMode::Whitelist => matches,
            NetworkFilterMode::Blacklist => !matches,
        }
    }

    /// Verifica si una URL está permitida (extrae el dominio).
    pub fn is_url_allowed(&self, url: &str) -> bool {
        match extract_domain(url) {
            Some(domain) => self.is_allowed(&domain),
            None => false,
        }
    }
}

fn is_localhost(domain: &str) -> bool {
    domain == "localhost"
        || domain == "127.0.0.1"
        || domain == "0.0.0.0"
        || domain == "::1"
        || domain.starts_with("localhost:")
        || domain.starts_with("127.0.0.1:")
}

fn domain_matches(domain: &str, pattern: &str) -> bool {
    if pattern.starts_with("*.") {
        // *.example.com matches sub.example.com but not example.com
        let suffix = &pattern[1..]; // .example.com
        domain.ends_with(suffix) || domain == &pattern[2..]
    } else {
        domain == pattern
    }
}

fn extract_domain(url: &str) -> Option<String> {
    // http://example.com/path → example.com
    let after_scheme = if let Some(pos) = url.find("://") {
        &url[pos + 3..]
    } else {
        url
    };
    let domain = after_scheme.split('/').next()?;
    // Remove port
    let domain = domain.split(':').next().unwrap_or(domain);
    if domain.is_empty() {
        None
    } else {
        Some(domain.to_string())
    }
}

// ── CSP Generation ──────────────────────────────────────────────────────────

/// Configuración para generación de Content Security Policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CspConfig {
    /// Permitir scripts inline (unsafe-inline).
    pub allow_inline_scripts: bool,
    /// Permitir eval() (unsafe-eval).
    pub allow_eval: bool,
    /// Orígenes permitidos para scripts.
    pub script_sources: Vec<String>,
    /// Orígenes permitidos para estilos.
    pub style_sources: Vec<String>,
    /// Orígenes permitidos para imágenes.
    pub img_sources: Vec<String>,
    /// Orígenes permitidos para conexiones (fetch, XHR, WebSocket).
    pub connect_sources: Vec<String>,
    /// Orígenes permitidos para frames.
    pub frame_sources: Vec<String>,
    /// Orígenes permitidos para fonts.
    pub font_sources: Vec<String>,
    /// URL para reportar violaciones.
    pub report_uri: Option<String>,
}

impl Default for CspConfig {
    fn default() -> Self {
        Self {
            allow_inline_scripts: true, // Sandbox needs this for live-reload
            allow_eval: true,           // JS engine uses eval
            script_sources: vec!["'self'".into()],
            style_sources: vec!["'self'".into(), "'unsafe-inline'".into()],
            img_sources: vec!["'self'".into(), "data:".into(), "blob:".into()],
            connect_sources: vec!["'self'".into()],
            frame_sources: vec!["'self'".into()],
            font_sources: vec!["'self'".into(), "data:".into()],
            report_uri: None,
        }
    }
}

impl CspConfig {
    /// Genera el header Content-Security-Policy.
    pub fn to_header(&self) -> String {
        let mut directives = Vec::new();

        // default-src
        directives.push("default-src 'self'".to_string());

        // script-src
        let mut script = self.script_sources.clone();
        if self.allow_inline_scripts {
            script.push("'unsafe-inline'".into());
        }
        if self.allow_eval {
            script.push("'unsafe-eval'".into());
        }
        directives.push(format!("script-src {}", script.join(" ")));

        // style-src
        directives.push(format!("style-src {}", self.style_sources.join(" ")));

        // img-src
        directives.push(format!("img-src {}", self.img_sources.join(" ")));

        // connect-src
        directives.push(format!("connect-src {}", self.connect_sources.join(" ")));

        // frame-src
        directives.push(format!("frame-src {}", self.frame_sources.join(" ")));

        // font-src
        directives.push(format!("font-src {}", self.font_sources.join(" ")));

        // report-uri
        if let Some(ref uri) = self.report_uri {
            directives.push(format!("report-uri {uri}"));
        }

        directives.join("; ")
    }
}

// ── HTML Sanitization ───────────────────────────────────────────────────────

/// Tags HTML peligrosos que deben ser eliminados.
const DANGEROUS_TAGS: &[&str] = &[
    "script", "iframe", "object", "embed", "form", "input", "textarea", "button", "select", "link",
    "meta", "base", "applet", "frame", "frameset",
];

/// Atributos HTML peligrosos que deben ser eliminados.
const DANGEROUS_ATTRS: &[&str] = &[
    "onclick",
    "onload",
    "onerror",
    "onmouseover",
    "onfocus",
    "onblur",
    "onsubmit",
    "onchange",
    "onkeydown",
    "onkeyup",
    "onkeypress",
    "onmousedown",
    "onmouseup",
    "onmousemove",
    "onmouseout",
    "ondblclick",
    "oncontextmenu",
    "ondrag",
    "ondragend",
    "ondragenter",
    "ondragleave",
    "ondragover",
    "ondragstart",
    "ondrop",
    "onscroll",
    "onresize",
    "oninput",
    "oninvalid",
    "formaction",
    "xlink:href",
    "data-bind",
];

/// Sanitiza HTML eliminando tags y atributos peligrosos.
/// Mantiene el contenido de texto pero elimina los tags peligrosos.
pub fn sanitize_html(html: &str) -> String {
    let mut result = html.to_string();

    // Remove dangerous tags and their contents
    for tag in DANGEROUS_TAGS {
        // Remove self-closing tags: <script/>
        let self_closing = format!("<{tag}");
        while let Some(start) = find_tag_case_insensitive(&result, &self_closing) {
            if let Some(end) = result[start..].find('>') {
                // Check if there's a closing tag
                let close_tag = format!("</{tag}>");
                let after_open = start + end + 1;
                if let Some(close_pos) =
                    find_tag_case_insensitive(&result[after_open..], &close_tag)
                {
                    // Remove from open tag to end of close tag
                    let total_end = after_open + close_pos + close_tag.len();
                    result = format!("{}{}", &result[..start], &result[total_end..]);
                } else {
                    // Self-closing or unclosed — remove just the tag
                    result = format!("{}{}", &result[..start], &result[start + end + 1..]);
                }
            } else {
                break;
            }
        }
    }

    // Remove dangerous attributes
    for attr in DANGEROUS_ATTRS {
        // Pattern: attr="..." or attr='...'
        let patterns = [
            format!(r#" {attr}=""#),
            format!(r#" {attr}='"#),
            format!(" {attr}="),
        ];
        for pattern in &patterns {
            while let Some(start) = find_tag_case_insensitive(&result, pattern) {
                let attr_start = start;
                let after_eq = attr_start + pattern.len();
                if after_eq >= result.len() {
                    break;
                }
                let quote = result.as_bytes().get(after_eq - 1).copied();
                let end = if quote == Some(b'"') || quote == Some(b'\'') {
                    let q = quote.unwrap() as char;
                    result[after_eq..].find(q).map(|p| after_eq + p + 1)
                } else {
                    // Unquoted: ends at space or >
                    result[after_eq..].find([' ', '>']).map(|p| after_eq + p)
                };
                match end {
                    Some(e) => {
                        result = format!("{}{}", &result[..attr_start], &result[e..]);
                    }
                    None => break,
                }
            }
        }
    }

    // Remove javascript: URLs in href/src
    result = remove_js_urls(&result, "href");
    result = remove_js_urls(&result, "src");

    result
}

fn find_tag_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let needle_bytes = needle.as_bytes();
    let haystack_bytes = haystack.as_bytes();
    if needle_bytes.is_empty() || haystack_bytes.len() < needle_bytes.len() {
        return None;
    }
    'outer: for i in 0..=(haystack_bytes.len() - needle_bytes.len()) {
        for j in 0..needle_bytes.len() {
            if !haystack_bytes[i + j].eq_ignore_ascii_case(&needle_bytes[j]) {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}

fn remove_js_urls(html: &str, attr: &str) -> String {
    let mut result = html.to_string();
    let patterns = [
        format!(r#"{attr}="javascript:"#),
        format!(r#"{attr}='javascript:"#),
    ];
    for pattern in &patterns {
        while let Some(pos) = find_tag_case_insensitive(&result, pattern) {
            let start = pos;
            let after = start + attr.len() + 2; // attr="
            let quote = result.as_bytes()[start + attr.len() + 1] as char;
            if let Some(end) = result[after..].find(quote) {
                let total_end = after + end + 1;
                result = format!("{}{attr}=\"#\"{}", &result[..start], &result[total_end..]);
            } else {
                break;
            }
        }
    }
    result
}

// ── Rate Limiting ───────────────────────────────────────────────────────────

/// Rate limiter por operación usando ventana deslizante.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Configuración por tipo de operación.
    limits: HashMap<String, RateLimit>,
    /// Timestamps de requests recientes por operación.
    windows: HashMap<String, Vec<u64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    /// Máximo número de operaciones permitidas en la ventana.
    pub max_requests: usize,
    /// Tamaño de la ventana en milisegundos.
    pub window_ms: u64,
}

impl RateLimiter {
    pub fn new() -> Self {
        let mut limits = HashMap::new();
        // Límites por defecto
        limits.insert(
            "exec".into(),
            RateLimit {
                max_requests: 100,
                window_ms: 60_000,
            },
        );
        limits.insert(
            "write".into(),
            RateLimit {
                max_requests: 500,
                window_ms: 60_000,
            },
        );
        limits.insert(
            "read".into(),
            RateLimit {
                max_requests: 1000,
                window_ms: 60_000,
            },
        );
        limits.insert(
            "network".into(),
            RateLimit {
                max_requests: 200,
                window_ms: 60_000,
            },
        );
        limits.insert(
            "preview".into(),
            RateLimit {
                max_requests: 50,
                window_ms: 60_000,
            },
        );

        Self {
            limits,
            windows: HashMap::new(),
        }
    }

    /// Verifica si una operación está permitida. Registra el intento.
    pub fn check(&mut self, operation: &str, now_ms: u64) -> LimitCheck {
        let limit = match self.limits.get(operation) {
            Some(l) => l.clone(),
            None => return LimitCheck::ok(), // No limit configured
        };

        let window = self.windows.entry(operation.to_string()).or_default();

        // Limpiar entradas fuera de la ventana
        let cutoff = now_ms.saturating_sub(limit.window_ms);
        window.retain(|&ts| ts > cutoff);

        if window.len() >= limit.max_requests {
            return LimitCheck::denied(
                format!(
                    "rate limit exceeded for '{operation}': {}/{} per {}ms",
                    window.len(),
                    limit.max_requests,
                    limit.window_ms
                ),
                window.len(),
                limit.max_requests,
            );
        }

        window.push(now_ms);
        LimitCheck::ok()
    }

    /// Configura el límite para una operación.
    pub fn set_limit(&mut self, operation: impl Into<String>, max_requests: usize, window_ms: u64) {
        self.limits.insert(
            operation.into(),
            RateLimit {
                max_requests,
                window_ms,
            },
        );
    }

    /// Limpia todas las ventanas de rate limiting.
    pub fn reset(&mut self) {
        self.windows.clear();
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

// ── Audit Log ───────────────────────────────────────────────────────────────

/// Entrada del audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: u64,
    pub timestamp_ms: u64,
    pub operation: String,
    pub details: String,
    pub outcome: AuditOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditOutcome {
    Success,
    Denied,
    Error,
}

/// Audit logger para operaciones sensibles del sandbox.
#[derive(Debug)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
    next_id: u64,
    max_entries: usize,
}

impl AuditLog {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            max_entries,
        }
    }

    /// Registra una operación en el audit log.
    pub fn log(
        &mut self,
        timestamp_ms: u64,
        operation: impl Into<String>,
        details: impl Into<String>,
        outcome: AuditOutcome,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.entries.push(AuditEntry {
            id,
            timestamp_ms,
            operation: operation.into(),
            details: details.into(),
            outcome,
        });

        // Evitar crecimiento ilimitado
        if self.entries.len() > self.max_entries {
            let drain = self.entries.len() - self.max_entries;
            self.entries.drain(..drain);
        }

        id
    }

    /// Retorna todas las entradas.
    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    /// Retorna entradas desde un ID dado.
    pub fn since(&self, id: u64) -> Vec<&AuditEntry> {
        self.entries.iter().filter(|e| e.id > id).collect()
    }

    /// Retorna entradas filtradas por operación.
    pub fn by_operation(&self, operation: &str) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| e.operation == operation)
            .collect()
    }

    /// Retorna entradas filtradas por outcome.
    pub fn by_outcome(&self, outcome: &AuditOutcome) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| &e.outcome == outcome)
            .collect()
    }

    /// Limpia el log.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Serializa el log completo a JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.entries).unwrap_or_default()
    }

    /// Número de entradas.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new(10_000)
    }
}

// ── Security Manager ────────────────────────────────────────────────────────

/// Manager central de seguridad que coordina todos los controles.
#[derive(Debug)]
pub struct SecurityManager {
    pub limits: ResourceLimits,
    pub network_policy: NetworkPolicy,
    pub csp: CspConfig,
    pub rate_limiter: RateLimiter,
    pub audit: AuditLog,
    /// Contadores actuales de recursos.
    file_count: usize,
    total_size: usize,
}

impl SecurityManager {
    pub fn new() -> Self {
        Self {
            limits: ResourceLimits::default(),
            network_policy: NetworkPolicy::default(),
            csp: CspConfig::default(),
            rate_limiter: RateLimiter::new(),
            audit: AuditLog::default(),
            file_count: 0,
            total_size: 0,
        }
    }

    /// Verifica si se puede escribir un archivo.
    pub fn check_write(
        &mut self,
        path: &str,
        size: usize,
        is_new_file: bool,
        now_ms: u64,
    ) -> LimitCheck {
        // Rate limit
        let rate = self.rate_limiter.check("write", now_ms);
        if !rate.allowed {
            self.audit.log(
                now_ms,
                "write",
                format!("denied: {path} (rate limit)"),
                AuditOutcome::Denied,
            );
            return rate;
        }

        // File size limit
        if size > self.limits.max_file_size {
            self.audit.log(
                now_ms,
                "write",
                format!("denied: {path} (file too large: {size})"),
                AuditOutcome::Denied,
            );
            return LimitCheck::denied(
                format!(
                    "file size {size} exceeds limit {}",
                    self.limits.max_file_size
                ),
                size,
                self.limits.max_file_size,
            );
        }

        // Total VFS size limit
        let new_total = self.total_size + size;
        if new_total > self.limits.max_total_size {
            self.audit.log(
                now_ms,
                "write",
                format!("denied: {path} (VFS full: {new_total})"),
                AuditOutcome::Denied,
            );
            return LimitCheck::denied(
                format!(
                    "total VFS size {new_total} would exceed limit {}",
                    self.limits.max_total_size
                ),
                new_total,
                self.limits.max_total_size,
            );
        }

        // File count limit (solo para archivos nuevos)
        if is_new_file && self.file_count >= self.limits.max_files {
            self.audit.log(
                now_ms,
                "write",
                format!("denied: {path} (too many files)"),
                AuditOutcome::Denied,
            );
            return LimitCheck::denied(
                format!(
                    "file count {} exceeds limit {}",
                    self.file_count, self.limits.max_files
                ),
                self.file_count,
                self.limits.max_files,
            );
        }

        // Directory depth
        let depth = path.matches('/').count();
        if depth > self.limits.max_dir_depth {
            self.audit.log(
                now_ms,
                "write",
                format!("denied: {path} (too deep)"),
                AuditOutcome::Denied,
            );
            return LimitCheck::denied(
                format!(
                    "directory depth {depth} exceeds limit {}",
                    self.limits.max_dir_depth
                ),
                depth,
                self.limits.max_dir_depth,
            );
        }

        self.audit.log(
            now_ms,
            "write",
            format!("allowed: {path} ({size} bytes)"),
            AuditOutcome::Success,
        );
        LimitCheck::ok()
    }

    /// Verifica si se puede ejecutar un comando.
    pub fn check_exec(&mut self, command: &str, running_count: usize, now_ms: u64) -> LimitCheck {
        let rate = self.rate_limiter.check("exec", now_ms);
        if !rate.allowed {
            self.audit.log(
                now_ms,
                "exec",
                format!("denied: {command} (rate limit)"),
                AuditOutcome::Denied,
            );
            return rate;
        }

        if running_count >= self.limits.max_processes {
            self.audit.log(
                now_ms,
                "exec",
                format!("denied: {command} (too many processes)"),
                AuditOutcome::Denied,
            );
            return LimitCheck::denied(
                format!(
                    "process count {running_count} exceeds limit {}",
                    self.limits.max_processes
                ),
                running_count,
                self.limits.max_processes,
            );
        }

        self.audit.log(
            now_ms,
            "exec",
            format!("allowed: {command}"),
            AuditOutcome::Success,
        );
        LimitCheck::ok()
    }

    /// Verifica si una request de red está permitida.
    pub fn check_network(&mut self, url: &str, now_ms: u64) -> LimitCheck {
        let rate = self.rate_limiter.check("network", now_ms);
        if !rate.allowed {
            self.audit.log(
                now_ms,
                "network",
                format!("denied: {url} (rate limit)"),
                AuditOutcome::Denied,
            );
            return rate;
        }

        if !self.network_policy.is_url_allowed(url) {
            self.audit.log(
                now_ms,
                "network",
                format!("denied: {url} (policy)"),
                AuditOutcome::Denied,
            );
            return LimitCheck::denied(format!("URL '{url}' blocked by network policy"), 0, 0);
        }

        self.audit.log(
            now_ms,
            "network",
            format!("allowed: {url}"),
            AuditOutcome::Success,
        );
        LimitCheck::ok()
    }

    /// Actualiza los contadores de recursos tras una escritura exitosa.
    pub fn record_write(&mut self, new_size: usize, is_new: bool, old_size: usize) {
        if is_new {
            self.total_size += new_size;
            self.file_count += 1;
        } else {
            self.total_size = self.total_size.saturating_sub(old_size) + new_size;
        }
    }

    /// Actualiza los contadores tras una eliminación.
    pub fn record_delete(&mut self, size: usize) {
        self.total_size = self.total_size.saturating_sub(size);
        self.file_count = self.file_count.saturating_sub(1);
    }

    /// Retorna estadísticas de uso de recursos.
    pub fn usage_stats(&self) -> serde_json::Value {
        serde_json::json!({
            "files": {
                "current": self.file_count,
                "limit": self.limits.max_files,
                "percent": if self.limits.max_files > 0 {
                    (self.file_count as f64 / self.limits.max_files as f64 * 100.0).clamp(0.0, 100.0) as u64
                } else { 0 }
            },
            "storage": {
                "current_bytes": self.total_size,
                "limit_bytes": self.limits.max_total_size,
                "percent": if self.limits.max_total_size > 0 {
                    (self.total_size as f64 / self.limits.max_total_size as f64 * 100.0).clamp(0.0, 100.0) as u64
                } else { 0 }
            },
            "rate_limits": {
                "exec": self.rate_limiter.limits.get("exec"),
                "write": self.rate_limiter.limits.get("write"),
                "network": self.rate_limiter.limits.get("network"),
            },
            "network_policy": {
                "mode": self.network_policy.mode,
                "domains_count": self.network_policy.domains.len(),
            },
            "audit_entries": self.audit.len(),
        })
    }

    /// Serializa la configuración completa a JSON.
    pub fn config_json(&self) -> String {
        serde_json::json!({
            "limits": self.limits,
            "network_policy": self.network_policy,
            "csp": self.csp,
        })
        .to_string()
    }
}

impl Default for SecurityManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_limits_default() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_files, 10_000);
        assert_eq!(limits.max_file_size, 10 * 1024 * 1024);
        assert_eq!(limits.max_processes, 64);
    }

    #[test]
    fn network_policy_blacklist() {
        let mut policy = NetworkPolicy::default();
        policy.domains = vec!["evil.com".into(), "*.malware.org".into()];
        policy.mode = NetworkFilterMode::Blacklist;

        assert!(policy.is_allowed("example.com"));
        assert!(!policy.is_allowed("evil.com"));
        assert!(!policy.is_allowed("sub.malware.org"));
        // *.malware.org also blocks the bare domain (domain_matches returns true)
        assert!(!policy.is_allowed("malware.org"));
        assert!(policy.is_allowed("localhost"));
        assert!(policy.is_allowed("safe-site.net"));
    }

    #[test]
    fn network_policy_whitelist() {
        let mut policy = NetworkPolicy::default();
        policy.domains = vec!["api.example.com".into(), "*.cdn.com".into()];
        policy.mode = NetworkFilterMode::Whitelist;

        assert!(policy.is_allowed("api.example.com"));
        assert!(policy.is_allowed("static.cdn.com"));
        assert!(!policy.is_allowed("evil.com"));
        assert!(policy.is_allowed("localhost")); // always allowed by default
    }

    #[test]
    fn network_url_check() {
        let policy = NetworkPolicy::default();
        assert!(policy.is_url_allowed("https://example.com/api"));
        assert!(policy.is_url_allowed("http://localhost:3000/"));
        assert!(policy.is_url_allowed("https://registry.npmjs.org/react"));
    }

    #[test]
    fn csp_header_generation() {
        let csp = CspConfig::default();
        let header = csp.to_header();
        assert!(header.contains("default-src 'self'"));
        assert!(header.contains("script-src"));
        assert!(header.contains("'unsafe-inline'"));
        assert!(header.contains("'unsafe-eval'"));
        assert!(header.contains("img-src"));
    }

    #[test]
    fn html_sanitization() {
        let html = r#"<p>Hello</p><script>alert('xss')</script><p>World</p>"#;
        let clean = sanitize_html(html);
        assert!(!clean.contains("<script>"));
        assert!(clean.contains("<p>Hello</p>"));
        assert!(clean.contains("<p>World</p>"));
    }

    #[test]
    fn html_sanitization_attrs() {
        let html = r#"<div onclick="alert('xss')">Hello</div>"#;
        let clean = sanitize_html(html);
        assert!(!clean.contains("onclick"));
        assert!(clean.contains("Hello"));
    }

    #[test]
    fn html_sanitization_js_url() {
        let html = r#"<a href="javascript:alert('xss')">Click</a>"#;
        let clean = sanitize_html(html);
        assert!(!clean.contains("javascript:"));
        assert!(clean.contains("Click"));
    }

    #[test]
    fn rate_limiter_allows() {
        let mut limiter = RateLimiter::new();
        limiter.set_limit("test", 3, 1000);

        assert!(limiter.check("test", 100).allowed);
        assert!(limiter.check("test", 200).allowed);
        assert!(limiter.check("test", 300).allowed);
        assert!(!limiter.check("test", 400).allowed); // 4th in same window
    }

    #[test]
    fn rate_limiter_window_expiry() {
        let mut limiter = RateLimiter::new();
        limiter.set_limit("test", 2, 1000);

        assert!(limiter.check("test", 100).allowed);
        assert!(limiter.check("test", 200).allowed);
        assert!(!limiter.check("test", 300).allowed);

        // After window expires
        assert!(limiter.check("test", 1200).allowed);
    }

    #[test]
    fn audit_log_records() {
        let mut log = AuditLog::new(100);
        log.log(1000, "write", "/test.txt", AuditOutcome::Success);
        log.log(2000, "exec", "npm install", AuditOutcome::Denied);

        assert_eq!(log.len(), 2);
        assert_eq!(log.by_outcome(&AuditOutcome::Denied).len(), 1);
        assert_eq!(log.by_operation("write").len(), 1);
    }

    #[test]
    fn audit_log_max_entries() {
        let mut log = AuditLog::new(3);
        for i in 0..5 {
            log.log(i * 1000, "op", format!("entry {i}"), AuditOutcome::Success);
        }
        assert_eq!(log.len(), 3); // Only last 3 kept
    }

    #[test]
    fn security_manager_check_write() {
        let mut sm = SecurityManager::new();
        sm.limits.max_file_size = 100;

        let check = sm.check_write("/test.txt", 50, true, 1000);
        assert!(check.allowed);

        let check = sm.check_write("/big.txt", 200, true, 2000);
        assert!(!check.allowed);
    }

    #[test]
    fn security_manager_check_network() {
        let mut sm = SecurityManager::new();
        sm.network_policy.mode = NetworkFilterMode::Blacklist;
        sm.network_policy.domains = vec!["evil.com".into()];

        let check = sm.check_network("https://example.com/api", 1000);
        assert!(check.allowed);

        let check = sm.check_network("https://evil.com/malware", 2000);
        assert!(!check.allowed);
    }

    #[test]
    fn domain_extraction() {
        assert_eq!(
            extract_domain("https://example.com/path"),
            Some("example.com".into())
        );
        assert_eq!(
            extract_domain("http://localhost:3000/api"),
            Some("localhost".into())
        );
        assert_eq!(
            extract_domain("ftp://files.server.org"),
            Some("files.server.org".into())
        );
    }

    #[test]
    fn security_manager_usage_stats() {
        let sm = SecurityManager::new();
        let stats = sm.usage_stats();
        assert!(stats["files"]["current"].as_u64().unwrap() == 0);
        assert!(stats["files"]["limit"].as_u64().unwrap() == 10_000);
    }
}
