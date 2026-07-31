use crate::error::{Result, RunboxError};
use serde::{Deserialize, Serialize};
/// Virtual Filesystem — sistema de archivos en memoria de alto rendimiento.
/// Todos los runtimes (Bun, Python, etc.) operan sobre este VFS.
/// Incluye tracking de cambios para hot-reload, metadatos de archivos,
/// content hashing, glob matching, y estadísticas.
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    File(Vec<u8>),
    Dir(BTreeMap<String, Node>),
}

// ── File Metadata ───────────────────────────────────────────────────────────

/// Metadatos de un archivo en el VFS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// Tamaño en bytes.
    pub size: usize,
    /// Hash del contenido (FNV-1a para rapidez).
    pub content_hash: u64,
    /// Timestamp de creación (ms).
    pub created_at: u64,
    /// Timestamp de última modificación (ms).
    pub modified_at: u64,
    /// Si el archivo contiene datos binarios (no UTF-8 válido).
    pub is_binary: bool,
}

impl FileMetadata {
    fn from_content(content: &[u8], now_ms: u64) -> Self {
        Self {
            size: content.len(),
            content_hash: fnv1a_hash(content),
            created_at: now_ms,
            modified_at: now_ms,
            is_binary: std::str::from_utf8(content).is_err(),
        }
    }

    fn update(&mut self, content: &[u8], now_ms: u64) {
        self.size = content.len();
        self.content_hash = fnv1a_hash(content);
        self.modified_at = now_ms;
        self.is_binary = std::str::from_utf8(content).is_err();
    }
}

/// FNV-1a hash — rápido y buena distribución para detección de cambios.
fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Debug)]
pub struct Vfs {
    root: Node,
    /// Paths modificados desde el último drain_changes().
    pending_changes: Vec<FileChange>,
    /// Metadatos de archivos indexados por path.
    metadata: BTreeMap<String, FileMetadata>,
    /// Estadísticas globales del VFS.
    stats: VfsStats,
}

/// Estadísticas globales del VFS.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VfsStats {
    /// Número total de archivos.
    pub file_count: usize,
    /// Tamaño total en bytes.
    pub total_size: usize,
    /// Número de operaciones de escritura.
    pub write_ops: u64,
    /// Número de operaciones de lectura.
    pub read_ops: u64,
    /// Número de archivos binarios.
    pub binary_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    /// Razón o intención detrás del cambio (escrita por el agente que lo generó).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

impl Vfs {
    pub fn new() -> Self {
        Self {
            root: Node::Dir(BTreeMap::new()),
            pending_changes: Vec::new(),
            metadata: BTreeMap::new(),
            stats: VfsStats::default(),
        }
    }

    /// Escribe un archivo. Crea directorios intermedios si no existen.
    pub fn write(&mut self, path: &str, content: Vec<u8>) -> Result<()> {
        self.write_at(path, content, 0)
    }

    /// Escribe un archivo con timestamp específico.
    pub fn write_at(&mut self, path: &str, content: Vec<u8>, now_ms: u64) -> Result<()> {
        let parts = split_path(path);
        let existed = get(&self.root, &parts).is_ok();

        // Actualizar metadatos
        let is_binary = std::str::from_utf8(&content).is_err();
        if existed {
            if let Some(meta) = self.metadata.get_mut(path) {
                let old_size = meta.size;
                let was_binary = meta.is_binary;
                meta.update(&content, now_ms);
                self.stats.total_size =
                    self.stats.total_size.saturating_sub(old_size) + content.len();
                if was_binary && !is_binary {
                    self.stats.binary_count = self.stats.binary_count.saturating_sub(1);
                } else if !was_binary && is_binary {
                    self.stats.binary_count += 1;
                }
            }
        } else {
            self.metadata.insert(
                path.to_string(),
                FileMetadata::from_content(&content, now_ms),
            );
            self.stats.file_count += 1;
            self.stats.total_size += content.len();
            if is_binary {
                self.stats.binary_count += 1;
            }
        }
        self.stats.write_ops += 1;

        insert(&mut self.root, &parts, content)?;

        // No emitir cambios en archivos internos de .git
        if !path.starts_with("/.git/") {
            self.pending_changes.push(FileChange {
                path: path.to_string(),
                kind: if existed {
                    ChangeKind::Modified
                } else {
                    ChangeKind::Created
                },
                reason: None,
            });
        }
        Ok(())
    }

