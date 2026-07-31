/// Optimización WASM — herramientas de optimización y profiling para WebAssembly.
///
/// Provee:
/// - Tracking de tamaño de módulos WASM
/// - Metadatos de cache de compilación
/// - Instrumentación de performance/profiling
/// - Tracking de uso de memoria
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── WASM Module Metrics ─────────────────────────────────────────────────────

/// Métricas de un módulo WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmModuleMetrics {
    /// Nombre/identificador del módulo.
    pub name: String,
    /// Tamaño del módulo en bytes (sin optimizar).
    pub raw_size: usize,
    /// Tamaño optimizado (post wasm-opt) si disponible.
    pub optimized_size: Option<usize>,
    /// Ratio de compresión (optimized/raw).
    pub compression_ratio: Option<f64>,
    /// Tiempo de compilación en ms.
    pub compile_time_ms: Option<u64>,
    /// Tiempo de instanciación en ms.
    pub instantiate_time_ms: Option<u64>,
    /// Número de funciones exportadas.
    pub export_count: usize,
    /// Número de funciones importadas.
    pub import_count: usize,
    /// Uso de memoria (páginas de 64KB).
    pub memory_pages: Option<u32>,
}

impl WasmModuleMetrics {
    pub fn new(name: impl Into<String>, raw_size: usize) -> Self {
        Self {
            name: name.into(),
            raw_size,
            optimized_size: None,
            compression_ratio: None,
            compile_time_ms: None,
            instantiate_time_ms: None,
            export_count: 0,
            import_count: 0,
            memory_pages: None,
        }
    }

    /// Registra el tamaño optimizado y calcula el ratio.
    pub fn set_optimized_size(&mut self, size: usize) {
        self.optimized_size = Some(size);
        if self.raw_size > 0 {
            self.compression_ratio = Some(size as f64 / self.raw_size as f64);
        }
    }

    /// Retorna el ahorro en bytes por la optimización.
    pub fn bytes_saved(&self) -> usize {
        match self.optimized_size {
            Some(opt) if opt < self.raw_size => self.raw_size - opt,
            _ => 0,
        }
    }

    /// Retorna el porcentaje de reducción.
    pub fn reduction_percent(&self) -> f64 {
        match self.optimized_size {
            Some(opt) if self.raw_size > 0 => (1.0 - (opt as f64 / self.raw_size as f64)) * 100.0,
            _ => 0.0,
        }
    }
}

// ── Compilation Cache ───────────────────────────────────────────────────────

/// Metadatos de cache de compilación WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationCacheEntry {
    /// Hash del módulo fuente.
    pub source_hash: String,
    /// Timestamp de compilación.
    pub compiled_at: u64,
    /// Tamaño del módulo compilado.
    pub compiled_size: usize,
    /// Tiempo de compilación en ms.
    pub compile_time_ms: u64,
    /// Target (web, node, etc).
    pub target: String,
    /// Si el módulo está en el cache de IndexedDB.
    pub in_idb: bool,
}

/// Cache de compilaciones WASM.
#[derive(Debug)]
pub struct CompilationCache {
    entries: HashMap<String, CompilationCacheEntry>,
    max_entries: usize,
}

impl CompilationCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries,
        }
    }

    /// Registra una compilación.
    pub fn record(
        &mut self,
        module_name: &str,
        source_hash: String,
        compiled_size: usize,
        compile_time_ms: u64,
        target: &str,
        timestamp: u64,
    ) {
        // Evict oldest if at capacity
        if self.entries.len() >= self.max_entries && !self.entries.contains_key(module_name) {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.compiled_at)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest {
                self.entries.remove(&key);
            }
        }

        self.entries.insert(
            module_name.to_string(),
            CompilationCacheEntry {
                source_hash,
                compiled_at: timestamp,
                compiled_size,
                compile_time_ms,
                target: target.to_string(),
                in_idb: false,
            },
        );
    }

    /// Verifica si un módulo tiene una compilación cacheada válida.
    pub fn is_cached(&self, module_name: &str, source_hash: &str) -> bool {
        self.entries
            .get(module_name)
            .is_some_and(|e| e.source_hash == source_hash)
    }

    /// Marca un módulo como almacenado en IndexedDB.
    pub fn mark_in_idb(&mut self, module_name: &str) {
        if let Some(entry) = self.entries.get_mut(module_name) {
            entry.in_idb = true;
        }
    }

    /// Retorna las métricas de compilación.
    pub fn get(&self, module_name: &str) -> Option<&CompilationCacheEntry> {
        self.entries.get(module_name)
    }

    /// Retorna todas las entradas.
    pub fn entries(&self) -> &HashMap<String, CompilationCacheEntry> {
        &self.entries
    }

    /// Limpia el cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for CompilationCache {
    fn default() -> Self {
        Self::new(100)
    }
}

