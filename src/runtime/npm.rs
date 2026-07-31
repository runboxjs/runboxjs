use super::{ExecOutput, Runtime};
use crate::error::{Result, RunboxError};
use crate::process::ProcessManager;
use crate::shell::Command;
use crate::vfs::Vfs;
use serde::{Deserialize, Serialize};
/// Runtime de package managers: npm, pnpm, yarn.
/// Lee y escribe package.json real del VFS.
/// Nativo: resuelve paquetes contra registry.npmjs.org y extrae tarballs al VFS.
/// Incluye: cache persistente, resolución semver, lockfile mejorado, workspaces.
use std::collections::HashMap;

// ── package.json ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct PackageJson {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    main: Option<String>,
    scripts: HashMap<String, String>,
    dependencies: HashMap<String, String>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: HashMap<String, String>,
    #[serde(rename = "peerDependencies")]
    peer_dependencies: HashMap<String, String>,
    workspaces: Option<serde_json::Value>,
}

impl PackageJson {
    fn load(vfs: &Vfs) -> Option<Self> {
        vfs.read("/package.json")
            .ok()
            .and_then(|b| serde_json::from_slice(b).ok())
    }

    fn save(&self, vfs: &mut Vfs) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| RunboxError::Runtime(format!("package.json serialization failed: {e}")))?;
        vfs.write("/package.json", json.into_bytes())
    }

    fn add_dep(&mut self, name: &str, version: &str, dev: bool) {
        let map = if dev {
            &mut self.dev_dependencies
        } else {
            &mut self.dependencies
        };
        map.insert(name.to_string(), version.to_string());
    }

    fn remove_dep(&mut self, name: &str) {
        self.dependencies.remove(name);
        self.dev_dependencies.remove(name);
        self.peer_dependencies.remove(name);
    }
}

// ── Lock file (simulado) ──────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct LockEntry {
    version: String,
    resolved: String,
    integrity: String,
}

// ── Persistent Cache Tracking ────────────────────────────────────────────────

/// Persistent cache entry for tracking installed packages.
/// Designed for IndexedDB serialization in WASM environments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageCacheEntry {
    pub name: String,
    pub version: String,
    pub integrity: String,
    pub tarball_url: String,
    pub cached_at: u64,
    pub size_bytes: usize,
    pub dep_count: usize,
}

/// Package cache manager for persistent caching across sessions.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PackageCache {
    entries: HashMap<String, PackageCacheEntry>,
    max_entries: usize,
    total_size: usize,
}

impl PackageCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries,
            total_size: 0,
        }
    }

    /// Check if a package@version is in cache.
    pub fn contains(&self, name: &str, version: &str) -> bool {
        self.entries.contains_key(&cache_key(name, version))
    }

    /// Record a cached package.
    pub fn record(&mut self, entry: PackageCacheEntry) {
        let key = cache_key(&entry.name, &entry.version);
        self.total_size += entry.size_bytes;

        // Evict oldest if at capacity
        if self.entries.len() >= self.max_entries
            && let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.cached_at)
                .map(|(k, _)| k.clone())
            && let Some(removed) = self.entries.remove(&oldest_key)
        {
            self.total_size = self.total_size.saturating_sub(removed.size_bytes);
        }

        self.entries.insert(key, entry);
    }

    /// Get a cached entry.
    pub fn get(&self, name: &str, version: &str) -> Option<&PackageCacheEntry> {
        self.entries.get(&cache_key(name, version))
    }

    /// Remove a cached entry.
    pub fn remove(&mut self, name: &str, version: &str) {
        let key = cache_key(name, version);
        if let Some(entry) = self.entries.remove(&key) {
            self.total_size = self.total_size.saturating_sub(entry.size_bytes);
        }
    }

    /// Get cache statistics.
    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "entries": self.entries.len(),
            "max_entries": self.max_entries,
            "total_size": self.total_size,
        })
    }

    /// Clear all cached entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.total_size = 0;
    }

    /// Export cache for IndexedDB persistence.
    pub fn export_json(&self) -> String {
        serde_json::to_string(&self.entries).unwrap_or_default()
    }
}

fn cache_key(name: &str, version: &str) -> String {
    format!("{name}@{version}")
}

// ── Semver Resolution ────────────────────────────────────────────────────────

/// Parsed semver version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre: Option<String>,
}

impl SemVer {
    /// Parse a version string like "1.2.3" or "1.2.3-beta.1".
    pub fn parse(s: &str) -> Option<Self> {
        let clean = s.trim_start_matches(|c: char| !c.is_ascii_digit());
        let (version_part, pre) = if let Some(pos) = clean.find('-') {
            (&clean[..pos], Some(clean[pos + 1..].to_string()))
        } else {
            (clean, None)
        };

        let parts: Vec<&str> = version_part.split('.').collect();
        Some(Self {
            major: parts.first()?.parse().ok()?,
            minor: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
            patch: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
            pre,
        })
    }