    /// Lee el contenido de un archivo.
    pub fn read(&self, path: &str) -> Result<&[u8]> {
        let parts = split_path(path);
        match get(&self.root, &parts)? {
            Node::File(bytes) => Ok(bytes),
            Node::Dir(_) => Err(RunboxError::Vfs(format!("{path} is a directory"))),
        }
    }

    /// Lee el contenido como string (falla si es binario inválido).
    pub fn read_string(&self, path: &str) -> Result<&str> {
        let bytes = self.read(path)?;
        std::str::from_utf8(bytes)
            .map_err(|_| RunboxError::Vfs(format!("{path} contains invalid UTF-8")))
    }

    /// Lista los entries de un directorio.
    pub fn list(&self, path: &str) -> Result<Vec<String>> {
        let parts = split_path(path);
        let node = if parts.is_empty() {
            &self.root
        } else {
            get(&self.root, &parts)?
        };
        match node {
            Node::Dir(entries) => Ok(entries.keys().cloned().collect()),
            Node::File(_) => Err(RunboxError::Vfs(format!("{path} is not a directory"))),
        }
    }

    /// Elimina un archivo o directorio.
    pub fn remove(&mut self, path: &str) -> Result<()> {
        let parts = split_path(path);
        remove(&mut self.root, &parts)?;

        // Actualizar metadatos y stats
        if let Some(meta) = self.metadata.remove(path) {
            self.stats.file_count = self.stats.file_count.saturating_sub(1);
            self.stats.total_size = self.stats.total_size.saturating_sub(meta.size);
            if meta.is_binary {
                self.stats.binary_count = self.stats.binary_count.saturating_sub(1);
            }
        }
        // Limpiar metadatos de hijos (caso directorio)
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let children: Vec<String> = self
            .metadata
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for child_path in children {
            if let Some(meta) = self.metadata.remove(&child_path) {
                self.stats.file_count = self.stats.file_count.saturating_sub(1);
                self.stats.total_size = self.stats.total_size.saturating_sub(meta.size);
                if meta.is_binary {
                    self.stats.binary_count = self.stats.binary_count.saturating_sub(1);
                }
            }
        }

        if !path.starts_with("/.git/") {
            self.pending_changes.push(FileChange {
                path: path.to_string(),
                kind: ChangeKind::Deleted,
                reason: None,
            });
        }
        Ok(())
    }

    /// Verifica si existe un path.
    pub fn exists(&self, path: &str) -> bool {
        let parts = split_path(path);
        get(&self.root, &parts).is_ok()
    }

    /// Crea un directorio (y sus padres si no existen).
    pub fn mkdir(&mut self, path: &str) -> Result<()> {
        let parts = split_path(path);
        mkdir_recursive(&mut self.root, &parts)?;

        // No emitir cambios en archivos internos de .git
        if !path.starts_with("/.git/") {
            self.pending_changes.push(FileChange {
                path: path.to_string(),
                kind: ChangeKind::Created,
                reason: None,
            });
        }
        Ok(())
    }

    /// Verifica si un path es un directorio.
    pub fn is_dir(&self, path: &str) -> bool {
        let parts = split_path(path);
        matches!(get(&self.root, &parts), Ok(Node::Dir(_)))
    }

    /// Retorna todos los cambios pendientes y limpia la cola.
    pub fn drain_changes(&mut self) -> Vec<FileChange> {
        std::mem::take(&mut self.pending_changes)
    }

    /// Retorna cambios sin limpiar la cola.
    pub fn peek_changes(&self) -> &[FileChange] {
        &self.pending_changes
    }

    // ── File Metadata API ────────────────────────────────────────────────────

    /// Retorna los metadatos de un archivo.
    pub fn stat(&self, path: &str) -> Option<&FileMetadata> {
        self.metadata.get(path)
    }

    /// Retorna el content hash de un archivo (para detección de cambios).
    pub fn content_hash(&self, path: &str) -> Option<u64> {
        self.metadata.get(path).map(|m| m.content_hash)
    }

    /// Verifica si el contenido de un archivo cambió comparando hashes.
    pub fn has_changed(&self, path: &str, previous_hash: u64) -> bool {
        self.metadata
            .get(path)
            .is_none_or(|m| m.content_hash != previous_hash)
    }

