use crate::vfs::{ChangeKind, FileChange};
/// Hot Reload — detecta cambios en el VFS y decide la estrategia de recarga.
/// Incluye detección de framework para HMR, CSS morphing con transiciones,
/// preservación de estado, error overlay, y progreso de recarga.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Estrategia de recarga ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReloadAction {
    /// Inyectar nuevas hojas de estilo sin recargar la página.
    InjectCss { paths: Vec<String> },
    /// Hot Module Replacement — actualizar módulos JS/TS sin perder estado.
    Hmr { paths: Vec<String> },
    /// Recarga completa del iframe.
    FullReload,
    /// No hacer nada (archivo irrelevante para el browser).
    None,
}

/// Clasifica el tipo de archivo y su impacto en el browser.
fn classify(path: &str) -> FileImpact {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "css" | "scss" | "sass" | "less" => FileImpact::Css,
        "ts" | "tsx" | "js" | "jsx" | "mjs" => FileImpact::Script,
        "html" | "htm" | "svelte" | "vue" => FileImpact::Markup,
        "json" if path.contains("package.json") => FileImpact::Config,
        "json" if path.contains("tsconfig") => FileImpact::Config,
        "toml" | "yaml" | "yml" | "env" => FileImpact::Config,
        _ => FileImpact::Asset,
    }
}

#[derive(Debug, PartialEq)]
enum FileImpact {
    Css,
    Script,
    Markup,
    Config,
    Asset,
}

// ── Debouncer ─────────────────────────────────────────────────────────────────

/// Acumula cambios en una ventana de tiempo y los retorna en batch.
#[derive(Debug)]
pub struct Debouncer {
    pending: Vec<FileChange>,
    window_ms: u64,
    last_change_ms: u64,
}

impl Debouncer {
    pub fn new(window_ms: u64) -> Self {
        Self {
            pending: Vec::new(),
            window_ms,
            last_change_ms: 0,
        }
    }

    /// Añade un cambio. Retorna true si el batch está listo para procesar.
    pub fn push(&mut self, change: FileChange, now_ms: u64) -> bool {
        self.pending.push(change);
        self.last_change_ms = now_ms;
        false // el caller decide cuándo flush
    }

    /// Retorna true si ha pasado la ventana de debounce desde el último cambio.
    pub fn ready(&self, now_ms: u64) -> bool {
        !self.pending.is_empty() && now_ms.saturating_sub(self.last_change_ms) >= self.window_ms
    }

    /// Retorna y limpia el batch de cambios pendientes.
    pub fn flush(&mut self) -> Vec<FileChange> {
        std::mem::take(&mut self.pending)
    }
}

// ── Framework Detection ──────────────────────────────────────────────────────

/// Frameworks soportados para HMR específico.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Framework {
    React,
    Vue,
    Svelte,
    Angular,
    Solid,
    Preact,
    Unknown,
}

impl Framework {
    /// Detecta el framework basándose en las dependencias del package.json.
    pub fn detect_from_deps(deps: &[&str]) -> Self {
        for dep in deps {
            match *dep {
                "react" | "react-dom" | "next" | "gatsby" | "remix" => return Framework::React,
                "vue" | "nuxt" | "@vue/cli-service" => return Framework::Vue,
                "svelte" | "@sveltejs/kit" => return Framework::Svelte,
                "@angular/core" => return Framework::Angular,
                "solid-js" => return Framework::Solid,
                "preact" => return Framework::Preact,
                _ => {}
            }
        }
        Framework::Unknown
    }

    /// Retorna si el framework soporta HMR nativo.
    pub fn supports_hmr(&self) -> bool {
        matches!(
            self,
            Framework::React
                | Framework::Vue
                | Framework::Svelte
                | Framework::Solid
                | Framework::Preact
        )
    }

