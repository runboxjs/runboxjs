/// Preview system — manages project preview sessions with custom domain support,
/// share URLs, live reload injection, and Open Graph metadata for social sharing.
///
/// Architecture:
///   1. PreviewConfig holds domain, port, base path, CORS, and metadata settings.
///   2. PreviewSession tracks the lifecycle of an active preview (start/stop/status).
///   3. PreviewRouter resolves incoming requests against the config and VFS.
///   4. The WASM layer exposes methods to JS; the host page renders the iframe.
///
/// Custom domain flow:
///   - User configures `domain` in PreviewConfig (e.g., "preview.myapp.com").
///   - The host application maps that domain (DNS CNAME / reverse proxy) to
///     the page running RunBox WASM.
///   - Service Worker routes requests for that origin through RunBox.
///   - Share URLs use the custom domain so third parties can view the project.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{Result, RunboxError};
use crate::vfs::Vfs;

// ── Preview configuration ────────────────────────────────────────────────────

/// Full configuration for a preview session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewConfig {
    /// Custom domain the user owns (e.g., "preview.myapp.com").
    /// When set, share URLs use this domain instead of localhost.
    #[serde(default)]
    pub domain: Option<String>,

    /// Port the preview "listens" on (used in localhost URLs).
    /// Default: 3000.
    #[serde(default = "default_port")]
    pub port: u16,

    /// Base path prefix (e.g., "/app"). Default: "/".
    #[serde(default = "default_base_path")]
    pub base_path: String,

    /// Whether to use HTTPS in generated URLs.
    #[serde(default)]
    pub https: bool,

    /// CORS configuration for cross-origin sharing.
    #[serde(default)]
    pub cors: CorsConfig,

    /// Open Graph / social sharing metadata.
    #[serde(default)]
    pub metadata: PreviewMetadata,

    /// Whether to inject the live-reload script into HTML responses.
    #[serde(default = "default_true")]
    pub live_reload: bool,

    /// Auto-open the preview URL in a new tab on start.
    #[serde(default)]
    pub auto_open: bool,

    /// Custom headers to add to every response.
    #[serde(default)]
    pub custom_headers: HashMap<String, String>,

    /// Allowed path patterns for the preview (empty = allow all).
    #[serde(default)]
    pub allowed_paths: Vec<String>,

    /// SPA mode — serve index.html for all non-file routes.
    #[serde(default = "default_true")]
    pub spa: bool,
}

fn default_port() -> u16 {
    3000
}
fn default_base_path() -> String {
    "/".to_string()
}
fn default_true() -> bool {
    true
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            domain: None,
            port: default_port(),
            base_path: default_base_path(),
            https: false,
            cors: CorsConfig::default(),
            metadata: PreviewMetadata::default(),
            live_reload: true,
            auto_open: false,
            custom_headers: HashMap::new(),
            allowed_paths: Vec::new(),
            spa: true,
        }
    }
}

// ── CORS configuration ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    /// Allowed origins. `["*"]` means any origin.
    #[serde(default = "default_cors_origins")]
    pub allowed_origins: Vec<String>,

    /// Allowed HTTP methods.
    #[serde(default = "default_cors_methods")]
    pub allowed_methods: Vec<String>,

    /// Allowed request headers.
    #[serde(default = "default_cors_headers")]
    pub allowed_headers: Vec<String>,

    /// Whether to allow credentials (cookies, auth headers).
    #[serde(default)]
    pub allow_credentials: bool,

    /// Max age for preflight cache (seconds).
    #[serde(default = "default_max_age")]
    pub max_age: u32,
}

fn default_cors_origins() -> Vec<String> {
    vec!["*".to_string()]
}
fn default_cors_methods() -> Vec<String> {
    vec![
        "GET".into(),
        "POST".into(),
        "PUT".into(),
        "DELETE".into(),
        "OPTIONS".into(),
        "PATCH".into(),
    ]
}
fn default_cors_headers() -> Vec<String> {
    vec![
        "Content-Type".into(),
        "Authorization".into(),
        "X-Requested-With".into(),
    ]
}
fn default_max_age() -> u32 {
    86400
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: default_cors_origins(),
            allowed_methods: default_cors_methods(),
            allowed_headers: default_cors_headers(),
            allow_credentials: false,
            max_age: default_max_age(),
        }
    }
}

impl CorsConfig {
    /// Build CORS headers for a given request origin.
    pub fn headers_for(&self, request_origin: Option<&str>) -> HashMap<String, String> {
        let mut h = HashMap::new();

        let origin_value = if self.allowed_origins.contains(&"*".to_string()) {
            if self.allow_credentials {
                // Can't use "*" with credentials; echo the request origin
                request_origin.unwrap_or("*").to_string()
            } else {
                "*".to_string()
            }
        } else if let Some(origin) = request_origin {
            if self.allowed_origins.iter().any(|o| o == origin) {
                origin.to_string()
            } else {
                return h; // Origin not allowed — return empty
            }
        } else {
            return h;
        };

        h.insert("Access-Control-Allow-Origin".into(), origin_value);
        h.insert(
            "Access-Control-Allow-Methods".into(),
            self.allowed_methods.join(", "),
        );
        h.insert(
            "Access-Control-Allow-Headers".into(),
            self.allowed_headers.join(", "),
        );
        h.insert("Access-Control-Max-Age".into(), self.max_age.to_string());

        if self.allow_credentials {
            h.insert("Access-Control-Allow-Credentials".into(), "true".into());
        }

        h
    }
}