    // ── Glob Pattern Matching ────────────────────────────────────────────────

    /// Busca archivos que coincidan con un patrón glob.
    /// Soporta: * (cualquier nombre), ** (cualquier profundidad), ? (un carácter).
    pub fn glob(&self, pattern: &str) -> Vec<String> {
        let all_paths = self.all_file_paths();
        all_paths
            .into_iter()
            .filter(|path| glob_matches(path, pattern))
            .collect()
    }

    /// Retorna todos los paths de archivos en el VFS.
    pub fn all_file_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        collect_paths(&self.root, String::new(), &mut paths);
        paths
    }

    // ── Statistics ───────────────────────────────────────────────────────────

    /// Retorna las estadísticas del VFS.
    pub fn stats(&self) -> &VfsStats {
        &self.stats
    }

    /// Calcula el tamaño de un directorio (recursivo).
    pub fn dir_size(&self, path: &str) -> usize {
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        self.metadata
            .iter()
            .filter(|(p, _)| p.starts_with(&prefix) || *p == path)
            .map(|(_, m)| m.size)
            .sum()
    }

    /// Retorna info del VFS como JSON.
    pub fn info_json(&self) -> String {
        serde_json::json!({
            "file_count": self.stats.file_count,
            "total_size": self.stats.total_size,
            "binary_count": self.stats.binary_count,
            "write_ops": self.stats.write_ops,
            "read_ops": self.stats.read_ops,
            "metadata_entries": self.metadata.len(),
        })
        .to_string()
    }

    // ── Lazy Loading ────────────────────────────────────────────────────────
    //
    // Permite registrar archivos con solo metadatos (sin contenido) y
    // cargarlos bajo demanda. Útil para proyectos con 10,000+ archivos.

    /// Registra un archivo como "lazy" (solo metadatos, sin contenido).
    /// El contenido se carga posteriormente con `fulfill_lazy`.
    pub fn register_lazy(&mut self, path: &str, size: usize, content_hash: u64, now_ms: u64) {
        let meta = FileMetadata {
            size,
            content_hash,
            created_at: now_ms,
            modified_at: now_ms,
            is_binary: false,
        };
        self.metadata.insert(path.to_string(), meta);
        self.stats.file_count += 1;
        self.stats.total_size += size;
        // Don't insert into the tree — content will be loaded on demand
    }

    /// Carga el contenido de un archivo previamente registrado como lazy.
    pub fn fulfill_lazy(&mut self, path: &str, content: Vec<u8>) -> Result<()> {
        let now_ms = 0; // Use 0; actual timestamp can be passed externally
        if let Some(meta) = self.metadata.get_mut(path) {
            meta.update(&content, now_ms);
        }
        let parts = split_path(path);
        insert(&mut self.root, &parts, content)?;
        Ok(())
    }

    /// Verifica si un archivo está registrado pero no cargado (lazy).
    pub fn is_lazy(&self, path: &str) -> bool {
        self.metadata.contains_key(path) && !self.exists(path)
    }

    // ── Streaming Reads ─────────────────────────────────────────────────────
    //
    // Para archivos grandes, permite leer por chunks sin cargar todo en memoria.

    /// Lee un chunk de un archivo binario (offset + length).
    pub fn read_chunk(&self, path: &str, offset: usize, length: usize) -> Result<&[u8]> {
        let bytes = self.read(path)?;
        if offset >= bytes.len() {
            return Ok(&[]);
        }
        let end = (offset + length).min(bytes.len());
        Ok(&bytes[offset..end])
    }

    /// Retorna el tamaño de un archivo sin leer el contenido completo.
    pub fn file_size(&self, path: &str) -> Option<usize> {
        self.metadata.get(path).map(|m| m.size)
    }

    // ── Snapshot & Restore ──────────────────────────────────────────────────
    //
    // Para sincronización incremental con WebSocket viewers.

    /// Genera un snapshot del VFS: mapa de paths → content hashes.
    pub fn snapshot(&self) -> HashMap<String, u64> {
        self.metadata
            .iter()
            .map(|(path, meta)| (path.clone(), meta.content_hash))
            .collect()
    }

    /// Calcula un diff entre un snapshot anterior y el estado actual.
    /// Retorna los paths que cambiaron (nuevos, modificados, eliminados).
    pub fn diff_snapshot(&self, previous: &HashMap<String, u64>) -> VfsSnapshotDiff {
        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut deleted = Vec::new();

        // Check current vs previous
        for (path, meta) in &self.metadata {
            match previous.get(path) {
                Some(&prev_hash) if prev_hash != meta.content_hash => {
                    modified.push(path.clone());
                }
                None => {
                    added.push(path.clone());
                }
                _ => {} // Unchanged
            }
        }

        // Check deleted
        for path in previous.keys() {
            if !self.metadata.contains_key(path) {
                deleted.push(path.clone());
            }
        }

        VfsSnapshotDiff {
            added,
            modified,
            deleted,
        }
    }

    // ── Compression ─────────────────────────────────────────────────────────
    //
    // Compresión simple para almacenar archivos grandes en menos memoria.

    /// Escribe un archivo con compresión (usando flate2 deflate).
    pub fn write_compressed(&mut self, path: &str, content: Vec<u8>) -> Result<()> {
        use flate2::Compression;
        use flate2::write::DeflateEncoder;
        use std::io::Write;

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
        let _ = encoder.write_all(&content);
        let compressed = encoder.finish().unwrap_or_else(|_| content.clone());

        // Only use compressed version if it's actually smaller
        if compressed.len() < content.len() {
            // Store with a marker prefix so we know it's compressed
            let mut stored = Vec::with_capacity(compressed.len() + 4);
            stored.extend_from_slice(b"CMP\x01"); // Compression marker
            stored.extend(compressed);

            // Metadata tracks the original size
            let now_ms = 0;
            let existed = self.exists(path);
            if existed {
                if let Some(meta) = self.metadata.get_mut(path) {
                    let old_size = meta.size;
                    meta.size = content.len(); // Original size
                    meta.content_hash = fnv1a_hash(&content);
                    meta.modified_at = now_ms;
                    self.stats.total_size =
                        self.stats.total_size.saturating_sub(old_size) + stored.len();
                }
            } else {
                let mut meta = FileMetadata::from_content(&content, now_ms);
                meta.size = content.len();
                self.metadata.insert(path.to_string(), meta);
                self.stats.file_count += 1;
                self.stats.total_size += stored.len();
            }

            let parts = split_path(path);
            insert(&mut self.root, &parts, stored)?;
        } else {
            // Not worth compressing
            self.write(path, content)?;
        }
        Ok(())
    }

    /// Lee un archivo, descomprimiendo si necesario.
    pub fn read_maybe_compressed(&self, path: &str) -> Result<Vec<u8>> {
        let bytes = self.read(path)?;
        if bytes.len() > 4 && &bytes[..4] == b"CMP\x01" {
            use flate2::read::DeflateDecoder;
            use std::io::Read;

            let mut decoder = DeflateDecoder::new(&bytes[4..]);
            let mut result = Vec::new();
            decoder
                .read_to_end(&mut result)
                .map_err(|e| RunboxError::Vfs(format!("Decompression failed: {e}")))?;
            Ok(result)
        } else {
            Ok(bytes.to_vec())
        }
    }

    // ── Batch Operations ────────────────────────────────────────────────────

    /// Escribe múltiples archivos de una vez (más eficiente que llamar write() en loop).
    pub fn write_batch(&mut self, files: Vec<(String, Vec<u8>)>) -> Result<Vec<String>> {
        let mut written = Vec::new();
        for (path, content) in files {
            self.write(&path, content)?;
            written.push(path);
        }
        Ok(written)
    }

    /// Lee múltiples archivos de una vez.
    pub fn read_batch(&self, paths: &[&str]) -> HashMap<String, Result<Vec<u8>>> {
        let mut results = HashMap::new();
        for &path in paths {
            let result = self.read(path).map(|b| b.to_vec());
            results.insert(path.to_string(), result);
        }
        results
    }
}

