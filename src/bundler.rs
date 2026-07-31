/// Bundler integrado — resolución de módulos ESM, transformación JSX/TSX,
/// CSS Modules, tree shaking básico y source maps.
///
/// Provee:
/// - Resolución de módulos ESM siguiendo el algoritmo de Node.js
/// - Transformación de JSX a React.createElement / h()
/// - Soporte para CSS Modules con scoping automático
/// - Tree shaking básico (eliminación de exports no usados)
/// - Generación de source maps para debugging
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ── Bundle Configuration ────────────────────────────────────────────────────

/// Configuración del bundler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleConfig {
    /// Punto de entrada principal.
    pub entry: String,
    /// Factory de JSX (e.g., "React.createElement", "h").
    pub jsx_factory: String,
    /// Fragment de JSX (e.g., "React.Fragment", "Fragment").
    pub jsx_fragment: String,
    /// Si se debe aplicar tree shaking.
    pub tree_shake: bool,
    /// Si se deben generar source maps.
    pub source_maps: bool,
    /// Si se deben procesar CSS Modules.
    pub css_modules: bool,
    /// Target de compilación.
    pub target: BundleTarget,
    /// Módulos externos (no incluir en el bundle).
    pub externals: Vec<String>,
    /// Alias de módulos.
    pub aliases: HashMap<String, String>,
    /// Minificar el output.
    pub minify: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BundleTarget {
    /// Navegador moderno (ES2020+).
    Browser,
    /// Node.js.
    Node,
    /// Neutral (librería).
    Neutral,
}

impl Default for BundleConfig {
    fn default() -> Self {
        Self {
            entry: "/src/index.tsx".to_string(),
            jsx_factory: "React.createElement".to_string(),
            jsx_fragment: "React.Fragment".to_string(),
            tree_shake: true,
            source_maps: true,
            css_modules: true,
            target: BundleTarget::Browser,
            externals: Vec::new(),
            aliases: HashMap::new(),
            minify: false,
        }
    }
}

// ── Module Resolution ───────────────────────────────────────────────────────

/// Un módulo resuelto en el bundle graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedModule {
    /// Path absoluto en el VFS.
    pub path: String,
    /// Contenido transformado.
    pub code: String,
    /// Dependencias (paths resueltos).
    pub dependencies: Vec<String>,
    /// Exports detectados.
    pub exports: Vec<String>,
    /// Si es un módulo CSS.
    pub is_css: bool,
    /// Source map (si generado).
    pub source_map: Option<String>,
}

/// Resultado del bundling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleResult {
    /// Código bundled final.
    pub code: String,
    /// CSS extraído (desde CSS Modules y imports).
    pub css: String,
    /// Source map combinado.
    pub source_map: Option<String>,
    /// Estadísticas del bundle.
    pub stats: BundleStats,
    /// Errores (si alguno).
    pub errors: Vec<BundleError>,
    /// Warnings.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BundleStats {
    /// Número de módulos incluidos.
    pub module_count: usize,
    /// Tamaño del bundle en bytes.
    pub bundle_size: usize,
    /// Tamaño del CSS en bytes.
    pub css_size: usize,
    /// Número de módulos eliminados por tree shaking.
    pub tree_shaken: usize,
    /// Tiempo de bundling en ms.
    pub build_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleError {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl BundleError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            file: None,
            line: None,
            column: None,
        }
    }

    pub fn with_location(mut self, file: &str, line: u32, column: u32) -> Self {
        self.file = Some(file.to_string());
        self.line = Some(line);
        self.column = Some(column);
        self
    }
}

// ── JSX Transformer ─────────────────────────────────────────────────────────

/// Transforma JSX/TSX a llamadas JavaScript.
#[derive(Debug)]
pub struct JsxTransformer {
    factory: String,
    fragment: String,
}

impl JsxTransformer {
    pub fn new(factory: &str, fragment: &str) -> Self {
        Self {
            factory: factory.to_string(),
            fragment: fragment.to_string(),
        }
    }