// ── Performance Profiler ────────────────────────────────────────────────────

/// Tipo de evento de profiling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileEventKind {
    /// Compilación de módulo WASM.
    Compile,
    /// Instanciación de módulo.
    Instantiate,
    /// Llamada a función.
    FunctionCall,
    /// Transferencia de datos JS↔WASM.
    DataTransfer,
    /// Operación de VFS.
    VfsOperation,
    /// Operación de red.
    NetworkRequest,
    /// Garbage collection / cleanup.
    Gc,
    /// Custom event.
    Custom,
}

/// Evento de profiling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileEvent {
    pub id: u64,
    pub kind: ProfileEventKind,
    pub name: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub metadata: HashMap<String, String>,
}

/// Profiler de performance para operaciones WASM.
#[derive(Debug)]
pub struct WasmProfiler {
    events: Vec<ProfileEvent>,
    next_id: u64,
    enabled: bool,
    max_events: usize,
    /// Eventos activos (en progreso).
    active: HashMap<u64, ProfileEvent>,
}

impl WasmProfiler {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Vec::new(),
            next_id: 1,
            enabled: false,
            max_events,
            active: HashMap::new(),
        }
    }

    /// Habilita/deshabilita el profiler.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Inicia un evento de profiling. Retorna el ID para finalizar después.
    pub fn start(&mut self, kind: ProfileEventKind, name: impl Into<String>, start_ms: u64) -> u64 {
        if !self.enabled {
            return 0;
        }

        let id = self.next_id;
        self.next_id += 1;

        self.active.insert(
            id,
            ProfileEvent {
                id,
                kind,
                name: name.into(),
                start_ms,
                duration_ms: 0,
                metadata: HashMap::new(),
            },
        );

        id
    }

    /// Finaliza un evento de profiling.
    pub fn end(&mut self, id: u64, end_ms: u64) {
        if !self.enabled || id == 0 {
            return;
        }

        if let Some(mut event) = self.active.remove(&id) {
            event.duration_ms = end_ms.saturating_sub(event.start_ms);

            self.events.push(event);

            // Trim if too many events
            if self.events.len() > self.max_events {
                let drain = self.events.len() - self.max_events;
                self.events.drain(..drain);
            }
        }
    }

    /// Añade metadata a un evento activo.
    pub fn add_metadata(&mut self, id: u64, key: impl Into<String>, value: impl Into<String>) {
        if let Some(event) = self.active.get_mut(&id) {
            event.metadata.insert(key.into(), value.into());
        }
    }

    /// Registra un evento completo de una vez (ya tiene start y duration).
    pub fn record(
        &mut self,
        kind: ProfileEventKind,
        name: impl Into<String>,
        start_ms: u64,
        duration_ms: u64,
    ) -> u64 {
        if !self.enabled {
            return 0;
        }

        let id = self.next_id;
        self.next_id += 1;

        self.events.push(ProfileEvent {
            id,
            kind,
            name: name.into(),
            start_ms,
            duration_ms,
            metadata: HashMap::new(),
        });

        if self.events.len() > self.max_events {
            let drain = self.events.len() - self.max_events;
            self.events.drain(..drain);
        }

        id
    }

    /// Retorna todos los eventos completados.
    pub fn events(&self) -> &[ProfileEvent] {
        &self.events
    }

    /// Retorna eventos filtrados por tipo.
    pub fn events_by_kind(&self, kind: &ProfileEventKind) -> Vec<&ProfileEvent> {
        self.events.iter().filter(|e| &e.kind == kind).collect()
    }

    /// Calcula estadísticas agregadas.
    pub fn summary(&self) -> HashMap<String, ProfileSummary> {
        let mut summaries: HashMap<String, Vec<u64>> = HashMap::new();

        for event in &self.events {
            let key = format!("{:?}", event.kind);
            summaries.entry(key).or_default().push(event.duration_ms);
        }

        summaries
            .into_iter()
            .map(|(kind, durations)| {
                let count = durations.len() as u64;
                let total: u64 = durations.iter().sum();
                let avg = total.checked_div(count).unwrap_or(0);
                let max = durations.iter().copied().max().unwrap_or(0);
                let min = durations.iter().copied().min().unwrap_or(0);
                (
                    kind,
                    ProfileSummary {
                        count,
                        total_ms: total,
                        avg_ms: avg,
                        min_ms: min,
                        max_ms: max,
                    },
                )
            })
            .collect()
    }

    /// Limpia todos los eventos.
    pub fn clear(&mut self) {
        self.events.clear();
        self.active.clear();
    }

    /// Serializa los eventos a JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.events).unwrap_or_default()
    }

    /// Número de eventos completados.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl Default for WasmProfiler {
    fn default() -> Self {
        Self::new(10_000)
    }
}