/// Resultado de un diff entre snapshots.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VfsSnapshotDiff {
    /// Archivos nuevos.
    pub added: Vec<String>,
    /// Archivos modificados.
    pub modified: Vec<String>,
    /// Archivos eliminados.
    pub deleted: Vec<String>,
}

impl VfsSnapshotDiff {
    /// Retorna true si no hay cambios.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    /// Número total de cambios.
    pub fn total_changes(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

fn split_path(path: &str) -> Vec<String> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn get<'a>(node: &'a Node, parts: &[String]) -> Result<&'a Node> {
    if parts.is_empty() {
        return Ok(node);
    }
    match node {
        Node::Dir(entries) => {
            let child = entries
                .get(&parts[0])
                .ok_or_else(|| RunboxError::NotFound(parts[0].clone()))?;
            get(child, &parts[1..])
        }
        Node::File(_) => Err(RunboxError::Vfs("expected directory".into())),
    }
}

fn insert(node: &mut Node, parts: &[String], content: Vec<u8>) -> Result<()> {
    match node {
        Node::Dir(entries) => {
            if parts.len() == 1 {
                entries.insert(parts[0].clone(), Node::File(content));
                Ok(())
            } else {
                let child = entries
                    .entry(parts[0].clone())
                    .or_insert_with(|| Node::Dir(BTreeMap::new()));
                insert(child, &parts[1..], content)
            }
        }
        Node::File(_) => Err(RunboxError::Vfs("expected directory".into())),
    }
}