// ── Open Graph / social sharing metadata ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewMetadata {
    /// Page title (used in <title> and og:title).
    #[serde(default = "default_title")]
    pub title: String,

    /// Description for og:description and meta description.
    #[serde(default)]
    pub description: String,

    /// URL to an image for og:image.
    #[serde(default)]
    pub image: String,

    /// Favicon URL.
    #[serde(default)]
    pub favicon: String,

    /// Theme color for mobile browsers.
    #[serde(default = "default_theme_color")]
    pub theme_color: String,

    /// Language code (e.g., "en", "es").
    #[serde(default = "default_lang")]
    pub lang: String,

    /// Twitter card type: "summary", "summary_large_image", "app", "player".
    #[serde(default = "default_twitter_card")]
    pub twitter_card: String,

    /// Author name.
    #[serde(default)]
    pub author: String,
}

fn default_title() -> String {
    "RunBox Preview".to_string()
}
fn default_theme_color() -> String {
    "#1a1b1e".to_string()
}
fn default_lang() -> String {
    "en".to_string()
}
fn default_twitter_card() -> String {
    "summary_large_image".to_string()
}

impl Default for PreviewMetadata {
    fn default() -> Self {
        Self {
            title: default_title(),
            description: String::new(),
            image: String::new(),
            favicon: String::new(),
            theme_color: default_theme_color(),
            lang: default_lang(),
            twitter_card: default_twitter_card(),
            author: String::new(),
        }
    }
}

impl PreviewMetadata {
    /// Generate the `<head>` meta tags for Open Graph / Twitter / SEO.
    pub fn to_meta_tags(&self, canonical_url: &str) -> String {
        let mut tags = String::new();

        // Basic SEO
        tags.push_str(&format!("<title>{}</title>\n", html_escape(&self.title)));
        if !self.description.is_empty() {
            tags.push_str(&format!(
                "<meta name=\"description\" content=\"{}\">\n",
                html_escape(&self.description)
            ));
        }
        if !self.author.is_empty() {
            tags.push_str(&format!(
                "<meta name=\"author\" content=\"{}\">\n",
                html_escape(&self.author)
            ));
        }
        tags.push_str(&format!(
            "<meta name=\"theme-color\" content=\"{}\">\n",
            html_escape(&self.theme_color)
        ));

        // Open Graph
        tags.push_str(&format!(
            "<meta property=\"og:title\" content=\"{}\">\n",
            html_escape(&self.title)
        ));
        tags.push_str(&format!(
            "<meta property=\"og:url\" content=\"{}\">\n",
            html_escape(canonical_url)
        ));
        tags.push_str("<meta property=\"og:type\" content=\"website\">\n");
        tags.push_str(&format!(
            "<meta property=\"og:site_name\" content=\"{}\">\n",
            html_escape(&self.title)
        ));
        tags.push_str(&format!(
            "<meta property=\"og:locale\" content=\"{}\">\n",
            html_escape(&self.lang)
        ));
        if !self.description.is_empty() {
            tags.push_str(&format!(
                "<meta property=\"og:description\" content=\"{}\">\n",
                html_escape(&self.description)
            ));
        }
        if !self.image.is_empty() {
            tags.push_str(&format!(
                "<meta property=\"og:image\" content=\"{}\">\n",
                html_escape(&self.image)
            ));
            tags.push_str("<meta property=\"og:image:width\" content=\"1200\">\n");
            tags.push_str("<meta property=\"og:image:height\" content=\"630\">\n");
        }

        // Twitter Card
        tags.push_str(&format!(
            "<meta name=\"twitter:card\" content=\"{}\">\n",
            html_escape(&self.twitter_card)
        ));
        tags.push_str(&format!(
            "<meta name=\"twitter:title\" content=\"{}\">\n",
            html_escape(&self.title)
        ));
        if !self.description.is_empty() {
            tags.push_str(&format!(
                "<meta name=\"twitter:description\" content=\"{}\">\n",
                html_escape(&self.description)
            ));
        }
        if !self.image.is_empty() {
            tags.push_str(&format!(
                "<meta name=\"twitter:image\" content=\"{}\">\n",
                html_escape(&self.image)
            ));
        }

        // LinkedIn / professional platforms
        if !self.author.is_empty() {
            tags.push_str(&format!(
                "<meta name=\"article:author\" content=\"{}\">\n",
                html_escape(&self.author)
            ));
        }

        // Discord / Slack / WhatsApp rich previews (use OG as base)
        // These platforms read og: tags, but we add specific helpers
        if !self.image.is_empty() {
            tags.push_str(&format!(
                "<meta property=\"og:image:alt\" content=\"{}\">\n",
                html_escape(&self.description)
            ));
        }

        // Favicon
        if !self.favicon.is_empty() {
            tags.push_str(&format!(
                "<link rel=\"icon\" href=\"{}\">\n",
                html_escape(&self.favicon)
            ));
            tags.push_str(&format!(
                "<link rel=\"apple-touch-icon\" href=\"{}\">\n",
                html_escape(&self.favicon)
            ));
        }

        // Canonical
        tags.push_str(&format!(
            "<link rel=\"canonical\" href=\"{}\">\n",
            html_escape(canonical_url)
        ));

        tags
    }

