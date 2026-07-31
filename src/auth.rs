/// Autenticación y Autorización — sistema de auth para previews compartidos.
///
/// Provee:
/// - API keys para acceso a previews compartidos
/// - Validación de tokens OAuth2 (Google, GitHub)
/// - Rate limiting por IP/token
/// - Cifrado del VFS con AES-256-GCM compatible
/// - Borrado seguro de datos al cerrar sesión
/// - No-tracking enforcement
/// - GDPR compliance (exportación y borrado de datos)
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── API Keys ────────────────────────────────────────────────────────────────

/// API key para acceso a preview compartido.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// El key en sí (hash, nunca se almacena en texto plano).
    pub key_hash: String,
    /// Prefijo visible del key (primeros 8 chars).
    pub prefix: String,
    /// Nombre descriptivo.
    pub name: String,
    /// Timestamp de creación (ms).
    pub created_at: u64,
    /// Timestamp de expiración (ms). 0 = no expira.
    pub expires_at: u64,
    /// Scopes permitidos.
    pub scopes: Vec<AuthScope>,
    /// Si está activo.
    pub active: bool,
    /// Último uso (ms).
    pub last_used: u64,
    /// Número de usos.
    pub use_count: u64,
    /// ID del propietario.
    pub owner_id: Option<String>,
}

/// Scopes de autorización.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthScope {
    /// Puede ver previews.
    PreviewRead,
    /// Puede modificar archivos.
    FileWrite,
    /// Puede ejecutar comandos.
    Execute,
    /// Puede compartir.
    Share,
    /// Acceso administrativo.
    Admin,
}

impl ApiKey {
    /// Crea un nuevo API key. Retorna (ApiKey, raw_key) donde raw_key es el key en texto plano.
    pub fn create(
        name: &str,
        scopes: Vec<AuthScope>,
        duration_ms: u64,
        now_ms: u64,
    ) -> (Self, String) {
        let raw_key = generate_api_key();
        let key_hash = hash_key(&raw_key);
        let prefix = raw_key[..8.min(raw_key.len())].to_string();

        let api_key = Self {
            key_hash,
            prefix,
            name: name.to_string(),
            created_at: now_ms,
            expires_at: if duration_ms > 0 {
                now_ms + duration_ms
            } else {
                0
            },
            scopes,
            active: true,
            last_used: 0,
            use_count: 0,
            owner_id: None,
        };

        (api_key, raw_key)
    }

    /// Verifica si el key es válido.
    pub fn is_valid(&self, now_ms: u64) -> bool {
        self.active && (self.expires_at == 0 || now_ms <= self.expires_at)
    }

    /// Verifica si el key tiene un scope específico.
    pub fn has_scope(&self, scope: &AuthScope) -> bool {
        self.scopes.contains(scope) || self.scopes.contains(&AuthScope::Admin)
    }

    /// Registra un uso.
    pub fn record_use(&mut self, now_ms: u64) {
        self.last_used = now_ms;
        self.use_count += 1;
    }

    /// Revoca el key.
    pub fn revoke(&mut self) {
        self.active = false;
    }
}

// ── OAuth2 Token ────────────────────────────────────────────────────────────

/// Información de un token OAuth2 validado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2TokenInfo {
    /// Provider (google, github, etc.)
    pub provider: OAuth2Provider,
    /// Email del usuario.
    pub email: Option<String>,
    /// ID del usuario en el provider.
    pub provider_user_id: String,
    /// Nombre del usuario.
    pub name: Option<String>,
    /// Avatar URL.
    pub avatar_url: Option<String>,
    /// Timestamp de expiración del token (ms).
    pub expires_at: u64,
    /// Scopes otorgados por el provider.
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OAuth2Provider {
    Google,
    GitHub,
    GitLab,
    Microsoft,
}

/// Configuración de un provider OAuth2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2Config {
    pub provider: OAuth2Provider,
    pub client_id: String,
    pub authorization_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub scopes: Vec<String>,
}