fn mkdir_recursive(node: &mut Node, parts: &[String]) -> Result<()> {
    match node {
        Node::Dir(entries) => {
            if parts.is_empty() {
                return Ok(());
            }
            if parts.len() == 1 {
                // Crear el directorio final si no existe
                entries
                    .entry(parts[0].clone())
                    .or_insert_with(|| Node::Dir(BTreeMap::new()));
                Ok(())
            } else {
                // Crear directorios intermedios recursivamente
                let child = entries
                    .entry(parts[0].clone())
                    .or_insert_with(|| Node::Dir(BTreeMap::new()));
                mkdir_recursive(child, &parts[1..])
            }
        }
        Node::File(_) => Err(RunboxError::Vfs("expected directory".into())),
    }
}

fn remove(node: &mut Node, parts: &[String]) -> Result<()> {
    match node {
        Node::Dir(entries) => {
            if parts.len() == 1 {
                entries
                    .remove(&parts[0])
                    .ok_or_else(|| RunboxError::NotFound(parts[0].clone()))?;
                Ok(())
            } else {
                let child = entries
                    .get_mut(&parts[0])
                    .ok_or_else(|| RunboxError::NotFound(parts[0].clone()))?;
                remove(child, &parts[1..])
            }
        }
        Node::File(_) => Err(RunboxError::Vfs("expected directory".into())),
    }
}

/// Recolecta todos los paths de archivos recursivamente.
fn collect_paths(node: &Node, prefix: String, paths: &mut Vec<String>) {
    match node {
        Node::File(_) => {
            paths.push(format!("/{prefix}"));
        }
        Node::Dir(entries) => {
            for (name, child) in entries {
                let child_path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                collect_paths(child, child_path, paths);
            }
        }
    }
}

/// Glob pattern matching simple pero funcional.
/// Soporta: * (cualquier segmento), ** (cualquier profundidad), ? (un carácter).
fn glob_matches(path: &str, pattern: &str) -> bool {
    let path_parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let pattern_parts: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    glob_match_parts(&path_parts, &pattern_parts)
}

fn glob_match_parts(path: &[&str], pattern: &[&str]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern[0] == "**" {
        // ** matches zero or more path segments
        if glob_match_parts(path, &pattern[1..]) {
            return true;
        }
        if !path.is_empty() {
            return glob_match_parts(&path[1..], pattern);
        }
        return false;
    }
    if path.is_empty() {
        return false;
    }
    if glob_match_segment(path[0], pattern[0]) {
        glob_match_parts(&path[1..], &pattern[1..])
    } else {
        false
    }
}

fn glob_match_segment(segment: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let seg_bytes = segment.as_bytes();
    let pat_bytes = pattern.as_bytes();
    glob_match_chars(seg_bytes, pat_bytes)
}

