/// Red y Cache — sistema de cache HTTP y optimizaciones de red.
///
/// Provee:
/// - Cache HTTP con Cache-Control, ETag, If-None-Match
/// - Estadísticas de cache (hits, misses, size)
/// - CDN URL generation para paquetes npm (esm.sh, skypack, unpkg)
/// - Compresión de respuestas (gzip via flate2)
/// - Preloading predictivo basado en análisis de imports
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── HTTP Cache ──────────────────────────────────────────────────────────────

/// Entrada en el cache HTTP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// URL original de la request.
    pub url: String,
    /// Cuerpo de la respuesta (bytes).
    #[serde(with = "serde_bytes_compat")]
    pub body: Vec<u8>,
    /// Headers de respuesta relevantes.
    pub headers: HashMap<String, String>,
    /// ETag del servidor.
    pub etag: Option<String>,
    /// Timestamp de cuando se cacheó (ms).
    pub cached_at: u64,
    /// Tiempo de expiración (ms desde epoch). 0 = no expira.
    pub expires_at: u64,
    /// Tamaño original sin comprimir.
    pub original_size: usize,
    /// Número de veces que se ha accedido desde el cache.
    pub hit_count: u64,
}

/// Módulo para serializar Vec<u8> como bytes en serde.
mod serde_bytes_compat {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<u8>::deserialize(deserializer)
    }
}

/// Cache HTTP con soporte para Cache-Control, ETag, y expiración.
#[derive(Debug)]
pub struct HttpCache {
    entries: HashMap<String, CacheEntry>,
    /// Tamaño máximo del cache en bytes.
    max_size: usize,
    /// Tamaño actual del cache en bytes.
    current_size: usize,
    /// Estadísticas.
    stats: CacheStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub total_bytes_saved: u64,
}

impl HttpCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_size,
            current_size: 0,
            stats: CacheStats::default(),
        }
    }

    /// Busca una entrada en el cache. Retorna None si no existe o expiró.
    pub fn get(&mut self, url: &str, now_ms: u64) -> Option<&CacheEntry> {
        // Check if exists and not expired
        let expired = self
            .entries
            .get(url)
            .is_none_or(|entry| entry.expires_at > 0 && now_ms > entry.expires_at);

        if expired {
            if self.entries.contains_key(url) {
                // Remove expired entry
                if let Some(entry) = self.entries.remove(url) {
                    self.current_size = self.current_size.saturating_sub(entry.body.len());
                }
            }
            self.stats.misses += 1;
            return None;
        }

        // Increment hit count
        if let Some(entry) = self.entries.get_mut(url) {
            entry.hit_count += 1;
            self.stats.hits += 1;
            self.stats.total_bytes_saved += entry.body.len() as u64;
        }

        self.entries.get(url)
    }

    /// Almacena una respuesta en el cache.
    pub fn put(&mut self, url: &str, body: Vec<u8>, headers: HashMap<String, String>, now_ms: u64) {
        let size = body.len();

        // Parse Cache-Control header
        let max_age = headers
            .get("cache-control")
            .or_else(|| headers.get("Cache-Control"))
            .and_then(|cc| parse_max_age(cc));

        let etag = headers.get("etag").or_else(|| headers.get("ETag")).cloned();

        let expires_at = match max_age {
            Some(seconds) => now_ms + (seconds as u64 * 1000),
            None => 0, // No expiration
        };

        // Check no-store
        if let Some(cc) = headers
            .get("cache-control")
            .or_else(|| headers.get("Cache-Control"))
            && cc.contains("no-store")
        {
            return; // Don't cache
        }

        // Don't cache if single entry exceeds max
        if size > self.max_size {
            return;
        }

        // Evict if needed
        while self.current_size + size > self.max_size && !self.entries.is_empty() {
            self.evict_lru();
        }

        // Remove old entry if exists
        if let Some(old) = self.entries.remove(url) {
            self.current_size = self.current_size.saturating_sub(old.body.len());
        }

        self.entries.insert(
            url.to_string(),
            CacheEntry {
                url: url.to_string(),
                body,
                headers,
                etag,
                cached_at: now_ms,
                expires_at,
                original_size: size,
                hit_count: 0,
            },
        );
        self.current_size += size;
    }

    /// Verifica si la respuesta del servidor indica que el cache es válido (304 Not Modified).
    pub fn validate_etag(&self, url: &str, server_etag: &str) -> bool {
        self.entries
            .get(url)
            .and_then(|e| e.etag.as_ref())
            .is_some_and(|cached_etag| cached_etag == server_etag)
    }

    /// Retorna el ETag almacenado para hacer If-None-Match requests.
    pub fn get_etag(&self, url: &str) -> Option<&str> {
        self.entries.get(url).and_then(|e| e.etag.as_deref())
    }

    /// Evicta la entrada menos recientemente usada.
    fn evict_lru(&mut self) {
        let lru_key = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.hit_count)
            .map(|(k, _)| k.clone());

        if let Some(key) = lru_key
            && let Some(entry) = self.entries.remove(&key)
        {
            self.current_size = self.current_size.saturating_sub(entry.body.len());
            self.stats.evictions += 1;
        }
    }

    /// Limpia el cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_size = 0;
    }

    /// Retorna estadísticas del cache.
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Retorna el número de entradas.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Retorna el tamaño actual del cache.
    pub fn current_size(&self) -> usize {
        self.current_size
    }

    /// Retorna info del cache como JSON.
    pub fn info_json(&self) -> String {
        serde_json::json!({
            "entries": self.entries.len(),
            "size_bytes": self.current_size,
            "max_size_bytes": self.max_size,
            "stats": self.stats,
            "fill_percent": if self.max_size > 0 {
                ((self.current_size as f64 / self.max_size as f64 * 100.0).clamp(0.0, 100.0)) as u64
            } else { 0 }
        })
        .to_string()
    }
}