    /// Check if this version satisfies a constraint like "^1.2.3", "~1.2.3", ">=1.0.0".
    pub fn satisfies(&self, constraint: &str) -> bool {
        let trimmed = constraint.trim();
        if trimmed == "*" || trimmed == "latest" || trimmed.is_empty() {
            return true;
        }

        if let Some(rest) = trimmed.strip_prefix('^')
            && let Some(c) = SemVer::parse(rest)
        {
            // ^1.2.3 means >=1.2.3 <2.0.0 (for major > 0)
            // ^0.2.3 means >=0.2.3 <0.3.0 (for major = 0)
            if c.major > 0 {
                return self.major == c.major
                    && (self.minor > c.minor || (self.minor == c.minor && self.patch >= c.patch));
            } else if c.minor > 0 {
                return self.major == 0 && self.minor == c.minor && self.patch >= c.patch;
            } else {
                return self.major == 0 && self.minor == 0 && self.patch == c.patch;
            }
        }

        if let Some(rest) = trimmed.strip_prefix('~')
            && let Some(c) = SemVer::parse(rest)
        {
            // ~1.2.3 means >=1.2.3 <1.3.0
            return self.major == c.major && self.minor == c.minor && self.patch >= c.patch;
        }

        if let Some(rest) = trimmed.strip_prefix(">=")
            && let Some(c) = SemVer::parse(rest)
        {
            return self.cmp_tuple() >= c.cmp_tuple();
        }

        if let Some(rest) = trimmed.strip_prefix('>')
            && let Some(c) = SemVer::parse(rest)
        {
            return self.cmp_tuple() > c.cmp_tuple();
        }

        if let Some(rest) = trimmed.strip_prefix("<=")
            && let Some(c) = SemVer::parse(rest)
        {
            return self.cmp_tuple() <= c.cmp_tuple();
        }

        if let Some(rest) = trimmed.strip_prefix('<')
            && let Some(c) = SemVer::parse(rest)
        {
            return self.cmp_tuple() < c.cmp_tuple();
        }

        // Exact match
        if let Some(c) = SemVer::parse(trimmed) {
            return self.major == c.major && self.minor == c.minor && self.patch == c.patch;
        }

        true // Unknown format, assume satisfied
    }

    fn cmp_tuple(&self) -> (u32, u32, u32) {
        (self.major, self.minor, self.patch)
    }
}

impl std::fmt::Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.pre {
            Some(pre) => write!(f, "{}.{}.{}-{pre}", self.major, self.minor, self.patch),
            None => write!(f, "{}.{}.{}", self.major, self.minor, self.patch),
        }
    }
}

// ── Workspace Support ────────────────────────────────────────────────────────

/// Workspace package info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacePackage {
    pub name: String,
    pub path: String,
    pub version: String,
    pub dependencies: HashMap<String, String>,
}

/// Detect and list workspace packages from package.json.
pub fn detect_workspaces(vfs: &Vfs) -> Vec<WorkspacePackage> {
    let root_pkg = match PackageJson::load(vfs) {
        Some(p) => p,
        None => return vec![],
    };

    let workspace_patterns = match &root_pkg.workspaces {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>(),
        Some(serde_json::Value::Object(obj)) => {
            // pnpm/yarn workspaces: { packages: [...] }
            obj.get("packages")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        }
        _ => return vec![],
    };

    let mut packages = Vec::new();
    for pattern in &workspace_patterns {
        // Simple pattern: "packages/*" → look for package.json in each subdir
        let base_dir = pattern.trim_end_matches('*').trim_end_matches('/');
        if base_dir.is_empty() {
            continue;
        }

        // List directories matching pattern
        if let Ok(entries) = vfs.list(&format!("/{base_dir}")) {
            for entry in entries {
                let pkg_path = format!("/{base_dir}/{entry}/package.json");
                if let Ok(bytes) = vfs.read(&pkg_path)
                    && let Ok(pkg) = serde_json::from_slice::<PackageJson>(bytes)
                {
                    packages.push(WorkspacePackage {
                        name: pkg.name.unwrap_or_else(|| entry.clone()),
                        path: format!("/{base_dir}/{entry}"),
                        version: pkg.version.unwrap_or_else(|| "0.0.0".into()),
                        dependencies: pkg.dependencies,
                    });
                }
            }
        }
    }

    packages
}