    /// Generate a placeholder SVG image for og:image when no image is provided.
    /// This creates a branded card with the project title and description.
    pub fn generate_og_svg(&self) -> String {
        let title = html_escape(&self.title);
        let desc = if self.description.is_empty() {
            "RunBox Preview".to_string()
        } else {
            html_escape(&self.description)
        };
        let truncated_desc = if desc.chars().count() > 100 {
            let truncated: String = desc.chars().take(97).collect();
            format!("{truncated}...")
        } else {
            desc
        };

        let mut svg = String::new();
        svg.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1200\" height=\"630\" viewBox=\"0 0 1200 630\">\n");
        svg.push_str("  <defs>\n");
        svg.push_str(
            "    <linearGradient id=\"bg\" x1=\"0%\" y1=\"0%\" x2=\"100%\" y2=\"100%\">\n",
        );
        svg.push_str("      <stop offset=\"0%\" style=\"stop-color:#1a1b2e\"/>\n");
        svg.push_str("      <stop offset=\"100%\" style=\"stop-color:#16213e\"/>\n");
        svg.push_str("    </linearGradient>\n");
        svg.push_str("  </defs>\n");
        svg.push_str("  <rect width=\"1200\" height=\"630\" fill=\"url(#bg)\"/>\n");
        svg.push_str("  <rect x=\"40\" y=\"40\" width=\"1120\" height=\"550\" rx=\"16\" fill=\"#0f1123\" stroke=\"#333\" stroke-width=\"1\"/>\n");
        svg.push_str(&format!("  <text x=\"100\" y=\"200\" font-family=\"sans-serif\" font-size=\"56\" font-weight=\"bold\" fill=\"#ffffff\">{title}</text>\n"));
        svg.push_str(&format!("  <text x=\"100\" y=\"280\" font-family=\"sans-serif\" font-size=\"24\" fill=\"#8888aa\">{truncated_desc}</text>\n"));
        svg.push_str("  <text x=\"100\" y=\"520\" font-family=\"sans-serif\" font-size=\"18\" fill=\"#666688\">Powered by RunBox</text>\n");
        svg.push_str("  <rect x=\"100\" y=\"340\" width=\"200\" height=\"8\" rx=\"4\" fill=\"#4f46e5\" opacity=\"0.5\"/>\n");
        svg.push_str("  <rect x=\"100\" y=\"370\" width=\"140\" height=\"8\" rx=\"4\" fill=\"#4f46e5\" opacity=\"0.3\"/>\n");
        svg.push_str("  <rect x=\"100\" y=\"400\" width=\"260\" height=\"8\" rx=\"4\" fill=\"#4f46e5\" opacity=\"0.2\"/>\n");
        svg.push_str("</svg>");
        svg
    }

    /// Generate a dynamic OG card as a data URI for use as og:image fallback.
    pub fn og_image_data_uri(&self) -> String {
        let svg = self.generate_og_svg();
        let encoded = base64_encode(svg.as_bytes());
        format!("data:image/svg+xml;base64,{encoded}")
    }

    /// Generate meta tags optimized for a specific platform.
    pub fn platform_meta_tags(&self, platform: SocialPlatform, canonical_url: &str) -> String {
        match platform {
            SocialPlatform::Twitter => {
                let mut tags = String::new();
                tags.push_str(&format!(
                    "<meta name=\"twitter:card\" content=\"{}\">\n",
                    html_escape(&self.twitter_card)
                ));
                tags.push_str(&format!(
                    "<meta name=\"twitter:title\" content=\"{}\">\n",
                    html_escape(&self.title)
                ));
                if !self.description.is_empty() {
                    tags.push_str(&format!(
                        "<meta name=\"twitter:description\" content=\"{}\">\n",
                        html_escape(&self.description)
                    ));
                }
                if !self.image.is_empty() {
                    tags.push_str(&format!(
                        "<meta name=\"twitter:image\" content=\"{}\">\n",
                        html_escape(&self.image)
                    ));
                }
                tags
            }
            SocialPlatform::LinkedIn => {
                let mut tags = String::new();
                tags.push_str(&format!(
                    "<meta property=\"og:title\" content=\"{}\">\n",
                    html_escape(&self.title)
                ));
                tags.push_str("<meta property=\"og:type\" content=\"website\">\n");
                tags.push_str(&format!(
                    "<meta property=\"og:url\" content=\"{}\">\n",
                    html_escape(canonical_url)
                ));
                if !self.description.is_empty() {
                    tags.push_str(&format!(
                        "<meta property=\"og:description\" content=\"{}\">\n",
                        html_escape(&self.description)
                    ));
                }
                if !self.image.is_empty() {
                    tags.push_str(&format!(
                        "<meta property=\"og:image\" content=\"{}\">\n",
                        html_escape(&self.image)
                    ));
                }
                tags
            }
            SocialPlatform::Discord => {
                // Discord uses OG tags + theme-color for embed color
                let mut tags = self.to_meta_tags(canonical_url);
                tags.push_str(&format!(
                    "<meta name=\"theme-color\" content=\"{}\">\n",
                    html_escape(&self.theme_color)
                ));
                tags
            }
            SocialPlatform::Slack => {
                // Slack uses OG tags
                self.to_meta_tags(canonical_url)
            }
            SocialPlatform::WhatsApp => {
                // WhatsApp uses OG tags, prefers og:image
                self.to_meta_tags(canonical_url)
            }
        }
    }
}