impl Default for HttpCache {
    fn default() -> Self {
        Self::new(50 * 1024 * 1024) // 50 MB default
    }
}

fn parse_max_age(cache_control: &str) -> Option<u32> {
    for directive in cache_control.split(',') {
        let directive = directive.trim();
        if let Some(age_str) = directive.strip_prefix("max-age=") {
            return age_str.trim().parse().ok();
        }
    }
    None
}

// ── CDN Integration ─────────────────────────────────────────────────────────

/// Proveedores CDN para paquetes npm.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CdnProvider {
    /// esm.sh — ESM modules
    Esm,
    /// cdn.skypack.dev — optimized ESM
    Skypack,
    /// unpkg.com — UMD/CommonJS
    Unpkg,
    /// cdn.jsdelivr.net — multi-format
    Jsdelivr,
}

impl CdnProvider {
    /// Genera la URL del CDN para un paquete npm.
    pub fn url(&self, package: &str, version: &str) -> String {
        match self {
            CdnProvider::Esm => format!("https://esm.sh/{package}@{version}"),
            CdnProvider::Skypack => format!("https://cdn.skypack.dev/{package}@{version}"),
            CdnProvider::Unpkg => format!("https://unpkg.com/{package}@{version}"),
            CdnProvider::Jsdelivr => format!("https://cdn.jsdelivr.net/npm/{package}@{version}"),
        }
    }

    /// Genera URL para un archivo específico dentro del paquete.
    pub fn file_url(&self, package: &str, version: &str, file: &str) -> String {
        let file = file.trim_start_matches('/');
        match self {
            CdnProvider::Esm => format!("https://esm.sh/{package}@{version}/{file}"),
            CdnProvider::Skypack => format!("https://cdn.skypack.dev/{package}@{version}/{file}"),
            CdnProvider::Unpkg => format!("https://unpkg.com/{package}@{version}/{file}"),
            CdnProvider::Jsdelivr => {
                format!("https://cdn.jsdelivr.net/npm/{package}@{version}/{file}")
            }
        }
    }
}