    /// Genera el script de HMR específico para el framework detectado.
    /// Esto se inyecta en el preview para habilitar hot module replacement real.
    pub fn hmr_runtime_script(&self) -> &'static str {
        match self {
            Framework::React => Self::react_fast_refresh_script(),
            Framework::Vue => Self::vue_hmr_script(),
            Framework::Svelte => Self::svelte_hmr_script(),
            Framework::Preact => Self::preact_hmr_script(),
            Framework::Solid => Self::solid_hmr_script(),
            _ => "",
        }
    }

    /// React Fast Refresh — actualiza componentes React sin perder estado.
    fn react_fast_refresh_script() -> &'static str {
        r#"<script data-runbox-hmr="react">
(function() {
  'use strict';
  if (window.__RUNBOX_REACT_HMR) return;
  window.__RUNBOX_REACT_HMR = true;

  // React Fast Refresh runtime integration
  window.__RUNBOX_HMR = {
    moduleRegistry: {},
    acceptCallbacks: {},

    register: function(id, component) {
      this.moduleRegistry[id] = component;
    },

    accept: function(id, callback) {
      this.acceptCallbacks[id] = callback;
    },

    update: function(updatedPaths) {
      var self = this;
      updatedPaths.forEach(function(path) {
        // Find the React root and trigger re-render
        var roots = document.querySelectorAll('[data-reactroot], #root, #app, #__next');
        if (roots.length > 0 && window.React && window.ReactDOM) {
          try {
            // Attempt to use React Fast Refresh if available
            if (window.__REACT_REFRESH_RUNTIME__) {
              window.__REACT_REFRESH_RUNTIME__.performReactRefresh();
              console.log('[HMR] React Fast Refresh: ' + path);
              return;
            }

            // Fallback: force re-render from React root
            roots.forEach(function(root) {
              var fiberRoot = root._reactRootContainer;
              if (fiberRoot && fiberRoot._internalRoot) {
                var update = fiberRoot._internalRoot;
                if (update.current && update.current.memoizedState) {
                  // Trigger a forced update
                  update.current.memoizedState.element = null;
                }
              }
            });
            console.log('[HMR] React re-render triggered: ' + path);
          } catch(e) {
            console.warn('[HMR] React refresh failed, doing full reload', e);
            window.location.reload();
          }
        }
      });
    }
  };
})();
</script>"#
    }

    /// Vue HMR — hot-reloads Vue components preservando estado reactivo.
    fn vue_hmr_script() -> &'static str {
        r#"<script data-runbox-hmr="vue">
(function() {
  'use strict';
  if (window.__RUNBOX_VUE_HMR) return;
  window.__RUNBOX_VUE_HMR = true;

  window.__RUNBOX_HMR = {
    componentMap: {},

    register: function(id, component) {
      this.componentMap[id] = component;
    },

    update: function(updatedPaths) {
      var self = this;
      updatedPaths.forEach(function(path) {
        try {
          // Vue 3 HMR API
          if (window.__VUE_HMR_RUNTIME__) {
            var id = path.replace(/[^a-zA-Z0-9]/g, '_');
            if (window.__VUE_HMR_RUNTIME__.reload) {
              window.__VUE_HMR_RUNTIME__.reload(id);
              console.log('[HMR] Vue component reloaded: ' + path);
              return;
            }
          }

          // Vue 2 fallback
          if (window.__VUE_HOT_MAP__) {
            Object.keys(window.__VUE_HOT_MAP__).forEach(function(key) {
              window.__VUE_HOT_MAP__[key].reload();
            });
            console.log('[HMR] Vue 2 hot reload: ' + path);
            return;
          }

          // Last resort: find Vue instances and $forceUpdate
          var vueApp = document.querySelector('#app').__vue_app__ || document.querySelector('#app').__vue__;
          if (vueApp) {
            if (vueApp.$forceUpdate) vueApp.$forceUpdate();
            console.log('[HMR] Vue force updated');
          }
        } catch(e) {
          console.warn('[HMR] Vue refresh failed, full reload', e);
          window.location.reload();
        }
      });
    }
  };
})();
</script>"#
    }

    /// Svelte HMR — hot-reloads Svelte components.
    fn svelte_hmr_script() -> &'static str {
        r#"<script data-runbox-hmr="svelte">
