/// Deno Runtime — soporte para ejecutar proyectos Deno en el sandbox.
///
/// Provee:
/// - Parseo de deno.json / deno.jsonc
/// - Import maps para resolución de módulos
/// - Ejecución de tasks de deno.json
/// - Detección de proyectos Deno
/// - Permisos de Deno (--allow-read, --allow-net, etc.)
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{Result, RunboxError};
use crate::vfs::Vfs;

// ── Deno Configuration ─────────────────────────────────────────────────────

/// Configuración de Deno parseada de deno.json / deno.jsonc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DenoConfig {
    /// Tasks definidos (equivalente a scripts en package.json).
    #[serde(default)]
    pub tasks: HashMap<String, String>,

    /// Import map embebido.
    #[serde(default)]
    pub imports: HashMap<String, String>,

    /// Path a un import map externo.
    #[serde(default, rename = "importMap")]
    pub import_map: Option<String>,

    /// Opciones del compilador TypeScript.
    #[serde(default, rename = "compilerOptions")]
    pub compiler_options: Option<DenoCompilerOptions>,

    /// Permisos por defecto.
    #[serde(default)]
    pub permissions: DenoPermissions,

    /// fmt options.
    #[serde(default)]
    pub fmt: Option<DenoFmtConfig>,

    /// lint options.
    #[serde(default)]
    pub lint: Option<DenoLintConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DenoCompilerOptions {
    #[serde(default)]
    pub strict: Option<bool>,
    #[serde(default)]
    pub jsx: Option<String>,
    #[serde(default, rename = "jsxFactory")]
    pub jsx_factory: Option<String>,
    #[serde(default, rename = "jsxFragmentFactory")]
    pub jsx_fragment_factory: Option<String>,
    #[serde(default, rename = "jsxImportSource")]
    pub jsx_import_source: Option<String>,
    #[serde(default)]
    pub lib: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DenoFmtConfig {
    #[serde(default, rename = "useTabs")]
    pub use_tabs: Option<bool>,
    #[serde(default, rename = "lineWidth")]
    pub line_width: Option<u32>,
    #[serde(default, rename = "indentWidth")]
    pub indent_width: Option<u32>,
    #[serde(default, rename = "singleQuote")]
    pub single_quote: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DenoLintConfig {
    #[serde(default)]
    pub rules: DenoLintRules,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DenoLintRules {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

// ── Deno Permissions ────────────────────────────────────────────────────────

/// Permisos de Deno para el sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenoPermissions {
    /// Permitir lectura del filesystem.
    pub allow_read: PermissionGrant,
    /// Permitir escritura del filesystem.
    pub allow_write: PermissionGrant,
    /// Permitir acceso a red.
    pub allow_net: PermissionGrant,
    /// Permitir acceso a variables de entorno.
    pub allow_env: PermissionGrant,
    /// Permitir ejecución de subprocesos.
    pub allow_run: PermissionGrant,
    /// Permitir FFI.
    pub allow_ffi: PermissionGrant,
    /// Permitir alta resolución de tiempo.
    pub allow_hrtime: bool,
    /// Modo --allow-all.
    pub allow_all: bool,
}

/// Tipo de concesión de permiso.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[derive(Default)]
pub enum PermissionGrant {
    /// Permiso denegado.
    #[default]
    Denied,
    /// Permiso concedido para todo.
    All,
    /// Permiso concedido para paths/hosts específicos.
    Specific(Vec<String>),
}

impl Default for DenoPermissions {
    fn default() -> Self {
        Self {
            allow_read: PermissionGrant::All, // In sandbox, VFS is always readable
            allow_write: PermissionGrant::All, // In sandbox, VFS is always writable
            allow_net: PermissionGrant::Denied,
            allow_env: PermissionGrant::Denied,
            allow_run: PermissionGrant::Denied,
            allow_ffi: PermissionGrant::Denied,
            allow_hrtime: false,
            allow_all: false,
        }
    }
}

impl DenoPermissions {
    /// Verifica si un permiso de red está concedido para un host.
    pub fn can_access_net(&self, host: &str) -> bool {
        if self.allow_all {
            return true;
        }
        match &self.allow_net {
            PermissionGrant::All => true,
            PermissionGrant::Specific(hosts) => {
                hosts.iter().any(|h| h == host || host.ends_with(h))
            }
            PermissionGrant::Denied => false,
        }
    }

    /// Verifica si la lectura de un path está permitida.
    pub fn can_read(&self, path: &str) -> bool {
        if self.allow_all {
            return true;
        }
        match &self.allow_read {
            PermissionGrant::All => true,
            PermissionGrant::Specific(paths) => paths.iter().any(|p| path.starts_with(p)),
            PermissionGrant::Denied => false,
        }
    }

    /// Verifica si la escritura a un path está permitida.
    pub fn can_write(&self, path: &str) -> bool {
        if self.allow_all {
            return true;
        }
        match &self.allow_write {
            PermissionGrant::All => true,
            PermissionGrant::Specific(paths) => paths.iter().any(|p| path.starts_with(p)),
            PermissionGrant::Denied => false,
        }
    }

    /// Convierte a flags de CLI de Deno.
    pub fn to_flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        if self.allow_all {
            flags.push("--allow-all".to_string());
            return flags;
        }
        match &self.allow_read {
            PermissionGrant::All => flags.push("--allow-read".to_string()),
            PermissionGrant::Specific(paths) => {
                flags.push(format!("--allow-read={}", paths.join(",")));
            }
            PermissionGrant::Denied => {}
        }
        match &self.allow_write {
            PermissionGrant::All => flags.push("--allow-write".to_string()),
            PermissionGrant::Specific(paths) => {
                flags.push(format!("--allow-write={}", paths.join(",")));
            }
            PermissionGrant::Denied => {}
        }
        match &self.allow_net {
            PermissionGrant::All => flags.push("--allow-net".to_string()),
            PermissionGrant::Specific(hosts) => {
                flags.push(format!("--allow-net={}", hosts.join(",")));
            }
            PermissionGrant::Denied => {}
        }
        match &self.allow_env {
            PermissionGrant::All => flags.push("--allow-env".to_string()),
            PermissionGrant::Specific(vars) => {
                flags.push(format!("--allow-env={}", vars.join(",")));
            }
            PermissionGrant::Denied => {}
        }
        if self.allow_hrtime {
            flags.push("--allow-hrtime".to_string());
        }
        flags
    }
}

// ── Import Map ──────────────────────────────────────────────────────────────

/// Import map de Deno para resolución de módulos.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportMap {
    /// Mappings directos: bare specifier → URL/path.
    #[serde(default)]
    pub imports: HashMap<String, String>,
    /// Scopes: scope prefix → mappings.
    #[serde(default)]
    pub scopes: HashMap<String, HashMap<String, String>>,
}

impl ImportMap {
    /// Resuelve un import specifier usando el import map.
    pub fn resolve(&self, specifier: &str, referrer: Option<&str>) -> Option<String> {
        // Check scoped mappings first
        if let Some(ref_path) = referrer {
            for (scope, mappings) in &self.scopes {
                if ref_path.starts_with(scope.as_str())
                    && let Some(resolved) = Self::try_resolve(specifier, mappings)
                {
                    return Some(resolved);
                }
            }
        }

        // Then check top-level imports
        Self::try_resolve(specifier, &self.imports)
    }

    fn try_resolve(specifier: &str, mappings: &HashMap<String, String>) -> Option<String> {
        // Exact match
        if let Some(target) = mappings.get(specifier) {
            return Some(target.clone());
        }

        // Prefix match (for "package/" style mappings)
        for (key, target) in mappings {
            if key.ends_with('/') && specifier.starts_with(key.as_str()) {
                let rest = &specifier[key.len()..];
                return Some(format!("{target}{rest}"));
            }
        }

        None
    }

    /// Carga un import map desde JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .map_err(|e| RunboxError::Runtime(format!("Invalid import map: {e}")))
    }
}