/// Plataformas sociales soportadas para meta tags específicos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SocialPlatform {
    Twitter,
    LinkedIn,
    Discord,
    Slack,
    WhatsApp,
}

/// Simple base64 encoding (no padding).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ── Preview session ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    /// Not started yet.
    Idle,
    /// Running and serving requests.
    Running,
    /// Stopped.
    Stopped,
    /// Error state.
    Error(String),
}

/// An active preview session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewSession {
    /// Unique session identifier.
    pub id: String,
    /// Current status.
    pub status: PreviewStatus,
    /// The configuration for this session.
    pub config: PreviewConfig,
    /// Timestamp (ms) when the session started.
    pub started_at: Option<u64>,
    /// Number of requests served.
    pub request_count: u64,
    /// Share token for generating unique share URLs.
    pub share_token: Option<String>,
}

impl PreviewSession {
    /// Create a new idle session with the given config.
    pub fn new(config: PreviewConfig) -> Self {
        let id = generate_session_id();
        Self {
            id,
            status: PreviewStatus::Idle,
            config,
            started_at: None,
            request_count: 0,
            share_token: None,
        }
    }

    /// Start the session.
    pub fn start(&mut self, now_ms: u64) {
        self.status = PreviewStatus::Running;
        self.started_at = Some(now_ms);
    }

    /// Stop the session.
    pub fn stop(&mut self) {
        self.status = PreviewStatus::Stopped;
    }