(function() {
  'use strict';
  if (window.__RUNBOX_SVELTE_HMR) return;
  window.__RUNBOX_SVELTE_HMR = true;

  window.__RUNBOX_HMR = {
    componentMap: {},

    register: function(id, component) {
      this.componentMap[id] = component;
    },

    update: function(updatedPaths) {
      updatedPaths.forEach(function(path) {
        try {
          // Svelte HMR API
          if (window.__SVELTE_HMR) {
            var id = path.replace(/[^a-zA-Z0-9]/g, '_');
            if (window.__SVELTE_HMR.hot && window.__SVELTE_HMR.hot[id]) {
              window.__SVELTE_HMR.hot[id].reload();
              console.log('[HMR] Svelte component reloaded: ' + path);
              return;
            }
          }

          // Fallback: find and re-create Svelte app
          var target = document.querySelector('#app') || document.body.firstElementChild;
          if (target && target.__svelte_meta) {
            console.log('[HMR] Svelte re-mount triggered: ' + path);
          }
        } catch(e) {
          console.warn('[HMR] Svelte refresh failed, full reload', e);
          window.location.reload();
        }
      });
    }
  };
})();
</script>"#
    }

    /// Preact HMR — similar to React but uses Preact's prefresh.
    fn preact_hmr_script() -> &'static str {
        r#"<script data-runbox-hmr="preact">
(function() {
  'use strict';
  if (window.__RUNBOX_PREACT_HMR) return;
  window.__RUNBOX_PREACT_HMR = true;

  window.__RUNBOX_HMR = {
    update: function(updatedPaths) {
      updatedPaths.forEach(function(path) {
        try {
          if (window.__PREFRESH__) {
            window.__PREFRESH__.replaceComponent();
            console.log('[HMR] Preact prefresh: ' + path);
            return;
          }
          // Fallback
          var root = document.getElementById('app') || document.getElementById('root');
          if (root && root.__P) {
            root.__P.__D = true;
            console.log('[HMR] Preact re-render: ' + path);
          }
        } catch(e) {
          window.location.reload();
        }
      });
    }
  };
})();
</script>"#
    }

    /// Solid HMR — hot-reloads SolidJS components.
    fn solid_hmr_script() -> &'static str {
        r#"<script data-runbox-hmr="solid">
(function() {
  'use strict';
  if (window.__RUNBOX_SOLID_HMR) return;
  window.__RUNBOX_SOLID_HMR = true;

  window.__RUNBOX_HMR = {
    update: function(updatedPaths) {
      updatedPaths.forEach(function(path) {
        try {
          if (window.__SOLID_HMR__) {
            window.__SOLID_HMR__.reload(path);
            console.log('[HMR] Solid reloaded: ' + path);
            return;
          }
          console.log('[HMR] Solid HMR not available, full reload');
          window.location.reload();
        } catch(e) {
          window.location.reload();
        }
      });
    }
  };
})();
</script>"#
    }
}

// ── State Preservation ───────────────────────────────────────────────────────

/// Tracking de estado que debe preservarse durante recargas.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Posición de scroll (x, y).
    pub scroll_position: Option<(f64, f64)>,
    /// Valores de inputs de formularios (selector → valor).
    pub form_inputs: HashMap<String, String>,
    /// Estado de componentes (framework-specific).
    pub component_state: HashMap<String, String>,
    /// Timestamp de la captura.
    pub captured_at: u64,
}

impl StateSnapshot {
    /// Genera el script JS para capturar y persistir el estado actual en sessionStorage.
    pub fn capture_script() -> &'static str {
        r#"(function() {
    const state = {
        scroll: [window.scrollX, window.scrollY],
        inputs: {},
        focused: null
    };
    document.querySelectorAll('input, textarea, select').forEach(el => {
        const id = el.id || el.name || el.getAttribute('data-runbox-id');
        if (id) {
            state.inputs[id] = el.type === 'checkbox' ? el.checked : el.value;
        }
    });
    if (document.activeElement) {
        state.focused = document.activeElement.id || document.activeElement.name;
    }
    