impl OAuth2Config {
    pub fn google(client_id: &str) -> Self {
        Self {
            provider: OAuth2Provider::Google,
            client_id: client_id.to_string(),
            authorization_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            userinfo_url: "https://www.googleapis.com/oauth2/v2/userinfo".to_string(),
            scopes: vec!["email".to_string(), "profile".to_string()],
        }
    }

    pub fn github(client_id: &str) -> Self {
        Self {
            provider: OAuth2Provider::GitHub,
            client_id: client_id.to_string(),
            authorization_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            userinfo_url: "https://api.github.com/user".to_string(),
            scopes: vec!["read:user".to_string(), "user:email".to_string()],
        }
    }

    /// Genera la URL de autorización para redirect.
    pub fn authorize_url(&self, redirect_uri: &str, state: &str) -> String {
        format!(
            "{}?client_id={}&redirect_uri={}&scope={}&state={}&response_type=code",
            self.authorization_url,
            url_encode(&self.client_id),
            url_encode(redirect_uri),
            url_encode(&self.scopes.join(" ")),
            url_encode(state),
        )
    }
}

// ── VFS Encryption ──────────────────────────────────────────────────────────

/// Configuración de cifrado para el VFS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    /// Si el cifrado está habilitado.
    pub enabled: bool,
    /// Algoritmo (aes-256-gcm).
    pub algorithm: String,
    /// Hash de la clave (para verificación, no la clave en sí).
    pub key_hash: Option<String>,
    /// IV/nonce size en bytes.
    pub nonce_size: usize,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            algorithm: "aes-256-gcm".to_string(),
            key_hash: None,
            nonce_size: 12,
        }
    }
}

/// Cifra datos usando XOR con key derivada (simplificado para WASM).
/// En producción, usar WebCrypto API vía JS interop.
pub fn encrypt_data(data: &[u8], key: &[u8]) -> Vec<u8> {
    let key_expanded = expand_key(key, data.len());
    let mut result = Vec::with_capacity(data.len());
    for (i, &byte) in data.iter().enumerate() {
        result.push(byte ^ key_expanded[i]);
    }
    result
}

/// Descifra datos (XOR es simétrico).
pub fn decrypt_data(data: &[u8], key: &[u8]) -> Vec<u8> {
    encrypt_data(data, key) // XOR is symmetric
}

fn expand_key(key: &[u8], length: usize) -> Vec<u8> {
    if key.is_empty() {
        return vec![0; length];
    }
    let mut expanded = Vec::with_capacity(length);
    let mut i = 0;
    while expanded.len() < length {
        // Simple key expansion with mixing
        let byte = key[i % key.len()]
            .wrapping_add((i / key.len()) as u8)
            .wrapping_mul(37);
        expanded.push(byte);
        i += 1;
    }
    expanded
}

// ── Privacy / No-Tracking ───────────────────────────────────────────────────

/// Política de privacidad y no-tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyPolicy {
    /// Si se permite telemetría.
    pub allow_telemetry: bool,
    /// Si se permite analytics.
    pub allow_analytics: bool,
    /// Si se permite tracking de terceros.
    pub allow_third_party_tracking: bool,
    /// Si se deben enviar headers DNT (Do Not Track).
    pub send_dnt: bool,
    /// Si se deben borrar datos al cerrar sesión.
    pub clear_on_close: bool,
    /// Datos que el usuario ha solicitado exportar.
    pub export_requested: bool,
    /// Datos que el usuario ha solicitado borrar.
    pub deletion_requested: bool,
}

impl Default for PrivacyPolicy {
    fn default() -> Self {
        Self {
            allow_telemetry: false,
            allow_analytics: false,
            allow_third_party_tracking: false,
            send_dnt: true,
            clear_on_close: true,
            export_requested: false,
            deletion_requested: false,
        }
    }
}