    /// Mark the session as errored.
    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.status = PreviewStatus::Error(msg.into());
    }

    /// Generate a share token if one doesn't exist.
    pub fn generate_share_token(&mut self) -> &str {
        if self.share_token.is_none() {
            self.share_token = Some(generate_share_token());
        }
        self.share_token.as_deref().unwrap()
    }

    /// Build the base URL for this preview.
    pub fn base_url(&self) -> String {
        let scheme = if self.config.https { "https" } else { "http" };
        if let Some(ref domain) = self.config.domain {
            format!(
                "{scheme}://{domain}{}",
                normalize_base_path(&self.config.base_path)
            )
        } else {
            format!(
                "{scheme}://localhost:{}{}",
                self.config.port,
                normalize_base_path(&self.config.base_path)
            )
        }
    }

    /// Build a share URL (uses custom domain if configured, otherwise localhost).
    pub fn share_url(&self) -> String {
        let base = self.base_url();
        if let Some(ref token) = self.share_token {
            format!("{base}?share={token}")
        } else {
            base
        }
    }

    /// Increment the request counter.
    pub fn record_request(&mut self) {
        self.request_count += 1;
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

// ── Preview manager ──────────────────────────────────────────────────────────

/// Manages preview sessions. Supports one active session at a time.
#[derive(Debug)]
pub struct PreviewManager {
    /// The current active session (if any).
    pub session: Option<PreviewSession>,
    /// History of past sessions (limited).
    history: Vec<PreviewSession>,
    /// Maximum history entries.
    max_history: usize,
}

impl PreviewManager {
    pub fn new() -> Self {
        Self {
            session: None,
            history: Vec::new(),
            max_history: 10,
        }
    }

    /// Start a new preview session with the given config.
    /// Stops the current session if one is active.
    pub fn start(&mut self, config: PreviewConfig, now_ms: u64) -> &PreviewSession {
        // Stop current session if active
        if let Some(mut old) = self.session.take() {
            old.stop();
            if self.history.len() >= self.max_history {
                self.history.remove(0);
            }
            self.history.push(old);
        }

        let mut session = PreviewSession::new(config);
        session.start(now_ms);
        self.session = Some(session);
        self.session.as_ref().unwrap()
    }

    /// Stop the current session.
    pub fn stop(&mut self) -> Result<()> {
        match self.session.as_mut() {
            Some(s) => {
                s.stop();
                Ok(())
            }
            None => Err(RunboxError::Runtime("no active preview session".into())),
        }
    }

    /// Get the current session (if any).
    pub fn current(&self) -> Option<&PreviewSession> {
        self.session.as_ref()
    }

    /// Get a mutable reference to the current session.
    pub fn current_mut(&mut self) -> Option<&mut PreviewSession> {
        self.session.as_mut()
    }

    /// Check if a preview is currently running.
    pub fn is_running(&self) -> bool {
        matches!(
            self.session.as_ref().map(|s| &s.status),
            Some(PreviewStatus::Running)
        )
    }

    /// Update the configuration of the current session.
    pub fn update_config(&mut self, config: PreviewConfig) -> Result<()> {
        match self.session.as_mut() {
            Some(s) => {
                s.config = config;
                Ok(())
            }
            None => Err(RunboxError::Runtime("no active preview session".into())),
        }
    }

    /// Set the custom domain for the current session.
    pub fn set_domain(&mut self, domain: &str) -> Result<()> {
        match self.session.as_mut() {
            Some(s) => {
                s.config.domain = Some(domain.to_string());
                Ok(())
            }
            None => Err(RunboxError::Runtime("no active preview session".into())),
        }
    }

    /// Generate a share URL for the current session.
    pub fn share(&mut self) -> Result<String> {
        match self.session.as_mut() {
            Some(s) => {
                s.generate_share_token();
                Ok(s.share_url())
            }
            None => Err(RunboxError::Runtime("no active preview session".into())),
        }
    }

    /// Get session history.
    pub fn history(&self) -> &[PreviewSession] {
        &self.history
    }

    /// Status as JSON.
    pub fn status_json(&self) -> String {
        match &self.session {
            Some(s) => s.to_json(),
            None => serde_json::json!({ "status": "idle", "session": null }).to_string(),
        }
    }
}

impl Default for PreviewManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Preview router ───────────────────────────────────────────────────────────

/// Handles an incoming preview request and produces a response.
/// This extends `handle_sw_request` with CORS, custom headers, live-reload
/// injection, and metadata injection.
pub fn handle_preview_request(
    req: &crate::network::SwRequest,
    vfs: &Vfs,
    session: &mut PreviewSession,
) -> crate::network::SwResponse {
    session.record_request();

    let path = extract_preview_path(&req.url, &session.config.base_path);

    // Handle CORS preflight
    if req.method.eq_ignore_ascii_case("OPTIONS") {
        let origin = req.headers.get("origin").map(|s| s.as_str());
        let cors_headers = session.config.cors.headers_for(origin);
        return crate::network::SwResponse {
            id: req.id.clone(),
            status: 204,
            headers: cors_headers,
            body: String::new(),
        };
    }

    // Check allowed paths (if configured)
    if !session.config.allowed_paths.is_empty()
        && !session
            .config
            .allowed_paths
            .iter()
            .any(|p| path.starts_with(p))
    {
        return crate::network::SwResponse::error(&req.id, "path not allowed");
    }

    // Try to serve from VFS
    let mut response = if let Ok(bytes) = vfs.read(&path) {
        let ct = mime_for_path_extended(&path);
        let body = String::from_utf8_lossy(bytes).to_string();

        // Inject live-reload and metadata into HTML responses
        let body = if ct.starts_with("text/html") && session.config.live_reload {
            inject_into_html(&body, session)
        } else {
            body
        };

        crate::network::SwResponse::ok(&req.id, body, ct)
    } else if session.config.spa && !path.contains('.') {
        // SPA fallback — serve index.html
        if let Ok(bytes) = vfs.read("/index.html") {
            let body = String::from_utf8_lossy(bytes).to_string();
            let body = if session.config.live_reload {
                inject_into_html(&body, session)
            } else {
                body
            };
            crate::network::SwResponse::ok(&req.id, body, "text/html; charset=utf-8")
        } else {
            crate::network::SwResponse::not_found(&req.id)
        }
    } else {
        crate::network::SwResponse::not_found(&req.id)
    };

    // Add CORS headers
    let origin = req.headers.get("origin").map(|s| s.as_str());
    let cors_headers = session.config.cors.headers_for(origin);
    for (k, v) in cors_headers {
        response.headers.insert(k, v);
    }

    // Add custom headers
    for (k, v) in &session.config.custom_headers {
        response.headers.insert(k.clone(), v.clone());
    }

    // Add cache control
    response
        .headers
        .entry("Cache-Control".into())
        .or_insert_with(|| "no-cache, no-store, must-revalidate".into());

    response
}

// ── HTML injection ──────────────────────────────────────────────────────────

/// Case-insensitive search for an ASCII needle in a string.
/// Returns the byte position in the *original* string, avoiding the
/// byte-length mismatch that `str::to_lowercase().find()` causes with
/// multi-byte Unicode characters (e.g. İ → i̇ changes byte length).
fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
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

/// Inject live-reload script and meta tags into an HTML document.
fn inject_into_html(html: &str, session: &PreviewSession) -> String {
    let mut result = html.to_string();

    // Build the meta tags
    let canonical = session.base_url();
    let meta_tags = session.config.metadata.to_meta_tags(&canonical);

    // Build the live-reload script
    let live_reload_script = LIVE_RELOAD_SCRIPT;

    // Inject meta tags after <head> (or at the beginning if no <head>)
    // Uses ASCII case-insensitive search on the original string to avoid
    // byte-position misalignment from to_lowercase() on multi-byte chars.
    if let Some(pos) = find_ascii_case_insensitive(&result, "<head>") {
        let insert_pos = pos + 6; // after "<head>"
        result.insert_str(insert_pos, &format!("\n{meta_tags}"));
    } else if let Some(pos) = find_ascii_case_insensitive(&result, "<html>") {
        let insert_pos = pos + 6;
        result.insert_str(insert_pos, &format!("\n<head>\n{meta_tags}</head>\n"));
    }

    // Inject live-reload script before </body> (or at the end)
    if let Some(pos) = find_ascii_case_insensitive(&result, "</body>") {
        result.insert_str(pos, live_reload_script);
    } else {
        result.push_str(live_reload_script);
    }

    result
}

/// The live-reload client script injected into HTML pages.
/// Uses postMessage to communicate with the RunBox host.
const LIVE_RELOAD_SCRIPT: &str = r#"
<script data-runbox-live-reload>
(function() {
  'use strict';
  var RUNBOX_LR = window.__RUNBOX_LIVE_RELOAD || {};
  RUNBOX_LR.connected = false;

  // Listen for reload messages from the RunBox host
  window.addEventListener('message', function(event) {
    var data = event.data;
    if (!data || !data.__runbox) return;

    switch (data.type) {
      case 'reload':
        if (data.hard) {
          window.location.reload();
        } else {
          // Soft reload — try to update CSS only
          var links = document.querySelectorAll('link[rel="stylesheet"]');
          links.forEach(function(link) {
            var href = link.href;
            if (href) {
              var url = new URL(href);
              url.searchParams.set('_rb_t', Date.now());
              link.href = url.toString();
            }
          });
        }
        break;

      case 'inject_css':
        // Hot-inject updated CSS files
        if (data.paths && data.paths.length) {
          data.paths.forEach(function(path) {
            var links = document.querySelectorAll('link[rel="stylesheet"]');
            links.forEach(function(link) {
              if (link.href && link.href.includes(path)) {
                var url = new URL(link.href);
                url.searchParams.set('_rb_t', Date.now());
                link.href = url.toString();
              }
            });
          });
        }
        break;

      case 'hmr':
        // Full reload as fallback (true HMR requires bundler support)
        window.location.reload();
        break;

      case 'navigate':
        if (data.url) {
          window.location.href = data.url;
        }
        break;

      case 'request_screenshot':
        // Auto-generate screenshot for og:image
        // In a real environment, this utilizes html2canvas or native mediaDevices API
        try {
          if (window.html2canvas) {
            window.html2canvas(document.body).then(canvas => {
                var dataUrl = canvas.toDataURL("image/png");
                window.parent.postMessage({ __runbox: true, type: 'screenshot_result', dataUrl: dataUrl }, '*');
            });
          } else {
            console.warn('[RunBox] html2canvas not found for auto-screenshot');
            // Mock response if not available in current dom
            window.parent.postMessage({ __runbox: true, type: 'screenshot_result', error: "not_available" }, '*');
          }
        } catch(e) {
          console.error(e);
        }
        break;
    }
  });

  // Notify host that we're ready for live-reload
  if (window.parent !== window) {
    window.parent.postMessage({ __runbox: true, type: 'lr_ready' }, '*');
  }

  RUNBOX_LR.connected = true;
  window.__RUNBOX_LIVE_RELOAD = RUNBOX_LR;
  console.log('[RunBox] Live reload connected');
})();
</script>
"#;

// ── Extended MIME types ──────────────────────────────────────────────────────

/// Extended MIME type detection with more file types than the base network module.
pub fn mime_for_path_extended(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        // Web fundamentals
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "ts" | "tsx" => "text/typescript; charset=utf-8",
        "jsx" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "jsonld" => "application/ld+json",
        "xml" => "application/xml; charset=utf-8",

        // Images
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",

        // Fonts
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",

        // Media
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",

        // Documents
        "pdf" => "application/pdf",
        "txt" => "text/plain; charset=utf-8",
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "yaml" | "yml" => "text/yaml; charset=utf-8",
        "toml" => "text/toml; charset=utf-8",

        // Web manifests
        "webmanifest" => "application/manifest+json",
        "map" => "application/json",

        // Source maps & misc
        "wasm" => "application/wasm",
        "glsl" | "vert" | "frag" => "text/plain; charset=utf-8",

        _ => "application/octet-stream",
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract the path from a URL, stripping the base path prefix.
fn extract_preview_path(url: &str, base_path: &str) -> String {
    let mut path = crate::network::extract_path(url);

    // Strip base path prefix
    let normalized_base = normalize_base_path(base_path);
    if normalized_base != "/"
        && let Some(rest) = path.strip_prefix(normalized_base.as_str())
    {
        path = if rest.is_empty() {
            "/".to_string()
        } else {
            rest.to_string()
        };
    }

    // Default to /index.html for root
    if path == "/" {
        "/index.html".to_string()
    } else {
        path
    }
}

/// Normalize a base path: ensure leading slash, no trailing slash.
fn normalize_base_path(base: &str) -> String {
    let mut p = base.to_string();
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    if p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

/// Generate a unique session ID (8 hex chars from a simple hash).
fn generate_session_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    format!("{:08x}", hasher.finish() & 0xFFFFFFFF)
}

/// Generate a share token (12 alphanumeric chars).
fn generate_share_token() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    nanos.hash(&mut hasher);
    // Mix in extra entropy
    (nanos.wrapping_mul(6364136223846793005).wrapping_add(1)).hash(&mut hasher);

    let hash = hasher.finish();
    // Use base36 for a more compact, URL-friendly token
    base36_encode(hash).to_string()
}

/// Simple base36 encoding of a u64.
fn base36_encode(mut n: u64) -> String {
    const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut result = Vec::new();
    while n > 0 {
        result.push(CHARS[(n % 36) as usize]);
        n /= 36;
    }
    result.reverse();
    String::from_utf8(result).unwrap_or_default()
}

/// Simple HTML escaping for attribute values.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = PreviewConfig::default();
        assert_eq!(config.port, 3000);
        assert_eq!(config.base_path, "/");
        assert!(config.live_reload);
        assert!(config.spa);
        assert!(!config.https);
        assert!(config.domain.is_none());
    }

    #[test]
    fn session_lifecycle() {
        let config = PreviewConfig::default();
        let mut session = PreviewSession::new(config);
        assert_eq!(session.status, PreviewStatus::Idle);

        session.start(1000);
        assert_eq!(session.status, PreviewStatus::Running);
        assert_eq!(session.started_at, Some(1000));

        session.record_request();
        session.record_request();
        assert_eq!(session.request_count, 2);

        session.stop();
        assert_eq!(session.status, PreviewStatus::Stopped);
    }

    #[test]
    fn base_url_without_domain() {
        let config = PreviewConfig {
            port: 8080,
            ..Default::default()
        };
        let session = PreviewSession::new(config);
        assert_eq!(session.base_url(), "http://localhost:8080/");
    }

    #[test]
    fn base_url_with_domain() {
        let config = PreviewConfig {
            domain: Some("preview.myapp.com".into()),
            https: true,
            ..Default::default()
        };
        let session = PreviewSession::new(config);
        assert_eq!(session.base_url(), "https://preview.myapp.com/");
    }

    #[test]
    fn base_url_with_base_path() {
        let config = PreviewConfig {
            domain: Some("myapp.com".into()),
            base_path: "/app".into(),
            https: true,
            ..Default::default()
        };
        let session = PreviewSession::new(config);
        assert_eq!(session.base_url(), "https://myapp.com/app");
    }

    #[test]
    fn share_url_generation() {
        let config = PreviewConfig::default();
        let mut session = PreviewSession::new(config);
        session.generate_share_token();
        let url = session.share_url();
        assert!(url.starts_with("http://localhost:3000/?share="));
    }

    #[test]
    fn cors_headers_wildcard() {
        let cors = CorsConfig::default();
        let headers = cors.headers_for(Some("https://example.com"));
        assert_eq!(headers.get("Access-Control-Allow-Origin").unwrap(), "*");
    }

    #[test]
    fn cors_headers_specific_origin() {
        let cors = CorsConfig {
            allowed_origins: vec!["https://example.com".into()],
            ..Default::default()
        };
        let headers = cors.headers_for(Some("https://example.com"));
        assert_eq!(
            headers.get("Access-Control-Allow-Origin").unwrap(),
            "https://example.com"
        );

        // Disallowed origin
        let headers = cors.headers_for(Some("https://evil.com"));
        assert!(headers.is_empty());
    }

    #[test]
    fn cors_credentials_echoes_origin() {
        let cors = CorsConfig {
            allow_credentials: true,
            ..Default::default()
        };
        let headers = cors.headers_for(Some("https://mysite.com"));
        assert_eq!(
            headers.get("Access-Control-Allow-Origin").unwrap(),
            "https://mysite.com"
        );
        assert_eq!(
            headers.get("Access-Control-Allow-Credentials").unwrap(),
            "true"
        );
    }

    #[test]
    fn metadata_generates_og_tags() {
        let meta = PreviewMetadata {
            title: "My App".into(),
            description: "A cool app".into(),
            image: "https://example.com/img.png".into(),
            ..Default::default()
        };
        let tags = meta.to_meta_tags("https://myapp.com");
        assert!(tags.contains("og:title"));
        assert!(tags.contains("My App"));
        assert!(tags.contains("og:description"));
        assert!(tags.contains("A cool app"));
        assert!(tags.contains("og:image"));
        assert!(tags.contains("twitter:card"));
    }

    #[test]
    fn preview_manager_lifecycle() {
        let mut mgr = PreviewManager::new();
        assert!(!mgr.is_running());

        let config = PreviewConfig::default();
        mgr.start(config, 1000);
        assert!(mgr.is_running());

        let status = mgr.status_json();
        assert!(status.contains("running"));

        // Start a new session (old one goes to history)
        let config2 = PreviewConfig {
            port: 8080,
            ..Default::default()
        };
        mgr.start(config2, 2000);
        assert!(mgr.is_running());
        assert_eq!(mgr.history().len(), 1);

        mgr.stop().unwrap();
        assert!(!mgr.is_running());
    }

    #[test]
    fn set_domain_on_active_session() {
        let mut mgr = PreviewManager::new();
        mgr.start(PreviewConfig::default(), 0);
        mgr.set_domain("custom.example.com").unwrap();

        let session = mgr.current().unwrap();
        assert_eq!(session.config.domain.as_deref(), Some("custom.example.com"));
        assert!(session.base_url().contains("custom.example.com"));
    }

    #[test]
    fn extract_preview_path_strips_base() {
        assert_eq!(
            extract_preview_path("http://localhost:3000/app/page", "/app"),
            "/page"
        );
        assert_eq!(
            extract_preview_path("http://localhost:3000/style.css", "/"),
            "/style.css"
        );
        assert_eq!(
            extract_preview_path("http://localhost:3000/", "/"),
            "/index.html"
        );
    }

    #[test]
    fn normalize_base_path_variants() {
        assert_eq!(normalize_base_path("/"), "/");
        assert_eq!(normalize_base_path("/app"), "/app");
        assert_eq!(normalize_base_path("/app/"), "/app");
        assert_eq!(normalize_base_path("app"), "/app");
    }

    #[test]
    fn extended_mime_types() {
        assert!(mime_for_path_extended("/style.css").starts_with("text/css"));
        assert!(mime_for_path_extended("/app.ts").starts_with("text/typescript"));
        assert_eq!(mime_for_path_extended("/font.woff2"), "font/woff2");
        assert_eq!(mime_for_path_extended("/img.webp"), "image/webp");
        assert_eq!(mime_for_path_extended("/img.avif"), "image/avif");
        assert_eq!(mime_for_path_extended("/app.wasm"), "application/wasm");
        assert_eq!(
            mime_for_path_extended("/manifest.webmanifest"),
            "application/manifest+json"
        );
    }

    #[test]
    fn html_injection() {
        let config = PreviewConfig {
            metadata: PreviewMetadata {
                title: "Test App".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut session = PreviewSession::new(config);
        session.start(0);

        let html = "<html><head></head><body><h1>Hello</h1></body></html>";
        let result = inject_into_html(html, &session);

        assert!(result.contains("Test App"));
        assert!(result.contains("data-runbox-live-reload"));
        assert!(result.contains("og:title"));
    }

    #[test]
    fn preview_request_serves_file() {
        let mut vfs = Vfs::new();
        vfs.write("/index.html", b"<h1>Hello</h1>".to_vec())
            .unwrap();

        let config = PreviewConfig::default();
        let mut session = PreviewSession::new(config);
        session.start(0);

        let req = crate::network::SwRequest {
            id: "1".into(),
            method: "GET".into(),
            url: "http://localhost:3000/".into(),
            headers: HashMap::new(),
            body: None,
        };

        let resp = handle_preview_request(&req, &vfs, &mut session);
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("Hello"));
        assert_eq!(session.request_count, 1);
    }

    #[test]
    fn preview_request_cors_preflight() {
        let vfs = Vfs::new();
        let config = PreviewConfig::default();
        let mut session = PreviewSession::new(config);
        session.start(0);

        let mut headers = HashMap::new();
        headers.insert("origin".into(), "https://example.com".into());

        let req = crate::network::SwRequest {
            id: "1".into(),
            method: "OPTIONS".into(),
            url: "http://localhost:3000/api".into(),
            headers,
            body: None,
        };

        let resp = handle_preview_request(&req, &vfs, &mut session);
        assert_eq!(resp.status, 204);
        assert!(resp.headers.contains_key("Access-Control-Allow-Origin"));
    }

    #[test]
    fn preview_request_spa_fallback() {
        let mut vfs = Vfs::new();
        vfs.write("/index.html", b"<div id='app'></div>".to_vec())
            .unwrap();

        let config = PreviewConfig {
            spa: true,
            ..Default::default()
        };
        let mut session = PreviewSession::new(config);
        session.start(0);

        let req = crate::network::SwRequest {
            id: "1".into(),
            method: "GET".into(),
            url: "http://localhost:3000/about/team".into(),
            headers: HashMap::new(),
            body: None,
        };

        let resp = handle_preview_request(&req, &vfs, &mut session);
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("app"));
    }

    #[test]
    fn base36_encodes_correctly() {
        assert_eq!(base36_encode(0), "0");
        assert_eq!(base36_encode(35), "z");
        assert_eq!(base36_encode(36), "10");
    }

    #[test]
    fn html_escape_works() {
        assert_eq!(html_escape("a<b>c"), "a&lt;b&gt;c");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("a\"b"), "a&quot;b");
    }

    #[test]
    fn og_svg_generation() {
        let meta = PreviewMetadata {
            title: "My Project".into(),
            description: "A cool project".into(),
            ..Default::default()
        };
        let svg = meta.generate_og_svg();
        assert!(svg.contains("My Project"));
        assert!(svg.contains("svg"));
        assert!(svg.contains("1200"));
    }

    #[test]
    fn og_image_data_uri() {
        let meta = PreviewMetadata {
            title: "Test".into(),
            ..Default::default()
        };
        let uri = meta.og_image_data_uri();
        assert!(uri.starts_with("data:image/svg+xml;base64,"));
    }

    #[test]
    fn platform_specific_tags() {
        let meta = PreviewMetadata {
            title: "Platform Test".into(),
            description: "Testing platforms".into(),
            ..Default::default()
        };
        let twitter = meta.platform_meta_tags(SocialPlatform::Twitter, "https://example.com");
        assert!(twitter.contains("twitter:card"));
        assert!(twitter.contains("Platform Test"));

        let linkedin = meta.platform_meta_tags(SocialPlatform::LinkedIn, "https://example.com");
        assert!(linkedin.contains("og:title"));
    }

    #[test]
    fn metadata_has_site_name_and_locale() {
        let meta = PreviewMetadata {
            title: "Site".into(),
            lang: "es".into(),
            ..Default::default()
        };
        let tags = meta.to_meta_tags("https://example.com");
        assert!(tags.contains("og:site_name"));
        assert!(tags.contains("og:locale"));
        assert!(tags.contains("es"));
    }

    #[test]
    fn metadata_image_dimensions() {
        let meta = PreviewMetadata {
            title: "Test".into(),
            image: "https://img.example.com/og.png".into(),
            ..Default::default()
        };
        let tags = meta.to_meta_tags("https://example.com");
        assert!(tags.contains("og:image:width"));
        assert!(tags.contains("og:image:height"));
        assert!(tags.contains("og:image:alt"));
    }
}
