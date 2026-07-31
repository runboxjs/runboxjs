/// Sesiones compartidas — gestión de tokens, permisos y accesos.
///
/// Provee:
/// - Tokens de acceso con expiración configurable
/// - Permisos por sesión: view (solo lectura), interact (terminal), edit (archivos)
/// - Panel de sesiones activas con tracking de viewers
/// - Revocación de tokens
/// - Historial de accesos
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Permission Levels ───────────────────────────────────────────────────────

/// Nivel de permisos para una sesión compartida.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SessionPermission {
    /// Solo lectura — puede ver el preview.
    #[default]
    View,
    /// Puede interactuar con el terminal.
    Interact,
    /// Puede modificar archivos del proyecto.
    Edit,
}

impl SessionPermission {
    /// Verifica si este nivel de permiso incluye la acción solicitada.
    pub fn allows(&self, action: &SessionAction) -> bool {
        match action {
            SessionAction::ViewPreview => true, // Todos pueden ver
            SessionAction::UseTerminal => {
                matches!(self, SessionPermission::Interact | SessionPermission::Edit)
            }
            SessionAction::EditFile => matches!(self, SessionPermission::Edit),
            SessionAction::ShareProject => matches!(self, SessionPermission::Edit),
        }
    }
}

/// Acciones que un usuario puede realizar en una sesión.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAction {
    ViewPreview,
    UseTerminal,
    EditFile,
    ShareProject,
}

// ── Share Token ─────────────────────────────────────────────────────────────

/// Token de acceso compartido con expiración y permisos.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareToken {
    /// El token en sí (string alfanumérico).
    pub token: String,
    /// Timestamp de creación (ms).
    pub created_at: u64,
    /// Timestamp de expiración (ms). 0 = no expira.
    pub expires_at: u64,
    /// Nivel de permisos otorgados.
    pub permission: SessionPermission,
    /// Si el token ha sido revocado.
    pub revoked: bool,
    /// Nombre descriptivo del token (opcional).
    pub label: Option<String>,
    /// Número de veces que se ha usado.
    pub use_count: u64,
    /// Número máximo de usos (0 = ilimitado).
    pub max_uses: u64,
    /// ID de la sesión de preview asociada.
    pub session_id: String,
}

impl ShareToken {
    /// Crea un nuevo token con duración en milisegundos.
    pub fn new(
        session_id: &str,
        permission: SessionPermission,
        duration_ms: u64,
        now_ms: u64,
    ) -> Self {
        Self {
            token: generate_token(),
            created_at: now_ms,
            expires_at: if duration_ms > 0 {
                now_ms + duration_ms
            } else {
                0
            },
            permission,
            revoked: false,
            label: None,
            use_count: 0,
            max_uses: 0,
            session_id: session_id.to_string(),
        }
    }

    /// Verifica si el token es válido (no expirado, no revocado, dentro del límite de usos).
    pub fn is_valid(&self, now_ms: u64) -> bool {
        if self.revoked {
            return false;
        }
        if self.expires_at > 0 && now_ms > self.expires_at {
            return false;
        }
        if self.max_uses > 0 && self.use_count >= self.max_uses {
            return false;
        }
        true
    }

    /// Revoca el token.
    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    /// Registra un uso del token.
    pub fn record_use(&mut self) {
        self.use_count += 1;
    }

    /// Retorna el tiempo restante en ms. 0 si ya expiró o no tiene expiración.
    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        if self.expires_at == 0 {
            return u64::MAX; // Sin expiración
        }
        self.expires_at.saturating_sub(now_ms)
    }
}

// ── Access Log ──────────────────────────────────────────────────────────────

/// Entrada del historial de accesos.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessEntry {
    pub id: u64,
    /// Token utilizado.
    pub token: String,
    /// Timestamp del acceso (ms).
    pub timestamp_ms: u64,
    /// IP o identificador del cliente (si disponible).
    pub client_hint: Option<String>,
    /// User-Agent del cliente.
    pub user_agent: Option<String>,
    /// Resultado del acceso.
    pub outcome: AccessOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessOutcome {
    /// Acceso exitoso.
    Granted,
    /// Token expirado.
    Expired,
    /// Token revocado.
    Revoked,
    /// Token no encontrado.
    NotFound,
    /// Permisos insuficientes.
    Forbidden,
}

/// Historial de accesos.
#[derive(Debug)]
pub struct AccessLog {
    entries: Vec<AccessEntry>,
    next_id: u64,
    max_entries: usize,
}