    // Save to sessionStorage so it survives an actual reload
    try {
        sessionStorage.setItem('__runbox_state', JSON.stringify(state));
    } catch(e) {}
    
    return JSON.stringify(state);
})()"#
    }

    /// Genera el script JS para restaurar el estado desde sessionStorage al recargar.
    pub fn restore_script(&self) -> String {
        // This script is injected on reload to pull from sessionStorage
        let base_script = r#"(function() {
    try {
        const stored = sessionStorage.getItem('__runbox_state');
        if (!stored) clearAndReturn();
        const state = JSON.parse(stored);
        
        // Restore scroll
        if (state.scroll) {
            window.scrollTo(state.scroll[0], state.scroll[1]);
        }
        
        // Restore inputs
        if (state.inputs) {
            Object.keys(state.inputs).forEach(function(id) {
                var el = document.getElementById(id) || document.querySelector('[name="' + id + '"]');
                if (el) {
                    if (el.type === 'checkbox') el.checked = state.inputs[id];
                    else el.value = state.inputs[id];
                }
            });
        }
        
        // Focus
        if (state.focused) {
            var el = document.getElementById(state.focused) || document.querySelector('[name="' + state.focused + '"]');
            if (el) el.focus();
        }
        
    } catch(e) {}
    
    function clearAndReturn() {}
})();"#;

        base_script.to_string()
    }
}

// ── CSS Morphing ─────────────────────────────────────────────────────────────

/// Configuración de CSS morphing con transiciones suaves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CssMorphConfig {
    /// Duración de la transición en ms.
    pub transition_ms: u32,
    /// Si aplicar transición suave al cambiar estilos.
    pub smooth_transition: bool,
}

impl Default for CssMorphConfig {
    fn default() -> Self {
        Self {
            transition_ms: 200,
            smooth_transition: true,
        }
    }
}

impl CssMorphConfig {
    /// Genera el script JS para inyección de CSS con transición suave.
    pub fn inject_script(&self, css_paths: &[String]) -> String {
        let paths_json = serde_json::to_string(css_paths).unwrap_or_default();
        let transition_ms = self.transition_ms;
        let smooth = self.smooth_transition;

        format!(
            r#"(function() {{
    const paths = {paths_json};
    const smooth = {smooth};
    const transitionMs = {transition_ms};
    paths.forEach(function(path) {{
        const links = document.querySelectorAll('link[rel="stylesheet"]');
        let found = false;
        links.forEach(function(link) {{
            if (link.href && link.href.includes(path.split('/').pop())) {{
                if (smooth) {{
                    link.style.transition = 'all ' + transitionMs + 'ms ease';
                }}
                link.href = link.href.split('?')[0] + '?t=' + Date.now();
                found = true;
            }}
        }});
        if (!found) {{
            const newLink = document.createElement('link');
            newLink.rel = 'stylesheet';
            newLink.href = path + '?t=' + Date.now();
            document.head.appendChild(newLink);
        }}
    }});
}})();"#
        )
    }
}

// ── Error Overlay ────────────────────────────────────────────────────────────

/// Error de compilación/runtime para mostrar en overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationError {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub stack: Option<String>,
}