fn glob_match_chars(s: &[u8], p: &[u8]) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    if p[0] == b'*' {
        // * matches zero or more characters within a segment
        glob_match_chars(s, &p[1..]) || (!s.is_empty() && glob_match_chars(&s[1..], p))
    } else if p[0] == b'?' {
        !s.is_empty() && glob_match_chars(&s[1..], &p[1..])
    } else {
        !s.is_empty() && s[0] == p[0] && glob_match_chars(&s[1..], &p[1..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read() {
        let mut vfs = Vfs::new();
        vfs.write("/src/main.ts", b"console.log('hi')".to_vec())
            .unwrap();
        assert_eq!(vfs.read("/src/main.ts").unwrap(), b"console.log('hi')");
    }

    #[test]
    fn list_dir() {
        let mut vfs = Vfs::new();
        vfs.write("/src/a.ts", vec![]).unwrap();
        vfs.write("/src/b.ts", vec![]).unwrap();
        let mut entries = vfs.list("/src").unwrap();
        entries.sort();
        assert_eq!(entries, vec!["a.ts", "b.ts"]);
    }

    #[test]
    fn not_found() {
        let vfs = Vfs::new();
        assert!(vfs.read("/nope.ts").is_err());
    }

    #[test]
    fn change_tracking() {
        let mut vfs = Vfs::new();
        vfs.write("/index.ts", b"v1".to_vec()).unwrap();
        vfs.write("/index.ts", b"v2".to_vec()).unwrap();
        vfs.write("/style.css", b"body{}".to_vec()).unwrap();

        let changes = vfs.drain_changes();
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].kind, ChangeKind::Created);
        assert_eq!(changes[1].kind, ChangeKind::Modified);
        assert_eq!(changes[2].path, "/style.css");

        // Después del drain debe estar vacía
        assert!(vfs.drain_changes().is_empty());
    }

    #[test]
    fn git_changes_ignored() {
        let mut vfs = Vfs::new();
        vfs.write("/.git/HEAD", b"ref: refs/heads/main\n".to_vec())
            .unwrap();
        vfs.write("/src/app.ts", b"code".to_vec()).unwrap();

        let changes = vfs.drain_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "/src/app.ts");
    }

    #[test]
    fn file_metadata_tracking() {
        let mut vfs = Vfs::new();
        vfs.write("/test.txt", b"hello world".to_vec()).unwrap();

        let meta = vfs.stat("/test.txt").unwrap();
        assert_eq!(meta.size, 11);
        assert!(!meta.is_binary);

        let hash1 = vfs.content_hash("/test.txt").unwrap();
        vfs.write("/test.txt", b"changed".to_vec()).unwrap();
        assert!(vfs.has_changed("/test.txt", hash1));
    }

    #[test]
    fn vfs_stats() {
        let mut vfs = Vfs::new();
        vfs.write("/a.txt", b"aaa".to_vec()).unwrap();
        vfs.write("/b.txt", b"bbb".to_vec()).unwrap();

        assert_eq!(vfs.stats().file_count, 2);
        assert_eq!(vfs.stats().total_size, 6);
        assert_eq!(vfs.stats().write_ops, 2);

        vfs.remove("/a.txt").unwrap();
        assert_eq!(vfs.stats().file_count, 1);
        assert_eq!(vfs.stats().total_size, 3);
    }

    #[test]
    fn glob_matching() {
        let mut vfs = Vfs::new();
        vfs.write("/src/app.ts", b"".to_vec()).unwrap();
        vfs.write("/src/utils.ts", b"".to_vec()).unwrap();
        vfs.write("/src/styles/main.css", b"".to_vec()).unwrap();
        vfs.write("/test/app.test.ts", b"".to_vec()).unwrap();

        let ts_files = vfs.glob("**/*.ts");
        assert!(ts_files.len() >= 3);

        let src_files = vfs.glob("src/*.ts");
        assert!(src_files.len() >= 2);
    }

    #[test]
    fn read_string_works() {
        let mut vfs = Vfs::new();
        vfs.write("/hello.txt", b"hello".to_vec()).unwrap();
        assert_eq!(vfs.read_string("/hello.txt").unwrap(), "hello");
    }

    #[test]
    fn dir_size_calculation() {
        let mut vfs = Vfs::new();
        vfs.write("/proj/a.ts", vec![0u8; 100]).unwrap();
        vfs.write("/proj/b.ts", vec![0u8; 200]).unwrap();
        vfs.write("/other/c.ts", vec![0u8; 50]).unwrap();

        assert_eq!(vfs.dir_size("/proj"), 300);
    }

    #[test]
    fn fnv1a_hash_consistency() {
        let h1 = fnv1a_hash(b"hello");
        let h2 = fnv1a_hash(b"hello");
        let h3 = fnv1a_hash(b"world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }
}