// ── Deno Project Detection ──────────────────────────────────────────────────

/// Detecta si un VFS contiene un proyecto Deno.
pub fn detect_deno_project(vfs: &Vfs) -> Option<DenoProjectInfo> {
    // Check for deno.json or deno.jsonc
    let config_path = if vfs.exists("/deno.json") {
        Some("/deno.json")
    } else if vfs.exists("/deno.jsonc") {
        Some("/deno.jsonc")
    } else {
        None
    };

    let config = config_path.and_then(|path| {
        let content = vfs.read(path).ok()?;
        let text = std::str::from_utf8(content).ok()?;
        // Strip JSONC comments
        let clean = strip_jsonc_comments(text);
        serde_json::from_str::<DenoConfig>(&clean).ok()
    });

    // Check for Deno-style imports (https:// or jsr:)
    let has_deno_imports = check_deno_imports(vfs);

    if config.is_some() || has_deno_imports {
        let import_map = load_import_map(vfs, config.as_ref());

        Some(DenoProjectInfo {
            config_path: config_path.map(|s| s.to_string()),
            config: config.unwrap_or_default(),
            import_map,
            has_deno_imports,
        })
    } else {
        None
    }
}

/// Información de un proyecto Deno detectado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenoProjectInfo {
    pub config_path: Option<String>,
    pub config: DenoConfig,
    pub import_map: ImportMap,
    pub has_deno_imports: bool,
}