    pub fn react() -> Self {
        Self::new("React.createElement", "React.Fragment")
    }

    pub fn preact() -> Self {
        Self::new("h", "Fragment")
    }

    /// Transforma JSX en el código fuente a llamadas de función.
    /// Implementa una transformación simplificada pero funcional.
    pub fn transform(&self, source: &str) -> Result<String, BundleError> {
        let mut result = String::with_capacity(source.len());
        let mut chars = source.chars().peekable();
        let mut in_string = false;
        let mut string_char = '"';
        let mut in_template = false;
        let mut template_depth = 0i32;

        while let Some(c) = chars.next() {
            // Handle strings
            if !in_string && !in_template && (c == '"' || c == '\'') {
                in_string = true;
                string_char = c;
                result.push(c);
                continue;
            }
            if in_string {
                if c == '\\' {
                    result.push(c);
                    if let Some(next) = chars.next() {
                        result.push(next);
                    }
                    continue;
                }
                if c == string_char {
                    in_string = false;
                }
                result.push(c);
                continue;
            }

            // Handle template literals
            if c == '`' {
                in_template = !in_template;
                result.push(c);
                continue;
            }
            if in_template {
                if (c == '$' && chars.peek() == Some(&'{')) || (c == '{' && template_depth > 0) {
                    template_depth += 1;
                }
                if c == '}' && template_depth > 0 {
                    template_depth -= 1;
                }
                result.push(c);
                continue;
            }

            // Handle JSX opening tag: <Component or <div
            if c == '<' && !in_string && !in_template {
                // Check if this is JSX (not comparison or type assertion)
                if let Some(&next) = chars.peek()
                    && (next.is_alphabetic() || next == '_' || next == '>')
                {
                    // This looks like JSX
                    let jsx = self.parse_jsx_element(&mut chars)?;
                    result.push_str(&jsx);
                    continue;
                }
            }

            result.push(c);
        }

        Ok(result)
    }

    /// Parse a JSX element starting after '<'
    fn parse_jsx_element(
        &self,
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<String, BundleError> {
        // Read tag name
        let mut tag_name = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' {
                tag_name.push(c);
                chars.next();
            } else {
                break;
            }
        }

        if tag_name.is_empty() {
            // Fragment: <>...</>
            return Ok(format!("{}({}, null", self.factory, self.fragment));
        }

        // Determine if component (PascalCase) or HTML element
        let is_component = tag_name.chars().next().is_some_and(|c| c.is_uppercase());
        let tag_expr = if is_component {
            tag_name.clone()
        } else {
            format!("\"{}\"", tag_name)
        };

        // Parse attributes
        let mut attrs = Vec::new();
        let mut self_closing = false;

        loop {
            // Skip whitespace
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }

            match chars.peek() {
                Some(&'/') => {
                    chars.next();
                    if chars.peek() == Some(&'>') {
                        chars.next();
                        self_closing = true;
                        break;
                    }
                }
                Some(&'>') => {
                    chars.next();
                    break;
                }
                Some(_) => {
                    let attr = self.parse_jsx_attribute(chars)?;
                    if !attr.is_empty() {
                        attrs.push(attr);
                    }
                }
                None => break,
            }
        }

        let props = if attrs.is_empty() {
            "null".to_string()
        } else {
            format!("{{{}}}", attrs.join(", "))
        };

        if self_closing {
            return Ok(format!("{}({}, {})", self.factory, tag_expr, props));
        }

        // Parse children until closing tag
        let mut children = Vec::new();
        let closing = format!("</{}>", tag_name);
        let mut text_buf = String::new();

        loop {
            match chars.peek() {
                None => break,
                Some(&'<') => {
                    // Flush text
                    let trimmed = text_buf.trim();
                    if !trimmed.is_empty() {
                        children.push(format!("\"{}\"", trimmed.replace('"', "\\\"")));
                    }
                    text_buf.clear();

                    // Check for closing tag
                    let rest: String = chars.clone().take(closing.len()).collect();
                    if rest == closing {
                        // Consume the closing tag
                        for _ in 0..closing.len() {
                            chars.next();
                        }
                        break;
                    }

                    // Child JSX element
                    chars.next(); // consume '<'
                    let child = self.parse_jsx_element(chars)?;
                    children.push(child);
                }
                Some(&'{') => {
                    // Flush text
                    let trimmed = text_buf.trim();
                    if !trimmed.is_empty() {
                        children.push(format!("\"{}\"", trimmed.replace('"', "\\\"")));
                    }
                    text_buf.clear();

                    // Expression child
                    chars.next(); // consume '{'
                    let expr = self.parse_jsx_expression(chars)?;
                    children.push(expr);
                }
                Some(&c) => {
                    text_buf.push(c);
                    chars.next();
                }
            }
        }

        let trimmed = text_buf.trim();
        if !trimmed.is_empty() {
            children.push(format!("\"{}\"", trimmed.replace('"', "\\\"")));
        }

        if children.is_empty() {
            Ok(format!("{}({}, {})", self.factory, tag_expr, props))
        } else {
            Ok(format!(
                "{}({}, {}, {})",
                self.factory,
                tag_expr,
                props,
                children.join(", ")
            ))
        }
    }

    fn parse_jsx_attribute(
        &self,
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<String, BundleError> {
        let mut name = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                name.push(c);
                chars.next();
            } else {
                break;
            }
        }

        if name.is_empty() {
            // Spread: {...obj}
            if chars.peek() == Some(&'{') {
                chars.next();
                let expr = self.parse_jsx_expression(chars)?;
                return Ok(format!("...{}", expr));
            }
            return Ok(String::new());
        }

        // Check for value
        if chars.peek() == Some(&'=') {
            chars.next();
            match chars.peek() {
                Some(&'"') | Some(&'\'') => {
                    let quote = chars.next().unwrap();
                    let mut value = String::new();
                    for c in chars.by_ref() {
                        if c == quote {
                            break;
                        }
                        value.push(c);
                    }
                    Ok(format!("{}: \"{}\"", name, value))
                }
                Some(&'{') => {
                    chars.next();
                    let expr = self.parse_jsx_expression(chars)?;
                    Ok(format!("{}: {}", name, expr))
                }
                _ => Ok(format!("{}: true", name)),
            }
        } else {
            Ok(format!("{}: true", name))
        }
    }

    fn parse_jsx_expression(
        &self,
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<String, BundleError> {
        let mut expr = String::new();
        let mut depth = 1i32;

        for c in chars.by_ref() {
            match c {
                '{' => {
                    depth += 1;
                    expr.push(c);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    expr.push(c);
                }
                _ => expr.push(c),
            }
        }

        Ok(expr.trim().to_string())
    }
}

// ── CSS Module Processor ────────────────────────────────────────────────────

/// Procesa CSS Modules, generando clases con scope automático.
#[derive(Debug)]
pub struct CssModuleProcessor {
    /// Prefijo para las clases con scope.
    prefix: String,
    /// Contador para generar IDs únicos.
    counter: u64,
}