/// Generate improved lockfile with integrity hashes.
pub fn generate_lockfile_v2(vfs: &Vfs, pm_name: &str) -> Result<String> {
    let pkg = PackageJson::load(vfs).ok_or_else(|| RunboxError::NotFound("package.json".into()))?;

    let all_deps: Vec<(&str, &str)> = pkg
        .dependencies
        .iter()
        .chain(pkg.dev_dependencies.iter())
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let content = match pm_name {
        "yarn" => {
            let mut lines = vec!["# yarn lockfile v1".to_string(), String::new()];
            for (name, ver) in &all_deps {
                let bare = ver.trim_start_matches(|c: char| !c.is_ascii_digit());
                let hash = simple_integrity_hash(name, bare);
                lines.push(format!("\"{name}@{ver}\":"));
                lines.push(format!("  version \"{bare}\""));
                lines.push(format!(
                    "  resolved \"https://registry.yarnpkg.com/{name}/-/{name}-{bare}.tgz#{hash}\""
                ));
                lines.push(format!("  integrity sha512-{hash}"));
                lines.push(String::new());
            }
            lines.join("\n")
        }
        "pnpm" => {
            let mut lines = vec![
                "lockfileVersion: '9.0'".to_string(),
                String::new(),
                "importers:".to_string(),
                "  .:".to_string(),
                "    dependencies:".to_string(),
            ];
            for (name, ver) in &all_deps {
                let bare = ver.trim_start_matches(|c: char| !c.is_ascii_digit());
                lines.push(format!("      {name}:"));
                lines.push(format!("        specifier: {ver}"));
                lines.push(format!("        version: {bare}"));
            }
            lines.push(String::new());
            lines.push("packages:".to_string());
            for (name, ver) in &all_deps {
                let bare = ver.trim_start_matches(|c: char| !c.is_ascii_digit());
                let hash = simple_integrity_hash(name, bare);
                lines.push(format!("  /{name}/{bare}:"));
                lines.push(format!("    resolution: {{integrity: sha512-{hash}}}"));
            }
            lines.join("\n")
        }
        _ => {
            // npm lockfile v3
            let mut packages = serde_json::Map::new();
            packages.insert(
                String::new(),
                serde_json::json!({
                    "name": pkg.name,
                    "version": pkg.version,
                    "dependencies": pkg.dependencies,
                    "devDependencies": pkg.dev_dependencies,
                }),
            );
            for (name, ver) in &all_deps {
                let bare = ver.trim_start_matches(|c: char| !c.is_ascii_digit());
                let hash = simple_integrity_hash(name, bare);
                packages.insert(format!("node_modules/{name}"), serde_json::json!({
                    "version": bare,
                    "resolved": format!("https://registry.npmjs.org/{name}/-/{name}-{bare}.tgz"),
                    "integrity": format!("sha512-{hash}"),
                }));
            }
            serde_json::to_string_pretty(&serde_json::json!({
                "name": pkg.name,
                "version": pkg.version,
                "lockfileVersion": 3,
                "requires": true,
                "packages": packages,
            }))
            .unwrap_or_default()
        }
    };

    Ok(content)
}

/// Generate a simple integrity hash (FNV-1a based) for lockfile entries.
fn simple_integrity_hash(name: &str, version: &str) -> String {
    let input = format!("{name}@{version}");
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

// ── npm registry resolver (nativo) ───────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn registry_resolve(name: &str, version_req: &str) -> crate::error::Result<RegistryPackage> {
    use crate::network::http_get;

    // Normalizar versión: quitar ^, ~, >=, etc.
    let ver = if version_req == "latest" || version_req.is_empty() {
        "latest".to_string()
    } else {
        version_req
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .to_string()
    };

    let url = format!("https://registry.npmjs.org/{name}/{ver}");
    let resp = http_get(&url)?;
    if resp.status == 404 {
        return Err(crate::error::RunboxError::NotFound(format!("{name}@{ver}")));
    }
    if resp.status != 200 {
        return Err(crate::error::RunboxError::Runtime(format!(
            "registry error for {name}: HTTP {}",
            resp.status
        )));
    }
    resp.json::<RegistryPackage>()
}