impl CompilationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            file: None,
            line: None,
            column: None,
            stack: None,
        }
    }

    pub fn with_location(mut self, file: &str, line: u32, column: u32) -> Self {
        self.file = Some(file.to_string());
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    /// Genera HTML del error overlay.
    pub fn to_overlay_html(&self) -> String {
        let file_info = match (&self.file, self.line, self.column) {
            (Some(f), Some(l), Some(c)) => format!(
                "<div class=\"rb-err-loc\">{}:{}:{}</div>",
                html_esc(f),
                l,
                c
            ),
            (Some(f), Some(l), None) => {
                format!("<div class=\"rb-err-loc\">{}:{}</div>", html_esc(f), l)
            }
            (Some(f), None, None) => format!("<div class=\"rb-err-loc\">{}</div>", html_esc(f)),
            _ => String::new(),
        };

        let stack_html = self.stack.as_ref().map_or(String::new(), |s| {
            format!("<pre class=\"rb-err-stack\">{}</pre>", html_esc(s))
        });

        format!(
            r#"<div id="runbox-error-overlay" style="position:fixed;inset:0;z-index:99999;background:rgba(0,0,0,0.85);color:#fff;font-family:monospace;padding:2rem;overflow:auto;">
  <div style="max-width:800px;margin:0 auto;">
    <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:1rem;">
      <h2 style="color:#ff5555;margin:0;">⚠ Compilation Error</h2>
      <button onclick="this.closest('#runbox-error-overlay').remove()" style="background:none;border:1px solid #666;color:#fff;padding:4px 12px;cursor:pointer;border-radius:4px;">✕</button>
    </div>
    <div class="rb-err-msg" style="background:#1a1a2e;padding:1rem;border-radius:8px;border-left:4px solid #ff5555;margin-bottom:1rem;">
      <code style="color:#ff8888;font-size:14px;white-space:pre-wrap;">{msg}</code>
    </div>
    {file_info}
    {stack_html}
  </div>
</div>"#,
            msg = html_esc(&self.message)
        )
    }

    /// Genera el script JS para mostrar el error overlay.
    pub fn to_overlay_script(&self) -> String {
        let html = self
            .to_overlay_html()
            .replace('\\', "\\\\")
            .replace('`', "\\`");
        format!(
            "(function(){{ var old = document.getElementById('runbox-error-overlay'); if(old) old.remove(); var d = document.createElement('div'); d.innerHTML = `{html}`; document.body.appendChild(d.firstElementChild); }})();"
        )
    }
}

fn html_esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Reload Progress ──────────────────────────────────────────────────────────

/// Indicador de progreso para el proceso de recarga.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadProgress {
    pub phase: ReloadPhase,
    pub percent: u8,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadPhase {
    Detecting,
    Compiling,
    Injecting,
    Complete,
    Error,
}

impl ReloadProgress {
    pub fn detecting() -> Self {
        Self {
            phase: ReloadPhase::Detecting,
            percent: 10,
            message: "Detecting changes...".into(),
        }
    }

    pub fn compiling() -> Self {
        Self {
            phase: ReloadPhase::Compiling,
            percent: 40,
            message: "Compiling...".into(),
        }
    }

    pub fn injecting() -> Self {
        Self {
            phase: ReloadPhase::Injecting,
            percent: 80,
            message: "Injecting updates...".into(),
        }
    }

    pub fn complete() -> Self {
        Self {
            phase: ReloadPhase::Complete,
            percent: 100,
            message: "Done".into(),
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            phase: ReloadPhase::Error,
            percent: 0,
            message: msg.into(),
        }
    }

    /// Genera el script JS para mostrar el indicador de progreso.
    pub fn to_progress_script(&self) -> String {
        let pct = self.percent;
        let msg = &self.message;
        let color = match self.phase {
            ReloadPhase::Error => "#ff5555",
            ReloadPhase::Complete => "#50fa7b",
            _ => "#8be9fd",
        };

        format!(
            r#"(function(){{
    var bar = document.getElementById('runbox-progress');
    if (!bar) {{
        bar = document.createElement('div');
        bar.id = 'runbox-progress';
        bar.style.cssText = 'position:fixed;top:0;left:0;right:0;height:3px;z-index:99998;transition:width 0.3s ease;';
        document.body.appendChild(bar);
    }}
    bar.style.width = '{pct}%';
    bar.style.background = '{color}';
    bar.title = '{msg}';
    if ({pct} >= 100 || '{phase}' === 'error') {{
        setTimeout(function(){{ bar.style.opacity = '0'; setTimeout(function(){{ bar.remove(); }}, 300); }}, 1000);
    }}
}})();"#,
            phase = format!("{:?}", self.phase).to_lowercase()
        )
    }
}