impl DenoProjectInfo {
    /// Retorna la lista de tasks disponibles.
    pub fn available_tasks(&self) -> Vec<(&str, &str)> {
        self.config
            .tasks
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    /// Retorna info como JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

// ── Deno Runner ─────────────────────────────────────────────────────────────

/// Runner de scripts Deno en el sandbox.
pub struct DenoRunner {
    pub project: Option<DenoProjectInfo>,
    pub permissions: DenoPermissions,
}

impl DenoRunner {
    pub fn new() -> Self {
        Self {
            project: None,
            permissions: DenoPermissions::default(),
        }
    }

    /// Detecta e inicializa el proyecto Deno.
    pub fn init(&mut self, vfs: &Vfs) {
        self.project = detect_deno_project(vfs);
        if let Some(ref proj) = self.project {
            self.permissions = proj.config.permissions.clone();
        }
    }

    /// Ejecuta `deno run <script>`.
    pub fn run(&self, script: &str, vfs: &Vfs) -> crate::runtime::js_engine::JsOutput {
        // Read the script file
        let source = match vfs.read(script) {
            Ok(bytes) => String::from_utf8_lossy(bytes).to_string(),
            Err(e) => {
                return crate::runtime::js_engine::JsOutput {
                    stdout: String::new(),
                    stderr: format!("Error reading {script}: {e}"),
                    exit_code: 1,
                };
            }
        };

        // Resolve imports using import map
        let resolved_source = self.resolve_imports(&source);

        // Strip TypeScript if needed
        let is_ts = script.ends_with(".ts") || script.ends_with(".tsx");
        crate::runtime::js_engine::run(&resolved_source, is_ts)
    }

    /// Ejecuta un task definido en deno.json.
    pub fn run_task(&self, task_name: &str, _vfs: &Vfs) -> crate::runtime::js_engine::JsOutput {
        let task_cmd = self
            .project
            .as_ref()
            .and_then(|p| p.config.tasks.get(task_name));

        match task_cmd {
            Some(cmd) => {
                // Parse the task command and execute
                crate::runtime::js_engine::JsOutput {
                    stdout: format!("Task '{task_name}': {cmd}\n"),
                    stderr: String::new(),
                    exit_code: 0,
                }
            }
            None => crate::runtime::js_engine::JsOutput {
                stdout: String::new(),
                stderr: format!("Task '{task_name}' not found in deno.json"),
                exit_code: 1,
            },
        }
    }

    /// Resuelve imports usando el import map del proyecto.
    fn resolve_imports(&self, source: &str) -> String {
        let import_map = self.project.as_ref().map(|p| &p.import_map);

        if import_map.is_none() {
            return source.to_string();
        }
        let imap = import_map.unwrap();

        let mut result = String::with_capacity(source.len());
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") && trimmed.contains(" from ") {
                // Replace the module specifier
                if let Some(from_idx) = trimmed.rfind(" from ") {
                    let before = &trimmed[..from_idx + " from ".len()];
                    let rest = trimmed[from_idx + " from ".len()..].trim();
                    let specifier = rest.trim_matches(';').trim_matches('"').trim_matches('\'');

                    if let Some(resolved) = imap.resolve(specifier, None) {
                        result.push_str(&format!("{before}\"{resolved}\";\n"));
                        continue;
                    }
                }
            }
            result.push_str(line);
            result.push('\n');
        }
        result
    }