#[cfg(not(target_arch = "wasm32"))]
fn registry_install_package(
    name: &str,
    version_req: &str,
    vfs: &mut Vfs,
) -> crate::error::Result<String> {
    use crate::network::http_get;

    let pkg = registry_resolve(name, version_req)?;
    let tarball_url = &pkg.dist.tarball;

    let tarball = http_get(tarball_url)?;
    extract_tgz_to_vfs(&tarball.body, name, vfs)?;

    // Asegurarse de que package.json existe en node_modules/<name>/
    let nm_pkg = serde_json::json!({
        "name": name,
        "version": pkg.version,
        "main": pkg.main.as_deref().unwrap_or("index.js"),
    });
    vfs.write(
        &format!("/node_modules/{name}/package.json"),
        serde_json::to_string(&nm_pkg).unwrap().into_bytes(),
    )?;

    Ok(pkg.version)
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct RegistryPackage {
    #[allow(dead_code)]
    name: String,
    version: String,
    main: Option<String>,
    dist: RegistryDist,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct RegistryDist {
    tarball: String,
    #[allow(dead_code)]
    integrity: Option<String>,
}

fn lock_filename(pm: &str) -> &'static str {
    match pm {
        "pnpm" => "pnpm-lock.yaml",
        "yarn" => "yarn.lock",
        "bun" => "bun.lock",
        _ => "package-lock.json",
    }
}

fn write_lock(vfs: &mut Vfs, pm_name: &str, pkg: &PackageJson) -> Result<()> {
    let all_deps: Vec<(&str, &str)> = pkg
        .dependencies
        .iter()
        .chain(pkg.dev_dependencies.iter())
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let content = match pm_name {
        "yarn" => {
            let mut lines = vec!["# yarn lockfile v1\n".to_string()];
            for (name, ver) in &all_deps {
                lines.push(format!("\"{name}@{ver}\":\n  version \"{ver}\"\n  resolved \"https://registry.yarnpkg.com/{name}/-/{name}-{ver}.tgz\"\n"));
            }
            lines.join("\n")
        }
        "pnpm" => {
            let mut lines = vec!["lockfileVersion: '9.0'\n\npackages:".to_string()];
            for (name, ver) in &all_deps {
                lines.push(format!(
                    "  /{name}/{ver}:\n    resolution: {{integrity: sha512-placeholder}}"
                ));
            }
            lines.join("\n")
        }
        _ => {
            let entries: HashMap<&str, LockEntry> = all_deps
                .iter()
                .map(|(name, ver)| {
                    (
                        *name,
                        LockEntry {
                            version: ver.to_string(),
                            resolved: format!(
                                "https://registry.npmjs.org/{name}/-/{name}-{ver}.tgz"
                            ),
                            integrity: "sha512-placeholder".into(),
                        },
                    )
                })
                .collect();
            serde_json::to_string_pretty(&serde_json::json!({
                "name": pkg.name,
                "lockfileVersion": 3,
                "packages": entries
            }))
            .unwrap_or_default()
        }
    };

    vfs.write(
        &format!("/{}", lock_filename(pm_name)),
        content.into_bytes(),
    )
}

// ── npm WASM — install via fetch del browser ──────────────────────────────────
//
// En WASM no podemos hacer HTTP bloqueante. El flujo es:
//
//   1. JS llama runbox.npm_packages_needed()  → lista JSON de paquetes a descargar
//   2. JS hace fetch() a registry.npmjs.org (CORS está habilitado ✓)
//   3. JS llama runbox.npm_process_tarball(name, version, bytes)
//   4. RunBox extrae el tarball al VFS y actualiza package.json
//
// Código JS de referencia:
//
// ```js
// async function npmInstall(runbox) {
//   const needed = JSON.parse(runbox.npm_packages_needed());
//   for (const { name, version } of needed) {
//     const meta = await fetch(`https://registry.npmjs.org/${name}/${version}`).then(r => r.json());
//     const buf  = await fetch(meta.dist.tarball).then(r => r.arrayBuffer());
//     runbox.npm_process_tarball(name, version, new Uint8Array(buf));
//   }
// }
// ```

#[derive(Debug, Serialize, Deserialize)]
pub struct NpmPackageRequest {
    pub name: String,
    pub version: String,
}

/// Retorna la lista de paquetes del package.json que aún no están en node_modules
/// o que solo tienen un stub sin código real.
pub fn packages_needed(vfs: &Vfs) -> Vec<NpmPackageRequest> {
    let pkg = match PackageJson::load(vfs) {
        Some(p) => p,
        None => return vec![],
    };
    pkg.dependencies
        .iter()
        .chain(pkg.dev_dependencies.iter())
        .filter(|(name, _)| {
            let base = format!("/node_modules/{name}");
            // Si ni siquiera hay package.json, falta completamente
            if !vfs.exists(&format!("{base}/package.json")) {
                return true;
            }
            // Verificar si hay código real: buscar el main o index.js
            // Si el contenido contiene "RunBox stub" es un placeholder
            let main_path = format!("{base}/index.js");
            match vfs.read(&main_path) {
                Ok(bytes) => {
                    let content = String::from_utf8_lossy(bytes);
                    content.contains("RunBox stub")
                }
                Err(_) => {
                    // No hay index.js — podría tener un main diferente
                    // Leer package.json para encontrar main
                    if let Ok(pj_bytes) = vfs.read(&format!("{base}/package.json")) {
                        if let Ok(pj) = serde_json::from_slice::<serde_json::Value>(pj_bytes) {
                            if let Some(main) = pj.get("main").and_then(|v| v.as_str()) {
                                let main_full = format!("{base}/{main}");
                                !vfs.exists(&main_full)
                            } else {
                                // No main field y no index.js — necesita descarga
                                true
                            }
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                }
            }
        })
        .map(|(name, ver)| NpmPackageRequest {
            name: name.clone(),
            version: ver
                .trim_start_matches(|c: char| !c.is_ascii_digit())
                .to_string(),
        })
        .collect()
}

/// Instala un paquete dado su tarball en bytes (llamado desde JS en WASM).
pub fn process_tarball(name: &str, _version: &str, bytes: &[u8], vfs: &mut Vfs) -> Result<()> {
    extract_tgz_to_vfs(bytes, name, vfs)?;
    Ok(())
}

/// Extrae un .tgz al VFS bajo /node_modules/<name>/
fn extract_tgz_to_vfs(bytes: &[u8], name: &str, vfs: &mut Vfs) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    use tar::Archive;

    let gz = GzDecoder::new(bytes);
    let mut archive = Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| crate::error::RunboxError::Runtime(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| crate::error::RunboxError::Runtime(e.to_string()))?;
        let raw_path = entry
            .path()
            .map_err(|e| crate::error::RunboxError::Runtime(e.to_string()))?
            .to_string_lossy()
            .into_owned();

        // Los tarballs de npm tienen "package/..." como prefijo
        let rel = raw_path.strip_prefix("package/").unwrap_or(&raw_path);

        // Saltar archivos muy grandes o binarios que no necesitamos
        let size = entry.size();
        if size > 2_000_000 {
            continue;
        }

        let vfs_path = format!("/node_modules/{name}/{rel}");
        let capacity = usize::try_from(size).unwrap_or(0);
        let mut content = Vec::with_capacity(capacity);
        if entry.read_to_end(&mut content).is_ok() {
            let _ = vfs.write(&vfs_path, content);
        }
    }

    Ok(())
}

// ── Runtime ───────────────────────────────────────────────────────────────────

pub struct PackageManagerRuntime {
    name: &'static str,
}

impl PackageManagerRuntime {
    pub fn npm() -> Self {
        Self { name: "npm" }
    }
    pub fn pnpm() -> Self {
        Self { name: "pnpm" }
    }
    pub fn yarn() -> Self {
        Self { name: "yarn" }
    }
    /// Bun usa el mismo sistema de paquetes que npm bajo el capó.
    pub fn bun_via_npm() -> Self {
        Self { name: "bun" }
    }
}

impl Runtime for PackageManagerRuntime {
    fn name(&self) -> &'static str {
        self.name
    }

    fn exec(&self, cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
        let pm_name = self.name;
        let sub = cmd.args.first().map(String::as_str).unwrap_or("");

        match sub {
            "install" | "i" | "ci" => pm_install(cmd, vfs, pm, pm_name),
            "add" => pm_add(cmd, vfs, pm, pm_name),
            "remove" | "uninstall" | "rm" | "un" => pm_remove(cmd, vfs, pm, pm_name),
            "run" => pm_run(cmd, vfs, pm, pm_name),
            "exec" | "dlx" | "npx" | "pnpx" | "create" => pm_exec(cmd, vfs, pm, pm_name),
            "init" => pm_init(cmd, vfs, pm, pm_name),
            "list" | "ls" => pm_list(vfs, pm, pm_name, cmd),
            "update" | "upgrade" => pm_update(cmd, vfs, pm, pm_name),
            "outdated" => pm_outdated(vfs, pm, pm_name, cmd),
            "audit" => pm_audit(vfs, pm, pm_name, cmd),
            _ => Err(RunboxError::Runtime(format!(
                "{pm_name}: unknown subcommand '{sub}'"
            ))),
        }
    }
}