impl AccessLog {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            max_entries,
        }
    }

    /// Registra un acceso.
    pub fn log(
        &mut self,
        token: &str,
        timestamp_ms: u64,
        client_hint: Option<&str>,
        user_agent: Option<&str>,
        outcome: AccessOutcome,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.entries.push(AccessEntry {
            id,
            token: token.to_string(),
            timestamp_ms,
            client_hint: client_hint.map(|s| s.to_string()),
            user_agent: user_agent.map(|s| s.to_string()),
            outcome,
        });

        if self.entries.len() > self.max_entries {
            let drain = self.entries.len() - self.max_entries;
            self.entries.drain(..drain);
        }

        id
    }

    /// Retorna todas las entradas.
    pub fn entries(&self) -> &[AccessEntry] {
        &self.entries
    }

    /// Retorna entradas por token.
    pub fn by_token(&self, token: &str) -> Vec<&AccessEntry> {
        self.entries.iter().filter(|e| e.token == token).collect()
    }

    /// Retorna entradas desde un timestamp.
    pub fn since(&self, timestamp_ms: u64) -> Vec<&AccessEntry> {
        self.entries
            .iter()
            .filter(|e| e.timestamp_ms >= timestamp_ms)
            .collect()
    }

    /// Número de entradas.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Limpia el historial.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Serializa a JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.entries).unwrap_or_default()
    }
}

impl Default for AccessLog {
    fn default() -> Self {
        Self::new(10_000)
    }
}

// ── Session Manager ─────────────────────────────────────────────────────────