/// Genera URLs de CDN para todos los proveedores.
pub fn cdn_urls(package: &str, version: &str) -> HashMap<String, String> {
    let mut urls = HashMap::new();
    urls.insert("esm".into(), CdnProvider::Esm.url(package, version));
    urls.insert("skypack".into(), CdnProvider::Skypack.url(package, version));
    urls.insert("unpkg".into(), CdnProvider::Unpkg.url(package, version));
    urls.insert(
        "jsdelivr".into(),
        CdnProvider::Jsdelivr.url(package, version),
    );
    urls
}

// ── Import Analysis (Predictive Preloading) ─────────────────────────────────

/// Analiza imports en código JavaScript/TypeScript para preloading predictivo.
pub fn analyze_imports(source: &str) -> Vec<String> {
    let mut imports = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        // import X from 'package'
        // import { X } from 'package'
        // import 'package'
        if (trimmed.starts_with("import ") || trimmed.starts_with("import\t"))
            && let Some(pkg) = extract_import_source(trimmed)
        {
            imports.push(pkg);
        }

        // require('package')
        if let Some(start) = trimmed.find("require(") {
            let after = &trimmed[start + 8..];
            if let Some(pkg) = extract_string_literal(after) {
                imports.push(pkg);
            }
        }

        // dynamic import('package')
        if let Some(start) = trimmed.find("import(") {
            let after = &trimmed[start + 7..];
            if let Some(pkg) = extract_string_literal(after) {
                imports.push(pkg);
            }
        }
    }

    imports.sort();
    imports.dedup();
    imports
}

fn extract_import_source(line: &str) -> Option<String> {
    // Look for 'from' keyword followed by string literal
    if let Some(from_pos) = line.rfind(" from ") {
        let after = line[from_pos + 6..].trim().trim_end_matches(';');
        return extract_string_literal(after);
    }
    // import 'side-effect'
    let after_import = line.strip_prefix("import ")?.trim().trim_end_matches(';');
    if after_import.starts_with('\'') || after_import.starts_with('"') {
        return extract_string_literal(after_import);
    }
    None
}

fn extract_string_literal(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(stripped) = s.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(s[1..1 + end].to_string())
    } else if let Some(stripped) = s.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(s[1..1 + end].to_string())
    } else if let Some(stripped) = s.strip_prefix('`') {
        let end = stripped.find('`')?;
        Some(s[1..1 + end].to_string())
    } else {
        None
    }
}

// ── Response Compression ────────────────────────────────────────────────────

/// Comprime datos con gzip (usando flate2).
pub fn compress_gzip(data: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_else(|_| data.to_vec())
}

/// Descomprime datos gzip.
pub fn decompress_gzip(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let mut decoder = GzDecoder::new(data);
    let mut result = Vec::new();
    decoder.read_to_end(&mut result).ok()?;
    Some(result)
}

/// Determina si una respuesta debe comprimirse basándose en el Content-Type.
pub fn should_compress(content_type: &str) -> bool {
    let compressible = [
        "text/",
        "application/json",
        "application/javascript",
        "application/xml",
        "application/xhtml",
        "application/wasm",
        "image/svg+xml",
    ];
    compressible
        .iter()
        .any(|ct| content_type.starts_with(ct) || content_type.contains(ct))
}