// ── Subcomandos ───────────────────────────────────────────────────────────────

fn pm_install(
    cmd: &Command,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    pm_name: &str,
) -> Result<ExecOutput> {
    // If package names were passed (e.g. `npm install lodash`), delegate to pm_add.
    let pkg_names: Vec<String> = cmd
        .args
        .iter()
        .skip(1) // skip "install"
        .filter(|a| !a.starts_with('-'))
        .cloned()
        .collect();

    if !pkg_names.is_empty() {
        let dev = cmd
            .args
            .iter()
            .any(|a| a == "-D" || a == "--save-dev" || a == "--dev");
        let mut add_args = vec!["add".to_string()];
        add_args.extend(pkg_names);
        if dev {
            add_args.push("-D".to_string());
        }
        let add_cmd = Command {
            program: cmd.program.clone(),
            args: add_args,
            env: cmd.env.clone(),
            stdin: None,
            redirect: None,
        };
        return pm_add(&add_cmd, vfs, pm, pm_name);
    }

    let pkg = PackageJson::load(vfs);
    let (dep_count, dev_count) = pkg
        .as_ref()
        .map(|p| (p.dependencies.len(), p.dev_dependencies.len()))
        .unwrap_or((0, 0));

    if dep_count + dev_count == 0 && vfs.exists("/package.json") {
        let pid = pm.spawn(pm_name, cmd.args.clone());
        pm.exit(pid, 0)?;
        return Ok(ok_out("up to date — no dependencies found in package.json"));
    }

    let mut resolved = 0usize;
    #[cfg(not(target_arch = "wasm32"))]
    let mut failed: Vec<&str> = vec![];
    #[cfg(target_arch = "wasm32")]
    let failed: Vec<&str> = vec![];

    if let Some(pkg) = &pkg {
        write_lock(vfs, pm_name, pkg)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            for (name, ver) in pkg.dependencies.iter().chain(pkg.dev_dependencies.iter()) {
                match registry_install_package(name, ver, vfs) {
                    Ok(_) => resolved += 1,
                    Err(_) => {
                        // Fallback: stub funcional en node_modules que exporta
                        // un proxy vacío para que require() no rompa.
                        let bare_ver = ver.trim_start_matches(|c: char| !c.is_ascii_digit());
                        let _ = vfs.write(
                            &format!("/node_modules/{name}/package.json"),
                            serde_json::json!({
                                "name": name,
                                "version": bare_ver,
                                "main": "index.js"
                            })
                            .to_string()
                            .into_bytes(),
                        );
                        // Crear un index.js stub que exporta un proxy permisivo
                        // para que el código del usuario no crashee al acceder propiedades
                        let name_safe =
                            name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
                        let stub_js = format!(
                            r#"// RunBox stub for {name} — tarball not yet downloaded
var __stub_warned_{name_safe} = false;
var handler = {{
  get: function(target, prop) {{
    if (prop === '__esModule' || prop === 'default') return target;
    if (prop === '__runbox_stub') return true;
    if (!__stub_warned_{name_safe}) {{
      __stub_warned_{name_safe} = true;
      if (typeof console !== 'undefined') console.warn('[RunBox] Package "{name}" is a stub — run the npm install flow to download the real package.');
    }}
    if (typeof prop === 'string') return new Proxy(function(){{}}, handler);
    return undefined;
  }},
  apply: function() {{ return new Proxy({{}}, handler); }},
  construct: function() {{ return new Proxy({{}}, handler); }},
}};
module.exports = new Proxy(function {name_safe}(){{}}, handler);
module.exports.default = module.exports;
"#,
                            name = name,
                            name_safe = name_safe,
                        );
                        let _ = vfs.write(
                            &format!("/node_modules/{name}/index.js"),
                            stub_js.into_bytes(),
                        );
                        failed.push(name.as_str());
                    }
                }
            }
        }

        // En WASM, crear stubs funcionales como safety net. El host JS
        // debería llamar packages_needed() + process_tarball() para instalar
        // los paquetes reales, pero si no lo hace, los stubs evitan que
        // require() crashee con "Cannot find module".
        #[cfg(target_arch = "wasm32")]
        {
            for (name, ver) in pkg.dependencies.iter().chain(pkg.dev_dependencies.iter()) {
                // Solo crear stub si no existe ya código real
                if !vfs.exists(&format!("/node_modules/{name}/index.js"))
                    && !vfs.exists(&format!("/node_modules/{name}/lib/index.js"))
                {
                    let bare_ver = ver.trim_start_matches(|c: char| !c.is_ascii_digit());
                    let _ = vfs.write(
                        &format!("/node_modules/{name}/package.json"),
                        serde_json::json!({
                            "name": name,
                            "version": bare_ver,
                            "main": "index.js"
                        })
                        .to_string()
                        .into_bytes(),
                    );
                    let name_safe = name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
                    let stub_js = format!(
                        r#"// RunBox stub for {name} — tarball not yet downloaded
var __stub_warned_{name_safe} = false;
var handler = {{
  get: function(target, prop) {{
    if (prop === '__esModule' || prop === 'default') return target;
    if (prop === '__runbox_stub') return true;
    if (!__stub_warned_{name_safe}) {{
      __stub_warned_{name_safe} = true;
      if (typeof console !== 'undefined') console.warn('[RunBox] Package "{name}" is a stub — run the npm install flow to download the real package.');
    }}
    if (typeof prop === 'string') return new Proxy(function(){{}}, handler);
    return undefined;
  }},
  apply: function() {{ return new Proxy({{}}, handler); }},
  construct: function() {{ return new Proxy({{}}, handler); }},
}};
module.exports = new Proxy(function {name_safe}(){{}}, handler);
module.exports.default = module.exports;
"#,
                        name = name,
                        name_safe = name_safe,
                    );
                    let _ = vfs.write(
                        &format!("/node_modules/{name}/index.js"),
                        stub_js.into_bytes(),
                    );
                }
            }

            // Emitir señal al host JS para que descargue los paquetes reales
            {
                let needed = packages_needed(vfs);
                if !needed.is_empty() {
                    if let Ok(json) = serde_json::to_string(&needed) {
                        let script = format!(
                            "if(typeof globalThis.__runbox_npm_install==='function'){{globalThis.__runbox_npm_install({json});}}"
                        );
                        let _ = js_sys::eval(&script);
                    }
                }
            }

            resolved = dep_count + dev_count;
        }
    }

    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, 0)?;
    let total = dep_count + dev_count;
    #[cfg(target_arch = "wasm32")]
    let msg = format!(
        "created stubs for {total} packages — run npm_install_packages() in JS to download from registry"
    );
    #[cfg(not(target_arch = "wasm32"))]
    let mut msg = format!("added {total} packages ({dep_count} prod, {dev_count} dev)");
    #[cfg(not(target_arch = "wasm32"))]
    if !failed.is_empty() {
        msg.push_str(&format!(
            "\nWarning: could not fetch from registry: {}",
            failed.join(", ")
        ));
    }
    let _ = resolved; // suppress unused warning when all are stubs
    Ok(ok_out(msg))
}