/// Manager central de sesiones compartidas.
#[derive(Debug)]
pub struct SessionManager {
    /// Tokens activos (token string → ShareToken).
    tokens: HashMap<String, ShareToken>,
    /// Historial de accesos.
    pub access_log: AccessLog,
    /// Duración por defecto de los tokens (ms). 0 = sin expiración.
    pub default_duration_ms: u64,
    /// Permiso por defecto para nuevos tokens.
    pub default_permission: SessionPermission,
    /// Número máximo de tokens activos.
    max_tokens: usize,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
            access_log: AccessLog::default(),
            default_duration_ms: 24 * 60 * 60 * 1000, // 24 horas
            default_permission: SessionPermission::View,
            max_tokens: 100,
        }
    }

    /// Crea un nuevo token de compartir.
    pub fn create_token(
        &mut self,
        session_id: &str,
        permission: SessionPermission,
        duration_ms: u64,
        now_ms: u64,
    ) -> &ShareToken {
        // Limpiar tokens expirados primero
        self.cleanup_expired(now_ms);

        // Limitar tokens activos
        if self.tokens.len() >= self.max_tokens {
            // Eliminar el más antiguo
            let oldest = self
                .tokens
                .iter()
                .min_by_key(|(_, t)| t.created_at)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest {
                self.tokens.remove(&key);
            }
        }

        let token = ShareToken::new(session_id, permission, duration_ms, now_ms);
        let key = token.token.clone();
        self.tokens.insert(key.clone(), token);
        self.tokens.get(&key).unwrap()
    }

    /// Crea un token con configuración por defecto.
    pub fn create_default_token(&mut self, session_id: &str, now_ms: u64) -> &ShareToken {
        let perm = self.default_permission.clone();
        let duration = self.default_duration_ms;
        self.create_token(session_id, perm, duration, now_ms)
    }

    /// Valida un token y registra el acceso.
    pub fn validate(
        &mut self,
        token_str: &str,
        action: &SessionAction,
        now_ms: u64,
        client_hint: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<&ShareToken, AccessOutcome> {
        let token = match self.tokens.get_mut(token_str) {
            Some(t) => t,
            None => {
                self.access_log.log(
                    token_str,
                    now_ms,
                    client_hint,
                    user_agent,
                    AccessOutcome::NotFound,
                );
                return Err(AccessOutcome::NotFound);
            }
        };

        if token.revoked {
            self.access_log.log(
                token_str,
                now_ms,
                client_hint,
                user_agent,
                AccessOutcome::Revoked,
            );
            return Err(AccessOutcome::Revoked);
        }

        if token.expires_at > 0 && now_ms > token.expires_at {
            self.access_log.log(
                token_str,
                now_ms,
                client_hint,
                user_agent,
                AccessOutcome::Expired,
            );
            return Err(AccessOutcome::Expired);
        }

        if token.max_uses > 0 && token.use_count >= token.max_uses {
            self.access_log.log(
                token_str,
                now_ms,
                client_hint,
                user_agent,
                AccessOutcome::Forbidden,
            );
            return Err(AccessOutcome::Forbidden);
        }

        if !token.permission.allows(action) {
            self.access_log.log(
                token_str,
                now_ms,
                client_hint,
                user_agent,
                AccessOutcome::Forbidden,
            );
            return Err(AccessOutcome::Forbidden);
        }

        token.record_use();
        self.access_log.log(
            token_str,
            now_ms,
            client_hint,
            user_agent,
            AccessOutcome::Granted,
        );

        // Re-borrow as immutable
        Ok(self.tokens.get(token_str).unwrap())
    }

    /// Revoca un token.
    pub fn revoke_token(&mut self, token_str: &str) -> bool {
        if let Some(token) = self.tokens.get_mut(token_str) {
            token.revoke();
            true
        } else {
            false
        }
    }

    /// Revoca todos los tokens de una sesión.
    pub fn revoke_session_tokens(&mut self, session_id: &str) {
        for token in self.tokens.values_mut() {
            if token.session_id == session_id {
                token.revoke();
            }
        }
    }

    /// Retorna todos los tokens activos (no revocados, no expirados).
    pub fn active_tokens(&self, now_ms: u64) -> Vec<&ShareToken> {
        self.tokens
            .values()
            .filter(|t| t.is_valid(now_ms))
            .collect()
    }

    /// Retorna todos los tokens de una sesión.
    pub fn session_tokens(&self, session_id: &str) -> Vec<&ShareToken> {
        self.tokens
            .values()
            .filter(|t| t.session_id == session_id)
            .collect()
    }

    /// Limpia tokens expirados.
    pub fn cleanup_expired(&mut self, now_ms: u64) {
        self.tokens.retain(|_, t| {
            // Keep if: no expiration, not yet expired, or revoked (keep for audit)
            t.expires_at == 0 || now_ms <= t.expires_at || t.revoked
        });
    }

    /// Número total de tokens.
    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    /// Retorna info como JSON.
    pub fn info_json(&self, now_ms: u64) -> String {
        serde_json::json!({
            "total_tokens": self.tokens.len(),
            "active_tokens": self.active_tokens(now_ms).len(),
            "access_log_entries": self.access_log.len(),
            "default_duration_ms": self.default_duration_ms,
            "default_permission": self.default_permission,
        })
        .to_string()
    }

    /// Retorna la lista de tokens como JSON (para el panel de sesiones activas).
    pub fn tokens_json(&self, now_ms: u64) -> String {
        let tokens: Vec<serde_json::Value> = self
            .tokens
            .values()
            .map(|t| {
                serde_json::json!({
                    "token": &t.token[..8.min(t.token.len())], // Solo primeros 8 chars
                    "permission": t.permission,
                    "created_at": t.created_at,
                    "expires_at": t.expires_at,
                    "revoked": t.revoked,
                    "use_count": t.use_count,
                    "valid": t.is_valid(now_ms),
                    "remaining_ms": if t.expires_at > 0 { t.remaining_ms(now_ms) } else { 0 },
                    "session_id": &t.session_id,
                })
            })
            .collect();
        serde_json::to_string(&tokens).unwrap_or_default()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Genera un token aleatorio de 16 caracteres alfanuméricos.
fn generate_token() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    let h1 = hasher.finish();

    let mut hasher2 = DefaultHasher::new();
    (h1.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407))
    .hash(&mut hasher2);
    let h2 = hasher2.finish();

    format!("{:016x}{:016x}", h1, h2).chars().take(24).collect()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_creation_and_validation() {
        let token = ShareToken::new("s1", SessionPermission::View, 60_000, 1000);
        assert!(token.is_valid(1000));
        assert!(token.is_valid(50_000));
        assert!(!token.is_valid(70_000)); // Expired
    }

    #[test]
    fn token_no_expiration() {
        let token = ShareToken::new("s1", SessionPermission::View, 0, 1000);
        assert!(token.is_valid(1000));
        assert!(token.is_valid(u64::MAX - 1));
    }

    #[test]
    fn token_revocation() {
        let mut token = ShareToken::new("s1", SessionPermission::Edit, 0, 1000);
        assert!(token.is_valid(2000));
        token.revoke();
        assert!(!token.is_valid(2000));
    }

    #[test]
    fn token_max_uses() {
        let mut token = ShareToken::new("s1", SessionPermission::View, 0, 1000);
        token.max_uses = 2;
        assert!(token.is_valid(2000));
        token.record_use();
        assert!(token.is_valid(2000));
        token.record_use();
        assert!(!token.is_valid(2000)); // Max uses reached
    }

    #[test]
    fn permission_allows() {
        let view = SessionPermission::View;
        let interact = SessionPermission::Interact;
        let edit = SessionPermission::Edit;

        assert!(view.allows(&SessionAction::ViewPreview));
        assert!(!view.allows(&SessionAction::UseTerminal));
        assert!(!view.allows(&SessionAction::EditFile));

        assert!(interact.allows(&SessionAction::ViewPreview));
        assert!(interact.allows(&SessionAction::UseTerminal));
        assert!(!interact.allows(&SessionAction::EditFile));

        assert!(edit.allows(&SessionAction::ViewPreview));
        assert!(edit.allows(&SessionAction::UseTerminal));
        assert!(edit.allows(&SessionAction::EditFile));
    }

    #[test]
    fn session_manager_create_and_validate() {
        let mut mgr = SessionManager::new();
        let token_str = {
            let token = mgr.create_token("s1", SessionPermission::View, 60_000, 1000);
            token.token.clone()
        };

        // Valid access
        let result = mgr.validate(&token_str, &SessionAction::ViewPreview, 2000, None, None);
        assert!(result.is_ok());

        // Forbidden action
        let result = mgr.validate(&token_str, &SessionAction::EditFile, 3000, None, None);
        assert_eq!(result.unwrap_err(), AccessOutcome::Forbidden);
    }

    #[test]
    fn session_manager_revoke() {
        let mut mgr = SessionManager::new();
        let token_str = {
            let token = mgr.create_token("s1", SessionPermission::Edit, 0, 1000);
            token.token.clone()
        };

        assert!(mgr.revoke_token(&token_str));
        let result = mgr.validate(&token_str, &SessionAction::ViewPreview, 2000, None, None);
        assert_eq!(result.unwrap_err(), AccessOutcome::Revoked);
    }

    #[test]
    fn session_manager_expired() {
        let mut mgr = SessionManager::new();
        let token_str = {
            let token = mgr.create_token("s1", SessionPermission::View, 1000, 1000);
            token.token.clone()
        };

        // Valid before expiration
        let result = mgr.validate(&token_str, &SessionAction::ViewPreview, 1500, None, None);
        assert!(result.is_ok());

        // Expired
        let result = mgr.validate(&token_str, &SessionAction::ViewPreview, 3000, None, None);
        assert_eq!(result.unwrap_err(), AccessOutcome::Expired);
    }

    #[test]
    fn session_manager_not_found() {
        let mut mgr = SessionManager::new();
        let result = mgr.validate("nonexistent", &SessionAction::ViewPreview, 1000, None, None);
        assert_eq!(result.unwrap_err(), AccessOutcome::NotFound);
    }

    #[test]
    fn access_log_records() {
        let mut log = AccessLog::new(100);
        log.log("tok1", 1000, Some("1.2.3.4"), None, AccessOutcome::Granted);
        log.log("tok1", 2000, None, None, AccessOutcome::Expired);

        assert_eq!(log.len(), 2);
        assert_eq!(log.by_token("tok1").len(), 2);
        assert_eq!(log.since(1500).len(), 1);
    }

    #[test]
    fn session_manager_revoke_session() {
        let mut mgr = SessionManager::new();
        let t1 = mgr
            .create_token("s1", SessionPermission::View, 0, 1000)
            .token
            .clone();
        let t2 = mgr
            .create_token("s1", SessionPermission::Edit, 0, 2000)
            .token
            .clone();
        let t3 = mgr
            .create_token("s2", SessionPermission::View, 0, 3000)
            .token
            .clone();

        mgr.revoke_session_tokens("s1");

        let result = mgr.validate(&t1, &SessionAction::ViewPreview, 4000, None, None);
        assert_eq!(result.unwrap_err(), AccessOutcome::Revoked);

        let result = mgr.validate(&t2, &SessionAction::ViewPreview, 4000, None, None);
        assert_eq!(result.unwrap_err(), AccessOutcome::Revoked);

        let result = mgr.validate(&t3, &SessionAction::ViewPreview, 4000, None, None);
        assert!(result.is_ok()); // s2 tokens not affected
    }

    #[test]
    fn active_tokens_filter() {
        let mut mgr = SessionManager::new();
        mgr.create_token("s1", SessionPermission::View, 10_000, 1000);
        mgr.create_token("s1", SessionPermission::Edit, 5_000, 1000);

        assert_eq!(mgr.active_tokens(3000).len(), 2);
        assert_eq!(mgr.active_tokens(7000).len(), 1); // One expired
        assert_eq!(mgr.active_tokens(12000).len(), 0); // Both expired
    }
}