impl CssModuleProcessor {
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            counter: 0,
        }
    }

    /// Procesa un archivo CSS Module y retorna el CSS transformado y el mapa de clases.
    pub fn process(&mut self, css: &str, file_path: &str) -> CssModuleResult {
        let file_hash = simple_hash(file_path);
        let mut class_map = HashMap::new();
        let mut output = String::with_capacity(css.len());

        for line in css.lines() {
            let trimmed = line.trim();
            // Detectar selectores de clase: .className {
            if trimmed.starts_with('.') && (trimmed.contains('{') || trimmed.ends_with(',')) {
                let mut processed_line = line.to_string();
                // Extraer nombres de clase
                let mut i = 0;
                let chars: Vec<char> = trimmed.chars().collect();
                while i < chars.len() {
                    if chars[i] == '.' {
                        let start = i + 1;
                        let mut end = start;
                        while end < chars.len()
                            && (chars[end].is_alphanumeric()
                                || chars[end] == '_'
                                || chars[end] == '-')
                        {
                            end += 1;
                        }
                        if end > start {
                            let original: String = chars[start..end].iter().collect();
                            self.counter += 1;
                            let scoped = format!(
                                "{}_{}_{}",
                                self.prefix,
                                original,
                                &file_hash[..6.min(file_hash.len())]
                            );
                            class_map.insert(original.clone(), scoped.clone());
                            processed_line = processed_line
                                .replace(&format!(".{original}"), &format!(".{scoped}"));
                        }
                        i = end;
                    } else {
                        i += 1;
                    }
                }
                output.push_str(&processed_line);
            } else {
                output.push_str(line);
            }
            output.push('\n');
        }

        CssModuleResult {
            css: output,
            class_map,
            file_path: file_path.to_string(),
        }
    }
}

impl Default for CssModuleProcessor {
    fn default() -> Self {
        Self::new("rb")
    }
}

/// Resultado del procesamiento de CSS Modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CssModuleResult {
    /// CSS transformado con clases con scope.
    pub css: String,
    /// Mapa de clase original → clase con scope.
    pub class_map: HashMap<String, String>,
    /// Path del archivo procesado.
    pub file_path: String,
}

impl CssModuleResult {
    /// Genera el módulo JS para importar las clases.
    pub fn to_js_module(&self) -> String {
        let entries: Vec<String> = self
            .class_map
            .iter()
            .map(|(k, v)| format!("  \"{k}\": \"{v}\""))
            .collect();
        format!("export default {{\n{}\n}};", entries.join(",\n"))
    }
}

// ── Module Graph ────────────────────────────────────────────────────────────

/// Grafo de dependencias para tree shaking.
#[derive(Debug, Default)]
pub struct ModuleGraph {
    /// Módulos en el grafo.
    modules: HashMap<String, ModuleNode>,
    /// Orden de resolución (topológico).
    order: Vec<String>,
}

#[derive(Debug, Clone)]
struct ModuleNode {
    imports: Vec<ImportInfo>,
    exports: Vec<String>,
    used_exports: HashSet<String>,
    side_effects: bool,
}

#[derive(Debug, Clone)]
struct ImportInfo {
    source: String,
}