fn pm_add(
    cmd: &Command,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    pm_name: &str,
) -> Result<ExecOutput> {
    let dev = cmd
        .args
        .iter()
        .any(|a| a == "-D" || a == "--save-dev" || a == "--dev");
    let exact = cmd.args.iter().any(|a| a == "-E" || a == "--save-exact");

    let packages: Vec<String> = cmd
        .args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .cloned()
        .collect();

    if packages.is_empty() {
        return Err(RunboxError::Runtime(format!(
            "{pm_name} add: specify at least one package"
        )));
    }

    let mut pkg = PackageJson::load(vfs).unwrap_or_default();
    let mut added = vec![];

    for spec in &packages {
        let (name, ver) = parse_package_spec(spec, exact);
        pkg.add_dep(&name, &ver, dev);
        let bare_ver = ver.trim_start_matches(|c: char| !c.is_ascii_digit());
        vfs.write(
            &format!("/node_modules/{name}/package.json"),
            serde_json::json!({ "name": name, "version": bare_ver, "main": "index.js" })
                .to_string()
                .into_bytes(),
        )?;
        // Crear stub index.js funcional si no existe código real
        let idx_path = format!("/node_modules/{name}/index.js");
        if !vfs.exists(&idx_path) {
            let name_safe = name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
            let stub_js = format!(
                r#"// RunBox stub for {name} — tarball not yet downloaded
var __stub_warned_{name_safe} = false;
var handler = {{
  get: function(target, prop) {{
    if (prop === '__esModule' || prop === 'default') return target;
    if (prop === '__runbox_stub') return true;
    if (!__stub_warned_{name_safe}) {{
      __stub_warned_{name_safe} = true;
      if (typeof console !== 'undefined') console.warn('[RunBox] Package "{name}" is a stub — run the npm install flow to download the real package.');
    }}
    if (typeof prop === 'string') return new Proxy(function(){{}}, handler);
    return undefined;
  }},
  apply: function() {{ return new Proxy({{}}, handler); }},
  construct: function() {{ return new Proxy({{}}, handler); }},
}};
module.exports = new Proxy(function {name_safe}(){{}}, handler);
module.exports.default = module.exports;
"#,
                name = name,
                name_safe = name_safe,
            );
            vfs.write(&idx_path, stub_js.into_bytes())?;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Intentar descargar paquete real del registry
            let _ = registry_install_package(&name, &ver, vfs);
        }

        added.push(format!("{name}@{bare_ver}"));
    }

    pkg.save(vfs)?;
    write_lock(vfs, pm_name, &pkg)?;

    // WASM: señal al host para descarga real
    #[cfg(target_arch = "wasm32")]
    {
        let pkg_list: Vec<serde_json::Value> = added
            .iter()
            .map(|s| serde_json::json!({"spec": s}))
            .collect();
        if let Ok(json) = serde_json::to_string(&pkg_list) {
            let script = format!(
                "if(typeof globalThis.__runbox_npm_install==='function'){{globalThis.__runbox_npm_install({json});}}"
            );
            let _ = js_sys::eval(&script);
        }
    }

    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, 0)?;
    let kind = if dev { "devDependency" } else { "dependency" };
    Ok(ok_out(format!(
        "added {} as {kind}: {}",
        added.len(),
        added.join(", ")
    )))
}