/// Resumen estadístico de un tipo de evento.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub count: u64,
    pub total_ms: u64,
    pub avg_ms: u64,
    pub min_ms: u64,
    pub max_ms: u64,
}

// ── Memory Tracker ──────────────────────────────────────────────────────────

/// Tracker de uso de memoria WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUsage {
    /// Memoria total de WASM (bytes).
    pub wasm_memory: usize,
    /// Tamaño del VFS (bytes).
    pub vfs_size: usize,
    /// Tamaño del cache HTTP (bytes).
    pub cache_size: usize,
    /// Entradas de consola (bytes estimados).
    pub console_size: usize,
    /// Procesos activos.
    pub active_processes: usize,
    /// Timestamp de la medición.
    pub timestamp_ms: u64,
}

/// Historial de uso de memoria.
#[derive(Debug)]
pub struct MemoryTracker {
    snapshots: Vec<MemoryUsage>,
    max_snapshots: usize,
}

impl MemoryTracker {
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            snapshots: Vec::new(),
            max_snapshots,
        }
    }

    /// Registra un snapshot de uso de memoria.
    pub fn record(&mut self, usage: MemoryUsage) {
        self.snapshots.push(usage);
        if self.snapshots.len() > self.max_snapshots {
            self.snapshots.remove(0);
        }
    }

    /// Retorna el último snapshot.
    pub fn latest(&self) -> Option<&MemoryUsage> {
        self.snapshots.last()
    }

    /// Retorna todos los snapshots.
    pub fn history(&self) -> &[MemoryUsage] {
        &self.snapshots
    }

    /// Retorna el total de memoria usada en el último snapshot.
    pub fn total_bytes(&self) -> usize {
        self.latest().map_or(0, |u| {
            u.wasm_memory + u.vfs_size + u.cache_size + u.console_size
        })
    }

    /// Limpia el historial.
    pub fn clear(&mut self) {
        self.snapshots.clear();
    }

    /// Serializa el historial a JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.snapshots).unwrap_or_default()
    }
}

impl Default for MemoryTracker {
    fn default() -> Self {
        Self::new(100)
    }
}

// ── WASM Optimization Manager ───────────────────────────────────────────────

/// Manager central de optimización WASM.
#[derive(Debug)]
pub struct WasmOptManager {
    pub modules: HashMap<String, WasmModuleMetrics>,
    pub compilation_cache: CompilationCache,
    pub profiler: WasmProfiler,
    pub memory: MemoryTracker,
}

impl WasmOptManager {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            compilation_cache: CompilationCache::default(),
            profiler: WasmProfiler::default(),
            memory: MemoryTracker::default(),
        }
    }

    /// Registra un módulo WASM.
    pub fn register_module(&mut self, name: &str, raw_size: usize) {
        self.modules
            .insert(name.to_string(), WasmModuleMetrics::new(name, raw_size));
    }

    /// Retorna métricas de un módulo.
    pub fn get_module(&self, name: &str) -> Option<&WasmModuleMetrics> {
        self.modules.get(name)
    }

    /// Retorna un resumen de todos los módulos.
    pub fn modules_summary(&self) -> serde_json::Value {
        let total_raw: usize = self.modules.values().map(|m| m.raw_size).sum();
        let total_opt: usize = self.modules.values().filter_map(|m| m.optimized_size).sum();
        let total_saved: usize = self.modules.values().map(|m| m.bytes_saved()).sum();

        serde_json::json!({
            "module_count": self.modules.len(),
            "total_raw_size": total_raw,
            "total_optimized_size": total_opt,
            "total_bytes_saved": total_saved,
            "modules": self.modules.values()
                .map(|m| serde_json::json!({
                    "name": m.name,
                    "raw_size": m.raw_size,
                    "optimized_size": m.optimized_size,
                    "reduction_percent": format!("{:.1}%", m.reduction_percent()),
                    "export_count": m.export_count,
                    "import_count": m.import_count,
                }))
                .collect::<Vec<_>>(),
        })
    }

    /// Retorna estadísticas completas como JSON.
    pub fn stats_json(&self) -> String {
        serde_json::json!({
            "modules": self.modules_summary(),
            "compilation_cache": {
                "entries": self.compilation_cache.len(),
            },
            "profiler": {
                "enabled": self.profiler.is_enabled(),
                "events": self.profiler.len(),
                "summary": self.profiler.summary(),
            },
            "memory": {
                "total_bytes": self.memory.total_bytes(),
                "snapshots": self.memory.history().len(),
            },
        })
        .to_string()
    }
}