/// Determina el Content-Type basado en la extensión del archivo.
pub fn content_type_for_ext(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "ts" | "tsx" => "application/typescript; charset=utf-8",
        "jsx" => "application/javascript; charset=utf-8",
        "map" => "application/json; charset=utf-8",
        "yaml" | "yml" => "text/yaml; charset=utf-8",
        "toml" => "text/toml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_put_and_get() {
        let mut cache = HttpCache::new(1024 * 1024);
        let headers = HashMap::new();
        cache.put("https://example.com/api", b"hello".to_vec(), headers, 1000);

        let entry = cache.get("https://example.com/api", 2000);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().body, b"hello");
    }

    #[test]
    fn cache_expiration() {
        let mut cache = HttpCache::new(1024 * 1024);
        let mut headers = HashMap::new();
        headers.insert("cache-control".into(), "max-age=10".into());
        cache.put("https://example.com/api", b"data".to_vec(), headers, 1000);

        // Not expired
        assert!(cache.get("https://example.com/api", 5000).is_some());

        // Expired (10s = 10000ms after cached_at=1000)
        assert!(cache.get("https://example.com/api", 12000).is_none());
    }

    #[test]
    fn cache_no_store() {
        let mut cache = HttpCache::new(1024 * 1024);
        let mut headers = HashMap::new();
        headers.insert("cache-control".into(), "no-store".into());
        cache.put(
            "https://example.com/secret",
            b"data".to_vec(),
            headers,
            1000,
        );

        assert!(cache.get("https://example.com/secret", 2000).is_none());
    }

    #[test]
    fn cache_eviction() {
        let mut cache = HttpCache::new(100); // Very small cache
        let headers = HashMap::new();
        cache.put("https://a.com", vec![0u8; 50], headers.clone(), 1000);
        cache.put("https://b.com", vec![0u8; 50], headers.clone(), 2000);

        assert_eq!(cache.len(), 2);

        // This should evict one entry
        cache.put("https://c.com", vec![0u8; 50], headers, 3000);
        assert!(cache.len() <= 2);
    }

    #[test]
    fn cache_etag() {
        let mut cache = HttpCache::new(1024 * 1024);
        let mut headers = HashMap::new();
        headers.insert("ETag".into(), "\"abc123\"".into());
        cache.put("https://example.com/api", b"data".to_vec(), headers, 1000);

        assert_eq!(
            cache.get_etag("https://example.com/api"),
            Some("\"abc123\"")
        );
        assert!(cache.validate_etag("https://example.com/api", "\"abc123\""));
        assert!(!cache.validate_etag("https://example.com/api", "\"xyz\""));
    }

    #[test]
    fn cdn_url_generation() {
        let url = CdnProvider::Esm.url("react", "18.2.0");
        assert_eq!(url, "https://esm.sh/react@18.2.0");

        let url = CdnProvider::Unpkg.file_url("lodash", "4.17.21", "lodash.min.js");
        assert_eq!(url, "https://unpkg.com/lodash@4.17.21/lodash.min.js");

        let url = CdnProvider::Jsdelivr.url("vue", "3.3.0");
        assert_eq!(url, "https://cdn.jsdelivr.net/npm/vue@3.3.0");
    }

    #[test]
    fn import_analysis() {
        let source = r#"
import React from 'react';
import { useState } from 'react';
import './styles.css';
const lodash = require('lodash');
const dynamic = import('next/dynamic');
"#;
        let imports = analyze_imports(source);
        assert!(imports.contains(&"react".to_string()));
        assert!(imports.contains(&"./styles.css".to_string()));
        assert!(imports.contains(&"lodash".to_string()));
        assert!(imports.contains(&"next/dynamic".to_string()));
    }

    #[test]
    fn gzip_roundtrip() {
        let data = b"Hello, World! This is a test of gzip compression.";
        let compressed = compress_gzip(data);
        let decompressed = decompress_gzip(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn should_compress_types() {
        assert!(should_compress("text/html"));
        assert!(should_compress("application/json"));
        assert!(should_compress("application/javascript"));
        assert!(!should_compress("image/png"));
        assert!(!should_compress("application/octet-stream"));
    }

    #[test]
    fn content_type_detection() {
        assert_eq!(content_type_for_ext("html"), "text/html; charset=utf-8");
        assert_eq!(
            content_type_for_ext("js"),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(content_type_for_ext("png"), "image/png");
        assert_eq!(content_type_for_ext("wasm"), "application/wasm");
    }

    #[test]
    fn parse_max_age_values() {
        assert_eq!(parse_max_age("max-age=3600"), Some(3600));
        assert_eq!(parse_max_age("public, max-age=86400"), Some(86400));
        assert_eq!(parse_max_age("no-cache"), None);
        assert_eq!(parse_max_age("max-age=0"), Some(0));
    }

    #[test]
    fn cache_stats() {
        let mut cache = HttpCache::new(1024 * 1024);
        let headers = HashMap::new();
        cache.put("https://a.com", b"data".to_vec(), headers, 1000);

        cache.get("https://a.com", 2000); // hit
        cache.get("https://b.com", 3000); // miss

        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
    }
}