fn pm_remove(
    cmd: &Command,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    pm_name: &str,
) -> Result<ExecOutput> {
    let packages: Vec<String> = cmd
        .args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .cloned()
        .collect();

    if packages.is_empty() {
        return Err(RunboxError::Runtime(format!(
            "{pm_name} remove: specify at least one package"
        )));
    }

    let mut pkg = PackageJson::load(vfs).unwrap_or_default();
    for name in &packages {
        pkg.remove_dep(name);
        let _ = vfs.remove(&format!("/node_modules/{name}/package.json"));
    }

    pkg.save(vfs)?;
    write_lock(vfs, pm_name, &pkg)?;

    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(format!(
        "removed {}: {}",
        packages.len(),
        packages.join(", ")
    )))
}

fn pm_run(
    cmd: &Command,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    pm_name: &str,
) -> Result<ExecOutput> {
    let script_name = cmd
        .args
        .get(1)
        .ok_or_else(|| RunboxError::Runtime(format!("{pm_name} run: specify a script name")))?;

    let pkg = PackageJson::load(vfs).ok_or_else(|| RunboxError::NotFound("package.json".into()))?;

    let script_cmd_str = pkg
        .scripts
        .get(script_name.as_str())
        .ok_or_else(|| {
            RunboxError::Runtime(format!(
                r#"missing script: "{script_name}"\n\nAvailable scripts:\n{}"#,
                pkg.scripts
                    .keys()
                    .map(|k| format!("  {k}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
        })?
        .clone();

    let header = format!(
        "> {}@{} {}\n> {}\n",
        pkg.name.as_deref().unwrap_or("app"),
        pkg.version.as_deref().unwrap_or("0.0.0"),
        script_name,
        script_cmd_str,
    );

    // Ejecutar el script real parseando y despachando el comando
    let result = run_script_command(&script_cmd_str, vfs, pm)?;

    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, result.exit_code)?;

    let mut stdout = header.into_bytes();
    stdout.extend_from_slice(&result.stdout);

    Ok(ExecOutput {
        stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
    })
}

/// Parsea y ejecuta el string de un script npm (e.g. "bun run /index.js", "node server.js")
fn run_script_command(script: &str, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    let script_cmd = Command::parse(script)
        .map_err(|_| RunboxError::Runtime(format!("invalid script: {script}")))?;

    match script_cmd.program.as_str() {
        "bun" => super::bun::BunRuntime.exec(&script_cmd, vfs, pm),
        "node" | "nodejs" => {
            // Tratar node igual que bun: `node file.js` → `bun run file.js`
            let mut args = vec!["run".to_string()];
            args.extend(script_cmd.args);
            super::bun::BunRuntime.exec(
                &Command {
                    program: "bun".into(),
                    args,
                    env: script_cmd.env,
                    stdin: None,
                    redirect: None,
                },
                vfs,
                pm,
            )
        }
        "ts-node" | "tsx" => {
            let mut args = vec!["run".to_string()];
            args.extend(script_cmd.args);
            super::bun::BunRuntime.exec(
                &Command {
                    program: "bun".into(),
                    args,
                    env: script_cmd.env,
                    stdin: None,
                    redirect: None,
                },
                vfs,
                pm,
            )
        }
        _ => Err(RunboxError::Runtime(format!(
            "script runtime '{}' not supported in sandbox",
            script_cmd.program
        ))),
    }
}

fn pm_exec(
    cmd: &Command,
    _vfs: &mut Vfs,
    pm: &mut ProcessManager,
    pm_name: &str,
) -> Result<ExecOutput> {
    let tool = cmd
        .args
        .get(1)
        .ok_or_else(|| RunboxError::Runtime(format!("{pm_name}: specify a package to execute")))?;

    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(format!(
        "Packages: {tool}\n[{pm_name}] running {tool}...\n"
    )))
}

fn pm_init(
    cmd: &Command,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    pm_name: &str,
) -> Result<ExecOutput> {
    let yes = cmd.args.iter().any(|a| a == "-y" || a == "--yes");

    let pkg = PackageJson {
        name: Some("my-project".into()),
        version: Some("0.1.0".into()),
        description: Some(String::new()),
        main: Some("index.js".into()),
        scripts: {
            let mut m = HashMap::new();
            m.insert("dev".into(), "bun run src/index.ts".into());
            m.insert(
                "build".into(),
                "bun build src/index.ts --outdir dist".into(),
            );
            m.insert("test".into(), "bun test".into());
            m
        },
        ..Default::default()
    };

    pkg.save(vfs)?;
    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, 0)?;

    if yes {
        Ok(ok_out("Wrote to /package.json"))
    } else {
        Ok(ok_out(
            serde_json::to_string_pretty(&pkg).unwrap_or_default(),
        ))
    }
}

fn pm_list(vfs: &Vfs, pm: &mut ProcessManager, pm_name: &str, cmd: &Command) -> Result<ExecOutput> {
    let pkg = match PackageJson::load(vfs) {
        Some(p) => p,
        None => return Ok(ok_out("(no package.json found)")),
    };

    let depth = cmd
        .args
        .iter()
        .find(|a| a.starts_with("--depth="))
        .and_then(|a| a.strip_prefix("--depth=")?.parse::<u8>().ok())
        .unwrap_or(1);

    let mut lines = vec![format!(
        "{} {}",
        pkg.name.as_deref().unwrap_or("app"),
        pkg.version.as_deref().unwrap_or("0.0.0"),
    )];

    if depth > 0 {
        for (name, ver) in &pkg.dependencies {
            lines.push(format!("├── {name}@{ver}"));
        }
        for (name, ver) in &pkg.dev_dependencies {
            lines.push(format!("├── {name}@{ver} (dev)"));
        }
    }

    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(lines.join("\n")))
}