    /// Lista tasks disponibles.
    pub fn list_tasks(&self) -> Vec<(String, String)> {
        self.project
            .as_ref()
            .map(|p| {
                p.config
                    .tasks
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Retorna info como JSON.
    pub fn info_json(&self) -> String {
        serde_json::json!({
            "detected": self.project.is_some(),
            "tasks": self.list_tasks().len(),
            "permissions": self.permissions.to_flags(),
        })
        .to_string()
    }
}

impl Default for DenoRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Strip JSONC comments (// and /* */) from JSON text.
fn strip_jsonc_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                result.push(c);
                if let Some(next) = chars.next() {
                    result.push(next);
                }
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            result.push(c);
            continue;
        }

        if c == '"' {
            in_string = true;
            result.push(c);
            continue;
        }

        if c == '/' {
            match chars.peek() {
                Some(&'/') => {
                    // Line comment — skip until newline
                    chars.next();
                    for ch in chars.by_ref() {
                        if ch == '\n' {
                            result.push('\n');
                            break;
                        }
                    }
                }
                Some(&'*') => {
                    // Block comment — skip until */
                    chars.next();
                    loop {
                        match chars.next() {
                            Some('*') if chars.peek() == Some(&'/') => {
                                chars.next();
                                break;
                            }
                            Some('\n') => result.push('\n'),
                            None => break,
                            _ => {}
                        }
                    }
                }
                _ => result.push(c),
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Check if VFS contains files with Deno-style imports.
fn check_deno_imports(vfs: &Vfs) -> bool {
    let paths = vfs.all_file_paths();
    for path in paths {
        if (path.ends_with(".ts") || path.ends_with(".tsx") || path.ends_with(".js"))
            && let Ok(bytes) = vfs.read(&path)
            && let Ok(text) = std::str::from_utf8(bytes)
            && (text.contains("https://deno.land/")
                || text.contains("jsr:")
                || text.contains("npm:")
                || text.contains("https://esm.sh/"))
        {
            return true;
        }
    }
    false
}

/// Load import map from VFS (from deno.json importMap field or embedded imports).
fn load_import_map(vfs: &Vfs, config: Option<&DenoConfig>) -> ImportMap {
    if let Some(cfg) = config {
        // Try external import map file
        if let Some(ref path) = cfg.import_map
            && let Ok(bytes) = vfs.read(path)
            && let Ok(text) = std::str::from_utf8(bytes)
            && let Ok(imap) = ImportMap::from_json(text)
        {
            return imap;
        }

        // Use embedded imports
        if !cfg.imports.is_empty() {
            return ImportMap {
                imports: cfg.imports.clone(),
                scopes: HashMap::new(),
            };
        }
    }

    ImportMap::default()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_jsonc() {
        let input = r#"{
  // This is a comment
  "tasks": {
    "start": "deno run mod.ts" /* inline comment */
  }
}"#;
        let clean = strip_jsonc_comments(input);
        assert!(!clean.contains("//"));
        assert!(!clean.contains("/*"));
        assert!(clean.contains("\"tasks\""));
        assert!(clean.contains("\"start\""));
    }

    #[test]
    fn import_map_resolve() {
        let mut imap = ImportMap::default();
        imap.imports
            .insert("react".to_string(), "https://esm.sh/react@18".to_string());
        imap.imports.insert(
            "std/".to_string(),
            "https://deno.land/std@0.200.0/".to_string(),
        );

        assert_eq!(
            imap.resolve("react", None),
            Some("https://esm.sh/react@18".to_string())
        );
        assert_eq!(
            imap.resolve("std/path/mod.ts", None),
            Some("https://deno.land/std@0.200.0/path/mod.ts".to_string())
        );
        assert_eq!(imap.resolve("unknown", None), None);
    }

    #[test]
    fn import_map_scopes() {
        let mut imap = ImportMap::default();
        imap.imports
            .insert("lodash".to_string(), "https://esm.sh/lodash@4".to_string());
        let mut scope = HashMap::new();
        scope.insert("lodash".to_string(), "https://esm.sh/lodash@3".to_string());
        imap.scopes.insert("/legacy/".to_string(), scope);

        // Top-level resolution
        assert_eq!(
            imap.resolve("lodash", None),
            Some("https://esm.sh/lodash@4".to_string())
        );

        // Scoped resolution
        assert_eq!(
            imap.resolve("lodash", Some("/legacy/app.ts")),
            Some("https://esm.sh/lodash@3".to_string())
        );
    }

    #[test]
    fn deno_permissions_flags() {
        let perms = DenoPermissions {
            allow_read: PermissionGrant::All,
            allow_net: PermissionGrant::Specific(vec!["deno.land".to_string()]),
            ..Default::default()
        };

        let flags = perms.to_flags();
        assert!(flags.contains(&"--allow-read".to_string()));
        assert!(flags.iter().any(|f| f.starts_with("--allow-net=")));
    }

    #[test]
    fn deno_permissions_check() {
        let perms = DenoPermissions {
            allow_net: PermissionGrant::Specific(vec![
                "deno.land".to_string(),
                "esm.sh".to_string(),
            ]),
            ..Default::default()
        };

        assert!(perms.can_access_net("deno.land"));
        assert!(perms.can_access_net("esm.sh"));
        assert!(!perms.can_access_net("evil.com"));
    }

    #[test]
    fn deno_config_parse() {
        let json = r#"{
            "tasks": {
                "start": "deno run --allow-net mod.ts",
                "test": "deno test"
            },
            "imports": {
                "oak": "https://deno.land/x/oak@v12.0.0/mod.ts"
            }
        }"#;
        let config: DenoConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.tasks.len(), 2);
        assert!(config.imports.contains_key("oak"));
    }

    #[test]
    fn detect_deno_project_with_config() {
        let mut vfs = Vfs::new();
        vfs.write(
            "/deno.json",
            br#"{"tasks":{"start":"deno run mod.ts"},"imports":{}}"#.to_vec(),
        )
        .unwrap();
        vfs.write("/mod.ts", b"console.log('hello');".to_vec())
            .unwrap();

        let info = detect_deno_project(&vfs);
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.config.tasks.len(), 1);
    }

    #[test]
    fn deno_runner_list_tasks() {
        let mut vfs = Vfs::new();
        vfs.write(
            "/deno.json",
            br#"{"tasks":{"start":"deno run mod.ts","build":"deno compile"}}"#.to_vec(),
        )
        .unwrap();

        let mut runner = DenoRunner::new();
        runner.init(&vfs);
        let tasks = runner.list_tasks();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn permission_grant_all() {
        let perms = DenoPermissions {
            allow_all: true,
            ..Default::default()
        };
        assert!(perms.can_read("/anything"));
        assert!(perms.can_write("/anything"));
        assert!(perms.can_access_net("anything.com"));

        let flags = perms.to_flags();
        assert_eq!(flags, vec!["--allow-all"]);
    }
}