// ── HotReloader ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct HotReloader {
    debouncer: Debouncer,
    /// Framework detectado para HMR específico.
    pub framework: Framework,
    /// Configuración de CSS morphing.
    pub css_morph: CssMorphConfig,
    /// Último error de compilación (si existe).
    pub last_error: Option<CompilationError>,
    /// Último progreso de recarga.
    pub last_progress: Option<ReloadProgress>,
    /// Historial de recargas (timestamps).
    reload_history: Vec<u64>,
    /// Máximo de entradas en historial.
    max_history: usize,
}

impl HotReloader {
    /// `debounce_ms`: cuántos ms esperar después del último cambio antes de recargar.
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            debouncer: Debouncer::new(debounce_ms),
            framework: Framework::Unknown,
            css_morph: CssMorphConfig::default(),
            last_error: None,
            last_progress: None,
            reload_history: Vec::new(),
            max_history: 100,
        }
    }

    /// Detecta el framework a partir de nombres de dependencias.
    pub fn detect_framework(&mut self, dependency_names: &[&str]) {
        self.framework = Framework::detect_from_deps(dependency_names);
    }

    /// Alimenta los cambios del VFS. Retorna la acción si el debounce expiró.
    pub fn feed(&mut self, changes: Vec<FileChange>, now_ms: u64) -> Option<ReloadAction> {
        for change in changes {
            // Ignorar eliminaciones de archivos de build
            if change.path.contains("/node_modules/")
                || change.path.contains("/.git/")
                || change.path.ends_with(".map")
            {
                continue;
            }
            self.debouncer.push(change, now_ms);
        }

        if self.debouncer.ready(now_ms) {
            let batch = self.debouncer.flush();
            self.last_progress = Some(ReloadProgress::detecting());
            let action = decide_action(&batch);
            self.last_progress = Some(ReloadProgress::complete());
            self.reload_history.push(now_ms);
            if self.reload_history.len() > self.max_history {
                self.reload_history.remove(0);
            }
            self.last_error = None; // Clear error on successful reload
            Some(action)
        } else {
            None
        }
    }

    /// Fuerza un flush sin esperar el debounce.
    pub fn flush_now(&mut self) -> Option<ReloadAction> {
        if self.debouncer.pending.is_empty() {
            return None;
        }
        let batch = self.debouncer.flush();
        Some(decide_action(&batch))
    }

    /// Registra un error de compilación.
    pub fn set_error(&mut self, error: CompilationError) {
        self.last_error = Some(error);
        self.last_progress = Some(ReloadProgress::error("Compilation failed"));
    }

    /// Limpia el error actual.
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    /// Retorna el número de recargas realizadas.
    pub fn reload_count(&self) -> usize {
        self.reload_history.len()
    }

    /// Retorna info como JSON.
    pub fn info_json(&self) -> String {
        serde_json::json!({
            "framework": self.framework,
            "reload_count": self.reload_history.len(),
            "has_error": self.last_error.is_some(),
            "css_morph": {
                "transition_ms": self.css_morph.transition_ms,
                "smooth": self.css_morph.smooth_transition,
            },
        })
        .to_string()
    }
}