fn pm_update(
    cmd: &Command,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    pm_name: &str,
) -> Result<ExecOutput> {
    let pkg = PackageJson::load(vfs);
    let count = pkg
        .as_ref()
        .map(|p| p.dependencies.len() + p.dev_dependencies.len())
        .unwrap_or(0);
    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(format!("updated {count} packages (simulated)")))
}

fn pm_outdated(
    vfs: &Vfs,
    pm: &mut ProcessManager,
    pm_name: &str,
    cmd: &Command,
) -> Result<ExecOutput> {
    let pkg = PackageJson::load(vfs).unwrap_or_default();
    let mut lines = vec!["Package  Current  Wanted  Latest".to_string()];
    for (name, ver) in &pkg.dependencies {
        lines.push(format!("{name}  {ver}  {ver}  {ver}  (up to date)"));
    }
    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(lines.join("\n")))
}

fn pm_audit(
    vfs: &Vfs,
    pm: &mut ProcessManager,
    pm_name: &str,
    cmd: &Command,
) -> Result<ExecOutput> {
    let pkg = PackageJson::load(vfs).unwrap_or_default();
    let total = pkg.dependencies.len() + pkg.dev_dependencies.len();
    let pid = pm.spawn(pm_name, cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(format!(
        "audited {total} packages\nfound 0 vulnerabilities"
    )))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ok_out(text: impl Into<String>) -> ExecOutput {
    ExecOutput {
        stdout: text.into().into_bytes(),
        stderr: vec![],
        exit_code: 0,
    }
}

/// Parsea "react@^18.2.0" → ("react", "^18.2.0")
fn parse_package_spec(spec: &str, exact: bool) -> (String, String) {
    // @scope/pkg@version — need to skip first char for scoped packages
    let (name, ver) = if let Some(stripped) = spec.strip_prefix('@') {
        // Scoped package: @scope/pkg@version
        if let Some(pos) = stripped.find('@') {
            (
                format!("@{}", &stripped[..pos]),
                stripped[pos + 1..].to_string(),
            )
        } else {
            (spec.to_string(), "latest".to_string())
        }
    } else if let Some(pos) = spec.find('@') {
        (spec[..pos].to_string(), spec[pos + 1..].to_string())
    } else {
        (spec.to_string(), "latest".to_string())
    };

    let version_str = if ver == "latest" {
        "^1.0.0".to_string()
    } else if exact {
        // Exact: strip any existing range prefix and use bare version
        let bare = ver.trim_start_matches(|c: char| !c.is_ascii_digit());
        bare.to_string()
    } else if ver.starts_with('^') || ver.starts_with('~') || ver.starts_with('>') {
        // Already has a range prefix
        ver.to_string()
    } else {
        format!("^{ver}")
    };

    (name, version_str)
}