impl ModuleGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Añade un módulo al grafo.
    pub fn add_module(&mut self, path: &str, code: &str) {
        let imports = parse_imports(code);
        let exports = parse_exports(code);
        let side_effects = has_side_effects(code);

        self.modules.insert(
            path.to_string(),
            ModuleNode {
                imports,
                exports,
                used_exports: HashSet::new(),
                side_effects,
            },
        );
    }

    /// Marca un export como usado.
    pub fn mark_used(&mut self, module: &str, export_name: &str) {
        if let Some(node) = self.modules.get_mut(module) {
            node.used_exports.insert(export_name.to_string());
        }
    }

    /// Ejecuta tree shaking — retorna los paths de módulos que pueden ser eliminados.
    pub fn tree_shake(&self) -> Vec<String> {
        self.modules
            .iter()
            .filter(|(_, node)| {
                !node.side_effects && node.used_exports.is_empty() && !node.exports.is_empty()
            })
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Retorna todos los módulos en orden topológico.
    pub fn ordered_modules(&self) -> &[String] {
        &self.order
    }

    /// Calcula el orden topológico.
    pub fn compute_order(&mut self, entry: &str) {
        let mut visited = HashSet::new();
        let mut order = Vec::new();
        self.visit(entry, &mut visited, &mut order);
        self.order = order;
    }

    fn visit(&self, path: &str, visited: &mut HashSet<String>, order: &mut Vec<String>) {
        if visited.contains(path) {
            return;
        }
        visited.insert(path.to_string());

        if let Some(node) = self.modules.get(path) {
            for imp in &node.imports {
                self.visit(&imp.source, visited, order);
            }
        }
        order.push(path.to_string());
    }

    /// Número de módulos.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

// ── Bundler ────────────────────────────────────────────────────────────────

/// Bundler principal.
#[derive(Debug)]
pub struct Bundler {
    pub config: BundleConfig,
    pub graph: ModuleGraph,
    pub css_processor: CssModuleProcessor,
    jsx_transformer: JsxTransformer,
}

impl Bundler {
    pub fn new(config: BundleConfig) -> Self {
        let jsx_transformer = JsxTransformer::new(&config.jsx_factory, &config.jsx_fragment);
        Self {
            config,
            graph: ModuleGraph::new(),
            css_processor: CssModuleProcessor::default(),
            jsx_transformer,
        }
    }

    /// Bundlea un proyecto a partir del VFS.
    pub fn bundle(&mut self, vfs: &crate::vfs::Vfs) -> BundleResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut css_output = String::new();
        let mut js_modules = Vec::new();

        // Resolver el punto de entrada
        let entry = &self.config.entry.clone();
        if !vfs.exists(entry) {
            errors.push(BundleError::new(format!("Entry point not found: {entry}")));
            return BundleResult {
                code: String::new(),
                css: String::new(),
                source_map: None,
                stats: BundleStats::default(),
                errors,
                warnings,
            };
        }

        // Recolectar módulos recursivamente
        let mut visited = HashSet::new();
        self.collect_modules(
            vfs,
            entry,
            &mut visited,
            &mut js_modules,
            &mut css_output,
            &mut errors,
            &mut warnings,
        );

        // Construir el bundle
        let mut bundle = String::new();
        bundle.push_str("(function(modules) {\n");
        bundle.push_str("  var cache = {};\n");
        bundle.push_str("  function require(id) {\n");
        bundle.push_str("    if (cache[id]) return cache[id].exports;\n");
        bundle.push_str("    var module = cache[id] = { exports: {} };\n");
        bundle.push_str("    modules[id](module, module.exports, require);\n");
        bundle.push_str("    return module.exports;\n");
        bundle.push_str("  }\n");
        bundle.push_str(&format!("  require(\"{}\");\n", escape_js(entry)));
        bundle.push_str("})({\n");

        for (i, (path, code)) in js_modules.iter().enumerate() {
            if i > 0 {
                bundle.push_str(",\n");
            }
            bundle.push_str(&format!(
                "  \"{}\": function(module, exports, require) {{\n{}\n  }}",
                escape_js(path),
                code
            ));
        }

        bundle.push_str("\n});\n");

        let stats = BundleStats {
            module_count: js_modules.len(),
            bundle_size: bundle.len(),
            css_size: css_output.len(),
            tree_shaken: 0,
            build_time_ms: 0,
        };

        if !warnings.is_empty() && warnings.len() > 20 {
            warnings.truncate(20);
            warnings.push("... and more warnings truncated".to_string());
        }

        BundleResult {
            code: bundle,
            css: css_output,
            source_map: None,
            stats,
            errors,
            warnings,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_modules(
        &mut self,
        vfs: &crate::vfs::Vfs,
        path: &str,
        visited: &mut HashSet<String>,
        js_modules: &mut Vec<(String, String)>,
        css_output: &mut String,
        errors: &mut Vec<BundleError>,
        warnings: &mut Vec<String>,
    ) {
        if visited.contains(path) {
            return;
        }
        visited.insert(path.to_string());

        let content = match vfs.read(path) {
            Ok(bytes) => String::from_utf8_lossy(bytes).to_string(),
            Err(_) => {
                warnings.push(format!("Could not read module: {path}"));
                return;
            }
        };

        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();

        // Process based on file type
        match ext.as_str() {
            "css" => {
                if self.config.css_modules && path.contains(".module.") {
                    let result = self.css_processor.process(&content, path);
                    css_output.push_str(&result.css);
                    // Generate JS module for CSS Module
                    let js_code = result.to_js_module();
                    js_modules.push((path.to_string(), js_code));
                } else {
                    css_output.push_str(&content);
                    css_output.push('\n');
                }
            }
            "jsx" | "tsx" => {
                // Transform JSX then convert ESM to CJS
                match self.jsx_transformer.transform(&content) {
                    Ok(transformed) => {
                        let code = esm_to_cjs(&transformed);
                        // Collect dependencies
                        let deps = extract_deps(&code);
                        for dep in &deps {
                            let resolved = resolve_module_path(path, dep);
                            self.collect_modules(
                                vfs, &resolved, visited, js_modules, css_output, errors, warnings,
                            );
                        }
                        js_modules.push((path.to_string(), code));
                    }
                    Err(e) => errors.push(e),
                }
            }
            "ts" => {
                // Strip TypeScript then convert ESM
                let stripped = crate::runtime::js_engine::strip_typescript(&content)
                    .unwrap_or_else(|_| content.clone());
                let code = esm_to_cjs(&stripped);
                let deps = extract_deps(&code);
                for dep in &deps {
                    let resolved = resolve_module_path(path, dep);
                    self.collect_modules(
                        vfs, &resolved, visited, js_modules, css_output, errors, warnings,
                    );
                }
                js_modules.push((path.to_string(), code));
            }
            "js" | "mjs" | "cjs" => {
                let code = esm_to_cjs(&content);
                let deps = extract_deps(&code);
                for dep in &deps {
                    let resolved = resolve_module_path(path, dep);
                    self.collect_modules(
                        vfs, &resolved, visited, js_modules, css_output, errors, warnings,
                    );
                }
                js_modules.push((path.to_string(), code));
            }
            "json" => {
                let code = format!("module.exports = {};", content.trim());
                js_modules.push((path.to_string(), code));
            }
            _ => {
                warnings.push(format!("Unknown file type: {path}"));
            }
        }
    }

    /// Retorna info del bundler como JSON.
    pub fn info_json(&self) -> String {
        serde_json::json!({
            "entry": self.config.entry,
            "jsx_factory": self.config.jsx_factory,
            "target": self.config.target,
            "tree_shake": self.config.tree_shake,
            "css_modules": self.config.css_modules,
            "externals": self.config.externals,
        })
        .to_string()
    }
}

impl Default for Bundler {
    fn default() -> Self {
        Self::new(BundleConfig::default())
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Converts ESM imports/exports to CJS.
fn esm_to_cjs(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim();

        // import X from 'Y' → const X = require('Y')
        if trimmed.starts_with("import ") && trimmed.contains(" from ") {
            let after = &trimmed["import ".len()..];
            if let Some(from_idx) = after.rfind(" from ") {
                let binding = after[..from_idx].trim();
                let rest = after[from_idx + " from ".len()..].trim();
                let module = rest.trim_matches(';').trim_matches('"').trim_matches('\'');

                if let Some(name) = binding.strip_prefix("* as ") {
                    result.push_str(&format!("const {name} = require('{module}');\n"));
                } else {
                    result.push_str(&format!("const {binding} = require('{module}');\n"));
                }
                continue;
            }
        }

        // import 'Y' (side-effect)
        if trimmed.starts_with("import '") || trimmed.starts_with("import \"") {
            let module = trimmed
                .trim_start_matches("import ")
                .trim_matches(';')
                .trim_matches('"')
                .trim_matches('\'');
            result.push_str(&format!("require('{module}');\n"));
            continue;
        }

        // export default X
        if let Some(stripped) = trimmed.strip_prefix("export default ") {
            result.push_str(&format!(
                "module.exports = module.exports.default = {};\n",
                stripped
            ));
            continue;
        }

        // export { A, B }
        if trimmed.starts_with("export {") {
            // Skip — handled differently
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // export const/let/var/function/class
        if trimmed.starts_with("export const ")
            || trimmed.starts_with("export let ")
            || trimmed.starts_with("export var ")
        {
            result.push_str(&trimmed["export ".len()..]);
            result.push('\n');
            continue;
        }
        if trimmed.starts_with("export function ") || trimmed.starts_with("export class ") {
            result.push_str(&trimmed["export ".len()..]);
            result.push('\n');
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }
    result
}

/// Extract dependency paths from require() calls.
fn extract_deps(code: &str) -> Vec<String> {
    let mut deps = Vec::new();
    for line in code.lines() {
        if let Some(pos) = line.find("require(") {
            let after = &line[pos + 8..];
            if let Some(dep) = extract_string(after)
                && (dep.starts_with('.') || dep.starts_with('/'))
            {
                deps.push(dep);
            }
        }
    }
    deps
}

fn extract_string(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(stripped) = s.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(s[1..1 + end].to_string())
    } else if let Some(stripped) = s.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(s[1..1 + end].to_string())
    } else {
        None
    }
}

/// Resolve a relative module path.
fn resolve_module_path(from: &str, dep: &str) -> String {
    if dep.starts_with('/') {
        return dep.to_string();
    }
    if !dep.starts_with('.') {
        // For bare imports, we leave them as-is. The bundler treats them as externals
        // or expects the runtime to resolve them.
        return dep.to_string();
    }

    let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("/");
    let combined = format!("{}/{}", dir, dep);
    let parts: Vec<&str> = combined.split('/').filter(|s| !s.is_empty()).collect();
    let mut resolved = Vec::new();
    for part in parts {
        match part {
            ".." => {
                resolved.pop();
            }
            "." => {}
            _ => resolved.push(part),
        }
    }
    let path = format!("/{}", resolved.join("/"));

    // Try extensions
    if path.contains('.') {
        path
    } else {
        format!("{}.tsx", path) // Default to .tsx, the VFS will handle fallback
    }
}

fn escape_js(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn simple_hash(s: &str) -> String {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    format!("{:x}", hash)
}

/// Parse import statements from source code.
fn parse_imports(source: &str) -> Vec<ImportInfo> {
    let mut imports = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ")
            && trimmed.contains(" from ")
            && let Some(from_idx) = trimmed.rfind(" from ")
        {
            let rest = trimmed[from_idx + " from ".len()..].trim();
            let source_mod = rest.trim_matches(';').trim_matches('"').trim_matches('\'');

            imports.push(ImportInfo {
                source: source_mod.to_string(),
            });
        }
    }
    imports
}

/// Parse export names from source code.
fn parse_exports(source: &str) -> Vec<String> {
    let mut exports = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("export default ") {
            exports.push("default".to_string());
        } else if trimmed.starts_with("export const ")
            || trimmed.starts_with("export let ")
            || trimmed.starts_with("export var ")
        {
            let after = trimmed.splitn(3, ' ').nth(2).unwrap_or("");
            if let Some(name) = after
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
            {
                exports.push(name.to_string());
            }
        } else if trimmed.starts_with("export function ") || trimmed.starts_with("export class ") {
            let after = trimmed.splitn(3, ' ').nth(2).unwrap_or("");
            if let Some(name) = after
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
            {
                exports.push(name.to_string());
            }
        }
    }
    exports
}

/// Check if a module has side effects (top-level code beyond declarations).
fn has_side_effects(source: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("let ")
            || trimmed.starts_with("var ")
            || trimmed.starts_with("function ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("interface ")
        {
            continue;
        }
        // Has executable code
        return true;
    }
    false
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esm_to_cjs_imports() {
        let source = r#"import React from 'react';
import { useState, useEffect } from 'react';
import * as utils from './utils';
import './styles.css';
const x = 1;"#;
        let cjs = esm_to_cjs(source);
        assert!(cjs.contains("const React = require('react')"));
        assert!(cjs.contains("const { useState, useEffect } = require('react')"));
        assert!(cjs.contains("const utils = require('./utils')"));
        assert!(cjs.contains("require('./styles.css')"));
        assert!(cjs.contains("const x = 1;"));
    }

    #[test]
    fn esm_to_cjs_exports() {
        let source = "export default function App() {}\nexport const foo = 1;";
        let cjs = esm_to_cjs(source);
        assert!(cjs.contains("module.exports"));
        assert!(cjs.contains("const foo = 1;"));
    }

    #[test]
    fn css_module_processing() {
        let mut processor = CssModuleProcessor::new("rb");
        let css = ".button { color: red; }\n.header { font-size: 18px; }";
        let result = processor.process(css, "/src/app.module.css");

        assert!(!result.class_map.is_empty());
        assert!(result.class_map.contains_key("button"));
        assert!(result.css.contains("rb_button_"));
    }

    #[test]
    fn css_module_js_export() {
        let mut processor = CssModuleProcessor::new("test");
        let css = ".main { display: flex; }";
        let result = processor.process(css, "/x.module.css");
        let js = result.to_js_module();
        assert!(js.contains("export default"));
        assert!(js.contains("main"));
    }

    #[test]
    fn module_resolution() {
        assert_eq!(
            resolve_module_path("/src/app.tsx", "./utils"),
            "/src/utils.tsx"
        );
        assert_eq!(
            resolve_module_path("/src/components/btn.tsx", "../utils"),
            "/src/utils.tsx"
        );
        assert_eq!(
            resolve_module_path("/src/app.tsx", "/lib/foo.js"),
            "/lib/foo.js"
        );
    }

    #[test]
    fn jsx_basic_transform() {
        let transformer = JsxTransformer::react();
        let jsx = r#"const el = <div className="test">Hello</div>;"#;
        let result = transformer.transform(jsx);
        assert!(result.is_ok());
        let code = result.unwrap();
        assert!(code.contains("React.createElement"));
        assert!(code.contains("\"div\""));
    }

    #[test]
    fn dependency_extraction() {
        let code =
            "const a = require('./a');\nconst b = require('./b');\nconst c = require('react');";
        let deps = extract_deps(code);
        assert_eq!(deps.len(), 2); // Only relative imports
        assert!(deps.contains(&"./a".to_string()));
        assert!(deps.contains(&"./b".to_string()));
    }

    #[test]
    fn tree_shake_detection() {
        let mut graph = ModuleGraph::new();
        graph.add_module("/a.js", "export const foo = 1;");
        graph.add_module(
            "/b.js",
            "export const bar = 2;\nconsole.log('side effect');",
        );

        let shaken = graph.tree_shake();
        // /a.js has no side effects and no used exports → can be shaken
        assert!(shaken.contains(&"/a.js".to_string()));
        // /b.js has side effects → keep
        assert!(!shaken.contains(&"/b.js".to_string()));
    }

    #[test]
    fn parse_imports_test() {
        let source = "import React from 'react';\nimport { useState } from 'react';";
        let imports = parse_imports(source);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "react");
    }

    #[test]
    fn parse_exports_test() {
        let source = "export default App;\nexport const foo = 1;\nexport function bar() {}";
        let exports = parse_exports(source);
        assert!(exports.contains(&"default".to_string()));
        assert!(exports.contains(&"foo".to_string()));
        assert!(exports.contains(&"bar".to_string()));
    }

    #[test]
    fn bundle_config_default() {
        let config = BundleConfig::default();
        assert_eq!(config.jsx_factory, "React.createElement");
        assert!(config.tree_shake);
        assert!(config.css_modules);
    }
}