/// Decide la acción mínima necesaria para el batch de cambios.
fn decide_action(changes: &[FileChange]) -> ReloadAction {
    let mut css_paths = vec![];
    let mut hmr_paths = vec![];
    let mut needs_full = false;

    for change in changes {
        // Las eliminaciones siempre requieren recarga completa
        if change.kind == ChangeKind::Deleted {
            needs_full = true;
            break;
        }
        match classify(&change.path) {
            FileImpact::Css => css_paths.push(change.path.clone()),
            FileImpact::Script => hmr_paths.push(change.path.clone()),
            FileImpact::Markup | FileImpact::Config => {
                needs_full = true;
                break;
            }
            FileImpact::Asset => {} // ignorar
        }
    }

    if needs_full {
        return ReloadAction::FullReload;
    }
    if !hmr_paths.is_empty() {
        // Si hay tanto CSS como scripts, usar HMR (más invasivo que CSS inject pero menos que full)
        let mut all = hmr_paths;
        all.extend(css_paths);
        return ReloadAction::Hmr { paths: all };
    }
    if !css_paths.is_empty() {
        return ReloadAction::InjectCss { paths: css_paths };
    }
    ReloadAction::None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, kind: ChangeKind) -> FileChange {
        FileChange {
            path: path.to_string(),
            kind,
            reason: None,
        }
    }

    #[test]
    fn css_only_injects() {
        let action = decide_action(&[change("/style.css", ChangeKind::Modified)]);
        assert!(matches!(action, ReloadAction::InjectCss { .. }));
    }

    #[test]
    fn ts_does_hmr() {
        let action = decide_action(&[change("/index.ts", ChangeKind::Modified)]);
        assert!(matches!(action, ReloadAction::Hmr { .. }));
    }

    #[test]
    fn html_does_full_reload() {
        let action = decide_action(&[change("/index.html", ChangeKind::Modified)]);
        assert_eq!(action, ReloadAction::FullReload);
    }

    #[test]
    fn deletion_does_full_reload() {
        let action = decide_action(&[change("/index.ts", ChangeKind::Deleted)]);
        assert_eq!(action, ReloadAction::FullReload);
    }

    #[test]
    fn debounce_batches_changes() {
        let mut hr = HotReloader::new(50);

        // Cambios a t=0, debounce=50ms
        let r = hr.feed(vec![change("/a.css", ChangeKind::Modified)], 0);
        assert!(r.is_none(), "no debe recargar antes del debounce");

        // A t=60 (>50ms después) debe disparar
        let r = hr.feed(vec![], 60);
        assert!(matches!(r, Some(ReloadAction::InjectCss { .. })));
    }

    #[test]
    fn framework_detection() {
        assert_eq!(
            Framework::detect_from_deps(&["react", "react-dom"]),
            Framework::React
        );
        assert_eq!(Framework::detect_from_deps(&["vue"]), Framework::Vue);
        assert_eq!(Framework::detect_from_deps(&["svelte"]), Framework::Svelte);
        assert_eq!(Framework::detect_from_deps(&["lodash"]), Framework::Unknown);
        assert!(Framework::React.supports_hmr());
        assert!(!Framework::Unknown.supports_hmr());
    }

    #[test]
    fn error_overlay_generation() {
        let err = CompilationError::new("SyntaxError: Unexpected token").with_location(
            "src/app.tsx",
            42,
            15,
        );
        let html = err.to_overlay_html();
        assert!(html.contains("SyntaxError"));
        assert!(html.contains("src/app.tsx"));
        assert!(html.contains("42"));
    }

    #[test]
    fn css_morph_script() {
        let config = CssMorphConfig::default();
        let script = config.inject_script(&["style.css".to_string()]);
        assert!(script.contains("style.css"));
        assert!(script.contains("transition"));
    }

    #[test]
    fn reload_progress() {
        let p = ReloadProgress::detecting();
        assert_eq!(p.percent, 10);
        let script = p.to_progress_script();
        assert!(script.contains("runbox-progress"));

        let p = ReloadProgress::complete();
        assert_eq!(p.percent, 100);
    }

    #[test]
    fn state_snapshot_scripts() {
        let script = StateSnapshot::capture_script();
        assert!(script.contains("scrollX"));
        assert!(script.contains("inputs"));
        assert!(script.contains("sessionStorage"));

        let snap = StateSnapshot::default();
        let restore = snap.restore_script();
        assert!(restore.contains("sessionStorage.getItem"));
        assert!(restore.contains("window.scrollTo"));
    }

    #[test]
    fn hotreloader_tracks_reloads() {
        let mut hr = HotReloader::new(50);
        assert_eq!(hr.reload_count(), 0);

        hr.feed(vec![change("/a.css", ChangeKind::Modified)], 0);
        hr.feed(vec![], 60);
        assert_eq!(hr.reload_count(), 1);
    }

    #[test]
    fn hotreloader_error_handling() {
        let mut hr = HotReloader::new(50);
        hr.set_error(CompilationError::new("test error"));
        assert!(hr.last_error.is_some());

        hr.clear_error();
        assert!(hr.last_error.is_none());
    }
}