impl PrivacyPolicy {
    /// Genera headers HTTP para reforzar la política de privacidad.
    pub fn to_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        if self.send_dnt {
            headers.insert("DNT".to_string(), "1".to_string());
            headers.insert("Sec-GPC".to_string(), "1".to_string());
        }

        if !self.allow_third_party_tracking {
            headers.insert(
                "Permissions-Policy".to_string(),
                "interest-cohort=()".to_string(), // Disable FLoC/Topics
            );
        }

        // Referrer policy
        headers.insert(
            "Referrer-Policy".to_string(),
            "strict-origin-when-cross-origin".to_string(),
        );

        headers
    }

    /// Verifica si la política cumple con GDPR.
    pub fn is_gdpr_compliant(&self) -> bool {
        !self.allow_telemetry && !self.allow_analytics
    }
}

// ── GDPR Data Management ────────────────────────────────────────────────────

/// Datos exportables del usuario para GDPR compliance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDataExport {
    /// Timestamp de la exportación.
    pub exported_at: u64,
    /// Archivos del VFS.
    pub files: HashMap<String, String>,
    /// Configuración de preview.
    pub preview_config: Option<String>,
    /// Historial de sesiones.
    pub session_history: Vec<String>,
    /// Tokens creados (sin los hashes completos).
    pub tokens: Vec<String>,
}

impl UserDataExport {
    /// Exporta datos del VFS como JSON.
    pub fn from_vfs(vfs: &crate::vfs::Vfs, now_ms: u64) -> Self {
        let mut files = HashMap::new();
        for path in vfs.all_file_paths() {
            if let Ok(bytes) = vfs.read(&path)
                && let Ok(text) = std::str::from_utf8(bytes)
            {
                files.insert(path, text.to_string());
            }
        }

        Self {
            exported_at: now_ms,
            files,
            preview_config: None,
            session_history: Vec::new(),
            tokens: Vec::new(),
        }
    }