impl Default for WasmOptManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_metrics() {
        let mut metrics = WasmModuleMetrics::new("test", 1000);
        assert_eq!(metrics.bytes_saved(), 0);

        metrics.set_optimized_size(700);
        assert_eq!(metrics.bytes_saved(), 300);
        assert!((metrics.reduction_percent() - 30.0).abs() < 0.1);
    }

    #[test]
    fn compilation_cache() {
        let mut cache = CompilationCache::new(10);
        cache.record("app", "hash123".into(), 5000, 100, "web", 1000);

        assert!(cache.is_cached("app", "hash123"));
        assert!(!cache.is_cached("app", "hash456"));
        assert!(!cache.is_cached("other", "hash123"));
    }

    #[test]
    fn compilation_cache_eviction() {
        let mut cache = CompilationCache::new(2);
        cache.record("a", "h1".into(), 100, 10, "web", 1000);
        cache.record("b", "h2".into(), 200, 20, "web", 2000);
        cache.record("c", "h3".into(), 300, 30, "web", 3000);

        assert_eq!(cache.len(), 2);
        // "a" should have been evicted (oldest)
        assert!(!cache.is_cached("a", "h1"));
    }

    #[test]
    fn profiler_basic() {
        let mut profiler = WasmProfiler::new(100);
        profiler.set_enabled(true);

        let id = profiler.start(ProfileEventKind::Compile, "test_module", 1000);
        assert!(id > 0);

        profiler.end(id, 1500);
        assert_eq!(profiler.len(), 1);
        assert_eq!(profiler.events()[0].duration_ms, 500);
    }

    #[test]
    fn profiler_disabled() {
        let mut profiler = WasmProfiler::new(100);
        // Not enabled by default
        let id = profiler.start(ProfileEventKind::Compile, "test", 1000);
        assert_eq!(id, 0);
        assert_eq!(profiler.len(), 0);
    }

    #[test]
    fn profiler_summary() {
        let mut profiler = WasmProfiler::new(100);
        profiler.set_enabled(true);

        profiler.record(ProfileEventKind::Compile, "a", 0, 100);
        profiler.record(ProfileEventKind::Compile, "b", 100, 200);
        profiler.record(ProfileEventKind::FunctionCall, "c", 200, 50);

        let summary = profiler.summary();
        let compile = summary.get("Compile").unwrap();
        assert_eq!(compile.count, 2);
        assert_eq!(compile.total_ms, 300);
    }

    #[test]
    fn memory_tracker() {
        let mut tracker = MemoryTracker::new(3);
        for i in 0..5 {
            tracker.record(MemoryUsage {
                wasm_memory: i * 1000,
                vfs_size: 500,
                cache_size: 200,
                console_size: 100,
                active_processes: 1,
                timestamp_ms: i as u64 * 1000,
            });
        }
        assert_eq!(tracker.history().len(), 3); // max 3
        assert_eq!(tracker.latest().unwrap().wasm_memory, 4000);
    }

    #[test]
    fn wasm_opt_manager() {
        let mut manager = WasmOptManager::new();
        manager.register_module("runbox", 500_000);

        if let Some(m) = manager.modules.get_mut("runbox") {
            m.set_optimized_size(350_000);
            m.export_count = 40;
        }

        let summary = manager.modules_summary();
        assert_eq!(summary["module_count"], 1);
        assert_eq!(summary["total_raw_size"], 500_000);
    }

    #[test]
    fn profiler_record_direct() {
        let mut profiler = WasmProfiler::new(100);
        profiler.set_enabled(true);

        profiler.record(ProfileEventKind::DataTransfer, "transfer_1", 0, 150);
        assert_eq!(profiler.len(), 1);
        assert_eq!(profiler.events()[0].duration_ms, 150);
    }

    #[test]
    fn idb_cache_marking() {
        let mut cache = CompilationCache::new(10);
        cache.record("module_a", "hash1".into(), 1000, 50, "web", 1000);
        assert!(!cache.get("module_a").unwrap().in_idb);

        cache.mark_in_idb("module_a");
        assert!(cache.get("module_a").unwrap().in_idb);
    }
}