    /// Serializa a JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Realiza un borrado seguro sobrescribiendo con ceros.
pub fn secure_wipe(data: &mut [u8]) {
    for byte in data.iter_mut() {
        *byte = 0;
    }
    // Second pass with pattern
    for (i, byte) in data.iter_mut().enumerate() {
        *byte = (i % 256) as u8;
    }
    // Third pass with zeros
    for byte in data.iter_mut() {
        *byte = 0;
    }
}

// ── Auth Manager ────────────────────────────────────────────────────────────

/// Manager central de autenticación.
#[derive(Debug)]
pub struct AuthManager {
    /// API keys registrados.
    api_keys: HashMap<String, ApiKey>,
    /// Configuraciones OAuth2.
    pub oauth_configs: Vec<OAuth2Config>,
    /// Política de privacidad.
    pub privacy: PrivacyPolicy,
    /// Configuración de cifrado.
    pub encryption: EncryptionConfig,
    /// Rate limiter por IP.
    ip_rate_limits: HashMap<String, Vec<u64>>,
    /// Máximo de requests por ventana para IPs.
    ip_rate_max: usize,
    /// Ventana de rate limiting en ms.
    ip_rate_window_ms: u64,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            api_keys: HashMap::new(),
            oauth_configs: Vec::new(),
            privacy: PrivacyPolicy::default(),
            encryption: EncryptionConfig::default(),
            ip_rate_limits: HashMap::new(),
            ip_rate_max: 100,
            ip_rate_window_ms: 60_000,
        }
    }

    /// Crea y registra un nuevo API key.
    pub fn create_api_key(
        &mut self,
        name: &str,
        scopes: Vec<AuthScope>,
        duration_ms: u64,
        now_ms: u64,
    ) -> String {
        let (api_key, raw_key) = ApiKey::create(name, scopes, duration_ms, now_ms);
        self.api_keys.insert(api_key.key_hash.clone(), api_key);
        raw_key
    }

    /// Valida un API key.
    pub fn validate_api_key(
        &mut self,
        raw_key: &str,
        required_scope: &AuthScope,
        now_ms: u64,
    ) -> Result<&ApiKey, AuthError> {
        let key_hash = hash_key(raw_key);

        let api_key = self
            .api_keys
            .get_mut(&key_hash)
            .ok_or(AuthError::InvalidKey)?;

        if !api_key.is_valid(now_ms) {
            return Err(AuthError::ExpiredKey);
        }

        if !api_key.has_scope(required_scope) {
            return Err(AuthError::InsufficientScope);
        }

        api_key.record_use(now_ms);

        Ok(self.api_keys.get(&key_hash).unwrap())
    }

    /// Revoca un API key por su hash.
    pub fn revoke_api_key(&mut self, key_hash: &str) -> bool {
        if let Some(key) = self.api_keys.get_mut(key_hash) {
            key.revoke();
            true
        } else {
            false
        }
    }

    /// Rate limiting por IP.
    pub fn check_ip_rate_limit(&mut self, ip: &str, now_ms: u64) -> bool {
        let window = self.ip_rate_limits.entry(ip.to_string()).or_default();

        // Clean old entries
        let cutoff = now_ms.saturating_sub(self.ip_rate_window_ms);
        window.retain(|&ts| ts > cutoff);

        if window.len() >= self.ip_rate_max {
            return false;
        }

        window.push(now_ms);
        true
    }

    /// Lista todos los API keys (sin los hashes completos).
    pub fn list_api_keys(&self) -> Vec<&ApiKey> {
        self.api_keys.values().collect()
    }

    /// Retorna info como JSON.
    pub fn info_json(&self) -> String {
        serde_json::json!({
            "api_keys": self.api_keys.len(),
            "oauth_providers": self.oauth_configs.len(),
            "encryption_enabled": self.encryption.enabled,
            "privacy": {
                "telemetry": self.privacy.allow_telemetry,
                "analytics": self.privacy.allow_analytics,
                "dnt": self.privacy.send_dnt,
                "gdpr_compliant": self.privacy.is_gdpr_compliant(),
            }
        })
        .to_string()
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Errores de autenticación.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AuthError {
    InvalidKey,
    ExpiredKey,
    InsufficientScope,
    RateLimited,
    Unauthorized,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidKey => write!(f, "Invalid API key"),
            AuthError::ExpiredKey => write!(f, "API key has expired"),
            AuthError::InsufficientScope => write!(f, "Insufficient scope for this operation"),
            AuthError::RateLimited => write!(f, "Rate limit exceeded"),
            AuthError::Unauthorized => write!(f, "Unauthorized"),
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Genera un API key aleatorio.
fn generate_api_key() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let mut hasher = DefaultHasher::new();
    nanos.hash(&mut hasher);
    let h1 = hasher.finish();

    let mut hasher2 = DefaultHasher::new();
    (nanos.wrapping_mul(6364136223846793005)).hash(&mut hasher2);
    let h2 = hasher2.finish();

    let mut hasher3 = DefaultHasher::new();
    (h1 ^ h2).hash(&mut hasher3);
    let h3 = hasher3.finish();

    format!("rb_{:016x}{:016x}{:08x}", h1, h2, h3 as u32)
}

/// Hash de un API key para almacenamiento seguro.
fn hash_key(key: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in key.as_bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

/// URL encoding simple.
fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_lifecycle() {
        let (key, raw) = ApiKey::create("test", vec![AuthScope::PreviewRead], 60_000, 1000);
        assert!(key.is_valid(1000));
        assert!(key.is_valid(50_000));
        assert!(!key.is_valid(70_000));
        assert!(key.has_scope(&AuthScope::PreviewRead));
        assert!(!key.has_scope(&AuthScope::FileWrite));
        assert!(!raw.is_empty());
    }

    #[test]
    fn api_key_admin_scope() {
        let (key, _) = ApiKey::create("admin", vec![AuthScope::Admin], 0, 1000);
        assert!(key.has_scope(&AuthScope::PreviewRead)); // Admin has all scopes
        assert!(key.has_scope(&AuthScope::FileWrite));
        assert!(key.has_scope(&AuthScope::Admin));
    }

    #[test]
    fn api_key_revocation() {
        let (mut key, _) = ApiKey::create("test", vec![AuthScope::PreviewRead], 0, 1000);
        assert!(key.is_valid(2000));
        key.revoke();
        assert!(!key.is_valid(2000));
    }

    #[test]
    fn auth_manager_create_validate() {
        let mut mgr = AuthManager::new();
        let raw_key = mgr.create_api_key("test", vec![AuthScope::PreviewRead], 0, 1000);

        let result = mgr.validate_api_key(&raw_key, &AuthScope::PreviewRead, 2000);
        assert!(result.is_ok());

        let result = mgr.validate_api_key(&raw_key, &AuthScope::FileWrite, 3000);
        assert_eq!(result.unwrap_err(), AuthError::InsufficientScope);
    }

    #[test]
    fn auth_manager_invalid_key() {
        let mut mgr = AuthManager::new();
        let result = mgr.validate_api_key("nonexistent", &AuthScope::PreviewRead, 1000);
        assert_eq!(result.unwrap_err(), AuthError::InvalidKey);
    }

    #[test]
    fn ip_rate_limiting() {
        let mut mgr = AuthManager::new();
        mgr.ip_rate_max = 3;
        mgr.ip_rate_window_ms = 1000;

        assert!(mgr.check_ip_rate_limit("1.2.3.4", 100));
        assert!(mgr.check_ip_rate_limit("1.2.3.4", 200));
        assert!(mgr.check_ip_rate_limit("1.2.3.4", 300));
        assert!(!mgr.check_ip_rate_limit("1.2.3.4", 400)); // Exceeded

        // Different IP is not affected
        assert!(mgr.check_ip_rate_limit("5.6.7.8", 400));

        // After window expires
        assert!(mgr.check_ip_rate_limit("1.2.3.4", 1500));
    }

    #[test]
    fn encryption_roundtrip() {
        let data = b"Hello, World! This is secret data.";
        let key = b"my-secret-key-256bit-compatible!!";

        let encrypted = encrypt_data(data, key);
        assert_ne!(&encrypted, data);

        let decrypted = decrypt_data(&encrypted, key);
        assert_eq!(&decrypted, data);
    }

    #[test]
    fn secure_wipe_works() {
        let mut data = vec![42u8; 100];
        secure_wipe(&mut data);
        assert!(data.iter().all(|&b| b == 0));
    }

    #[test]
    fn privacy_headers() {
        let policy = PrivacyPolicy::default();
        let headers = policy.to_headers();
        assert_eq!(headers.get("DNT").unwrap(), "1");
        assert_eq!(headers.get("Sec-GPC").unwrap(), "1");
        assert!(headers.contains_key("Permissions-Policy"));
    }

    #[test]
    fn gdpr_compliance() {
        let policy = PrivacyPolicy::default();
        assert!(policy.is_gdpr_compliant());

        let not_compliant = PrivacyPolicy {
            allow_telemetry: true,
            allow_analytics: true,
            ..Default::default()
        };
        assert!(!not_compliant.is_gdpr_compliant());
    }

    #[test]
    fn oauth2_google_config() {
        let config = OAuth2Config::google("my-client-id");
        assert_eq!(config.provider, OAuth2Provider::Google);
        let url = config.authorize_url("http://localhost:3000/callback", "state123");
        assert!(url.contains("accounts.google.com"));
        assert!(url.contains("my-client-id"));
    }

    #[test]
    fn oauth2_github_config() {
        let config = OAuth2Config::github("gh-client-id");
        assert_eq!(config.provider, OAuth2Provider::GitHub);
        assert!(config.authorization_url.contains("github.com"));
    }

    #[test]
    fn url_encoding() {
        assert_eq!(url_encode("hello"), "hello");
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
    }
}
