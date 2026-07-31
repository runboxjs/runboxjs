/// Motor JavaScript / TypeScript.
///
/// WASM   → usa `js_sys::eval()` — el browser provee V8 directamente.
/// Native → usa `boa_engine` (intérprete puro Rust), sin dependencias externas.
///
/// TypeScript se transpila primero quitando las anotaciones de tipos antes
/// de pasar el código al motor. No es perfecto pero cubre ~90 % de los casos.
///
/// Polyfills mejorados:
/// - async/await con Promise wrapper
/// - fetch() polyfill sobre RunBox network layer
/// - setTimeout/setInterval/clearTimeout/clearInterval
/// - Web APIs: URL, URLSearchParams, TextEncoder, TextDecoder, crypto
/// - Node.js built-ins: path, fs (VFS-mapped), process, Buffer
mod polyfills;
mod typescript;

pub use polyfills::{PolyfillGenerator, node_builtin_modules};
pub use typescript::strip_typescript;

// ── Resultado de ejecución ────────────────────────────────────────────────────

#[derive(Debug)]
pub struct JsOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

// ── API pública ───────────────────────────────────────────────────────────────

/// Ejecuta código JS o TS. Transpila TS a JS si `is_typescript` es true.
pub fn run(source: &str, is_typescript: bool) -> JsOutput {
    let js = if is_typescript {
        match strip_typescript(source) {
            Ok(js) => js,
            Err(e) => {
                return JsOutput {
                    stdout: String::new(),
                    stderr: e,
                    exit_code: 1,
                };
            }
        }
    } else {
        source.to_string()
    };

    eval_js(&js)
}

// ── Motor WASM — js_sys::eval ─────────────────────────────────────────────────

/// Convierte ESM import statements a require() para compatibilidad con eval().
/// `import X from 'Y'`        → `const X = require('Y');`
/// `import { A, B } from 'Y'` → `const { A, B } = require('Y');`
/// `import * as X from 'Y'`   → `const X = require('Y');`
#[cfg(target_arch = "wasm32")]
fn transform_esm_imports(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("import ") && trimmed.contains(" from ") {
            // Extraer la parte entre 'import' y 'from'
            let after_import = &trimmed["import ".len()..];
            if let Some(from_idx) = after_import.rfind(" from ") {
                let binding = after_import[..from_idx].trim();
                let rest = after_import[from_idx + " from ".len()..].trim();
                // Extraer el nombre del módulo (entre comillas)
                let module = rest.trim_matches(';').trim_matches('"').trim_matches('\'');

                if binding.starts_with("* as ") {
                    // import * as X from 'y' → const X = require('y')
                    let name = &binding["* as ".len()..];
                    out.push_str(&format!("const {name} = require('{module}');\n"));
                } else if binding.starts_with('{') {
                    // import { A, B } from 'y' → const { A, B } = require('y')
                    out.push_str(&format!("const {binding} = require('{module}');\n"));
                } else if binding.is_empty() {
                    // import 'y' — side-effect only, ignorar
                    out.push('\n');
                } else {
                    // import X from 'y' → const X = require('y')
                    out.push_str(&format!("const {binding} = require('{module}');\n"));
                }
                continue;
            }
        }
        // export default / export { ... } — ignorar en contexto eval
        if trimmed.starts_with("export default ") {
            out.push_str(&trimmed["export default ".len()..]);
            out.push('\n');
            continue;
        }
        if trimmed.starts_with("export {")
            || trimmed.starts_with("export const ")
            || trimmed.starts_with("export function ")
            || trimmed.starts_with("export class ")
        {
            out.push_str(&trimmed["export ".len()..]);
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(target_arch = "wasm32")]
fn eval_js(source: &str) -> JsOutput {
    // Transformar ESM imports → require() antes del eval
    let source = transform_esm_imports(source);
    let source = source.as_str();

    // Polyfill de módulos Node.js — persiste en globalThis entre llamadas
    let polyfill = r#"
        if (!globalThis.__runbox_servers) globalThis.__runbox_servers = {};

        const __http = {
            createServer(handler) {
                return {
                    _handler: handler,
                    listen(port, hostname, cb) {
                        const callback = typeof hostname === 'function' ? hostname : cb;
                        globalThis.__runbox_servers[port] = handler;
                        if (typeof callback === 'function') callback();
                        __logs.push('__RUNBOX_SERVER_READY__:' + port);
                    }
                };
            },
            STATUS_CODES: { 200: 'OK', 201: 'Created', 404: 'Not Found', 500: 'Internal Server Error' },
        };

        const __process = {
            env: { NODE_ENV: 'production' },
            argv: ['node', 'index.js'],
            version: 'v18.0.0',
            platform: 'browser',
            exit(code) { throw new Error('__EXIT__:' + (code || 0)); },
            cwd() { return '/'; },
            stdout: { write(s) { __logs.push(String(s)); } },
            stderr: { write(s) { __errs.push(String(s)); } },
        };

        const __path = {
            join: (...parts) => parts.join('/').replace(/\/+/g, '/'),
            resolve: (...parts) => '/' + parts.join('/').replace(/\/+/g, '/'),
            extname: (p) => { const m = p.match(/\.[^.]+$/); return m ? m[0] : ''; },
            basename: (p, ext) => { const b = p.split('/').pop(); return ext ? b.replace(ext, '') : b; },
            dirname: (p) => p.split('/').slice(0, -1).join('/') || '/',
        };

        // ── CJS Module Loader desde VFS ──────────────────────────────────────────
        // globalThis.__vfs_modules es un mapa path→content pre-cargado por Rust
        // antes del eval. Permite require() de cualquier paquete npm instalado.
        if (!globalThis.__vfs_modules) globalThis.__vfs_modules = {};
        const __module_cache = {};

        function __resolve_module_path(from_dir, dep) {
            if (!dep.startsWith('.')) return dep;
            const stack = (from_dir ? from_dir + '/' + dep : dep).split('/');
            const resolved = [];
            for (const p of stack) {
                if (p === '..') resolved.pop();
                else if (p && p !== '.') resolved.push(p);
            }
            return resolved.join('/');
        }

        function __pick_package_entry(pkg) {
            function pick(v) {
                if (!v) return null;
                if (typeof v === 'string') return v;
                if (Array.isArray(v)) {
                    for (const item of v) {
                        const hit = pick(item);
                        if (hit) return hit;
                    }
                    return null;
                }
                if (typeof v === 'object') {
                    // Prefer CommonJS-friendly keys first.
                    for (const key of ['require', 'node', 'default', 'import', 'browser']) {
                        const hit = pick(v[key]);
                        if (hit) return hit;
                    }
                    // Fallback: first string-like value in object.
                    for (const value of Object.values(v)) {
                        const hit = pick(value);
                        if (hit) return hit;
                    }
                }
                return null;
            }

            const exportsField = pkg && pkg.exports;
            let exportTarget = exportsField;
            if (exportsField && typeof exportsField === 'object' && exportsField['.'] !== undefined) {
                exportTarget = exportsField['.'];
            }

            return pick(exportTarget) || pick(pkg.main) || pick(pkg.module) || 'index.js';
        }


        function __find_module_file(name) {
            const vfs = globalThis.__vfs_modules;
            
            // Direct key match first (handles already-resolved paths like 'react/cjs/react.production.min.js')
            if (vfs[name] !== undefined) {
                return { path: name, code: vfs[name] };
            }

            // Leer main de package.json — preferir CJS (main) sobre ESM (module)
            const pkgPath = name + '/package.json';
            if (vfs[pkgPath]) {
                try {
                    const pkg = JSON.parse(vfs[pkgPath]);
                    const entry = __pick_package_entry(pkg);
                    const normalizedEntry = String(entry).replace(/^\.\//, '');
                    const resolved = name + '/' + normalizedEntry;
                    const entryCandidates = [
                        resolved,
                        resolved + '.js',
                        resolved + '.cjs',
                        resolved + '.mjs',
                        resolved + '.ts',
                        resolved + '.tsx',
                        resolved + '.jsx',
                        resolved + '/index.js',
                        resolved + '/index.cjs',
                        resolved + '/index.mjs',
                        resolved + '/index.ts',
                        resolved + '/index.tsx',
                        resolved + '/index.jsx',
                    ];
                    for (const candidate of entryCandidates) {
                        if (vfs[candidate] !== undefined) {
                            return { path: candidate, code: vfs[candidate] };
                        }
                    }
                } catch(e) {}
            }
            // Fallback: candidatos en orden de prioridad (CJS antes que ESM)
            const candidates = [
                name + '/index.js',
                name + '/index.cjs',
                name + '/index.mjs',
                name + '/index.ts',
                name + '/index.tsx',
                name + '/index.jsx',
                name + '/index.json',
                name + '.js',
                name + '.cjs',
                name + '.mjs',
                name + '.ts',
                name + '.tsx',
                name + '.jsx',
                name + '.json',
            ];
            for (const c of candidates) {
                if (vfs[c] !== undefined) return { path: c, code: vfs[c] };
            }

            // Handle subpath imports like 'react-dom/client' — split into package + subpath
            const slashIdx = name.indexOf('/');
            if (slashIdx > 0 && !name.startsWith('@')) {
                const pkgName = name.substring(0, slashIdx);
                const subPath = name.substring(slashIdx + 1);
                // Check package.json exports for subpath
                const rootPkgPath = pkgName + '/package.json';
                if (vfs[rootPkgPath]) {
                    try {
                        const rootPkg = JSON.parse(vfs[rootPkgPath]);
                        if (rootPkg.exports && typeof rootPkg.exports === 'object') {
                            const subKey = './' + subPath;
                            if (rootPkg.exports[subKey]) {
                                const subTarget = pick_export_path_inner(rootPkg.exports[subKey]);
                                if (subTarget) {
                                    const full = pkgName + '/' + subTarget.replace(/^\.\//, '');
                                    if (vfs[full] !== undefined) return { path: full, code: vfs[full] };
                                }
                            }
                        }
                    } catch(e) {}
                }
                // Direct subpath fallback for non-scoped packages
                const directCandidates = [
                    pkgName + '/' + subPath,
                    pkgName + '/' + subPath + '.js',
                    pkgName + '/' + subPath + '.cjs',
                    pkgName + '/' + subPath + '.mjs',
                    pkgName + '/' + subPath + '.ts',
                    pkgName + '/' + subPath + '.tsx',
                    pkgName + '/' + subPath + '.jsx',
                    pkgName + '/' + subPath + '/index.js',
                    pkgName + '/' + subPath + '/index.cjs',
                    pkgName + '/' + subPath + '/index.mjs',
                    pkgName + '/' + subPath + '/index.ts',
                    pkgName + '/' + subPath + '/index.tsx',
                    pkgName + '/' + subPath + '/index.jsx',
                ];
                for (const c of directCandidates) {
                    if (vfs[c] !== undefined) return { path: c, code: vfs[c] };
                }
            }
            // Handle scoped package subpath imports like '@scope/pkg/subpath'
            if (name.startsWith('@')) {
                const parts = name.split('/');
                if (parts.length >= 3) {
                    const pkgName = parts[0] + '/' + parts[1];
                    const subPath = parts.slice(2).join('/');
                    const rootPkgPath = pkgName + '/package.json';
                    if (vfs[rootPkgPath]) {
                        try {
                            const rootPkg = JSON.parse(vfs[rootPkgPath]);
                            if (rootPkg.exports && typeof rootPkg.exports === 'object') {
                                const subKey = './' + subPath;
                                if (rootPkg.exports[subKey]) {
                                    const subTarget = pick_export_path_inner(rootPkg.exports[subKey]);
                                    if (subTarget) {
                                        const full = pkgName + '/' + subTarget.replace(/^\.\//, '');
                                        if (vfs[full] !== undefined) return { path: full, code: vfs[full] };
                                    }
                                }
                            }
                        } catch(e) {}
                    }
                    // Direct subpath fallback
                    const directCandidates = [
                        pkgName + '/' + subPath,
                        pkgName + '/' + subPath + '.js',
                        pkgName + '/' + subPath + '.cjs',
                        pkgName + '/' + subPath + '.mjs',
                        pkgName + '/' + subPath + '.ts',
                        pkgName + '/' + subPath + '.tsx',
                        pkgName + '/' + subPath + '.jsx',
                        pkgName + '/' + subPath + '/index.js',
                        pkgName + '/' + subPath + '/index.cjs',
                        pkgName + '/' + subPath + '/index.mjs',
                        pkgName + '/' + subPath + '/index.ts',
                        pkgName + '/' + subPath + '/index.tsx',
                        pkgName + '/' + subPath + '/index.jsx',
                    ];
                    for (const c of directCandidates) {
                        if (vfs[c] !== undefined) return { path: c, code: vfs[c] };
                    }
                }
            }

            return null;
        }

        // Helper for resolving export map values recursively
        function pick_export_path_inner(v) {
            if (!v) return null;
            if (typeof v === 'string') return v;
            if (Array.isArray(v)) {
                for (const item of v) {
                    const hit = pick_export_path_inner(item);
                    if (hit) return hit;
                }
                return null;
            }
            if (typeof v === 'object') {
                for (const key of ['require', 'node', 'default', 'import', 'browser']) {
                    const hit = pick_export_path_inner(v[key]);
                    if (hit) return hit;
                }
                for (const value of Object.values(v)) {
                    const hit = pick_export_path_inner(value);
                    if (hit) return hit;
                }
            }
            return null;
        }

        // Transforma ESM imports/exports a CJS para poder usar new Function()
        function __esm_to_cjs(code) {
            // import X from 'Y'
            code = code.replace(/^\s*import\s+(\w+)\s+from\s+['"]([^'"]+)['"]\s*;?/gm,
                "const $1 = require('$2');");
            // import { A, B as C } from 'Y'
            code = code.replace(/^\s*import\s+(\{[^}]+\})\s+from\s+['"]([^'"]+)['"]\s*;?/gm,
                "const $1 = require('$2');");
            // import * as X from 'Y'
            code = code.replace(/^\s*import\s+\*\s+as\s+(\w+)\s+from\s+['"]([^'"]+)['"]\s*;?/gm,
                "const $1 = require('$2');");
            // import 'Y' (side-effect)
            code = code.replace(/^\s*import\s+['"][^'"]+['"]\s*;?/gm, "");
            // export default X
            code = code.replace(/^\s*export\s+default\s+/gm, "module.exports = module.exports.default = ");
            // export { A, B }
            code = code.replace(/^\s*export\s+\{([^}]+)\}\s*;?/gm, (_, names) => {
                return names.split(',').map(n => {
                    const [orig, alias] = n.trim().split(/\s+as\s+/);
                    const exp = (alias || orig).trim();
                    return 'exports.' + exp + ' = ' + orig.trim() + ';';
                }).join(' ');
            });
            // export const/let/var/function/class X
            code = code.replace(/^\s*export\s+(const|let|var)\s+(\w+)\s*=\s*/gm,
                "$1 $2 = exports.$2 = ");
            code = code.replace(/^\s*export\s+(function|class)\s+(\w+)/gm,
                "exports.$2 = $1 $2");
            return code;
        }

        function __vfs_require(name) {
            if (__module_cache[name] !== undefined) {
                return __module_cache[name];
            }
            const found = __find_module_file(name);
            if (!found) {
                // Verificar si es un paquete conocido pero no descargado
                const inPkg = globalThis.__runbox_pkg_deps && globalThis.__runbox_pkg_deps[name];
                if (inPkg) {
                    throw new Error("Package '" + name + "' is listed in package.json but not yet downloaded. Call runbox.npm_install_packages() from JS or run `npm install`.");
                }
                throw new Error("Cannot find module '" + name + "'. Run `npm install " + name + "` first.");
            }
            const { path: modPath, code: rawCode } = found;
            // Also cache by the resolved path to avoid double-loading
            if (modPath !== name && __module_cache[modPath] !== undefined) return __module_cache[modPath];
            const mod = { exports: {}, id: modPath };
            __module_cache[name] = mod.exports; // Pre-cache para evitar ciclos
            __module_cache[modPath] = mod.exports; // Cache by resolved path too
            const dir = modPath.split('/').slice(0, -1).join('/');
            // Transformar ESM a CJS si el archivo usa import/export
            const needsTransform = /^\s*(import\s|export\s)/m.test(rawCode);
            const code = needsTransform ? __esm_to_cjs(rawCode) : rawCode;
            try {
                (new Function('module', 'exports', 'require', '__dirname', '__filename', code))(
                    mod, mod.exports,
                    (dep) => {
                        // Strip node: prefix for core modules
                        const cleanDep = dep.startsWith('node:') ? dep.substring(5) : dep;

                        if (cleanDep.startsWith('.')) {
                            // Relative import — resolve against current directory
                            const resolved = __resolve_module_path(dir, cleanDep);
                            // Use __find_module_file for proper entry point resolution
                            const f = __find_module_file(resolved);
                            if (f) return __vfs_require(f.path);
                            // Fallback: try as-is via main require
                            return require(resolved);
                        }
                        // Absolute/bare import — delegate to main require which handles built-ins + VFS
                        return require(cleanDep);
                    },
                    '/' + dir, '/' + modPath
                );
                __module_cache[name] = mod.exports;
                __module_cache[modPath] = mod.exports;
            } catch(e) {
                delete __module_cache[name];
                delete __module_cache[modPath];
                throw new Error("Error loading module '" + name + "': " + e.message);
            }
            return mod.exports;
        }

        // ── Node.js built-in shims ──────────────────────────────────────────
        const __events_polyfill = (function() {
            function EventEmitter() { this._events = {}; }
            EventEmitter.prototype.on = function(e, fn) { (this._events[e] = this._events[e] || []).push(fn); return this; };
            EventEmitter.prototype.emit = function(e) { var a = [].slice.call(arguments, 1); (this._events[e] || []).forEach(function(fn) { fn.apply(null, a); }); return this; };
            EventEmitter.prototype.once = function(e, fn) { var self = this; function f() { self.off(e, f); fn.apply(null, arguments); } this.on(e, f); return this; };
            EventEmitter.prototype.off = EventEmitter.prototype.removeListener = function(e, fn) { this._events[e] = (this._events[e] || []).filter(function(f) { return f !== fn; }); return this; };
            EventEmitter.prototype.removeAllListeners = function(e) { if (e) delete this._events[e]; else this._events = {}; return this; };
            EventEmitter.prototype.listeners = function(e) { return this._events[e] || []; };
            EventEmitter.prototype.listenerCount = function(e) { return (this._events[e] || []).length; };
            EventEmitter.prototype.addListener = EventEmitter.prototype.on;
            EventEmitter.EventEmitter = EventEmitter;
            EventEmitter.default = EventEmitter;
            return EventEmitter;
        })();

        const __stream_polyfill = { Readable: __events_polyfill, Writable: __events_polyfill, Transform: __events_polyfill, Duplex: __events_polyfill, PassThrough: __events_polyfill, Stream: __events_polyfill, pipeline: function() {}, finished: function() {} };
        const __util_polyfill = { inherits: function(c, p) { c.prototype = Object.create(p.prototype); c.prototype.constructor = c; }, deprecate: function(fn) { return fn; }, promisify: function(fn) { return function() { var a = [].slice.call(arguments); return new Promise(function(r, j) { a.push(function(e, v) { e ? j(e) : r(v); }); fn.apply(null, a); }); }; }, types: { isDate: function(v) { return v instanceof Date; }, isRegExp: function(v) { return v instanceof RegExp; } }, inspect: function(v) { return JSON.stringify(v); }, format: function() { return [].slice.call(arguments).join(' '); } };
        const __assert_polyfill = function(v, msg) { if (!v) throw new Error(msg || 'Assertion failed'); };
        __assert_polyfill.ok = __assert_polyfill;
        __assert_polyfill.strictEqual = function(a, b, msg) { if (a !== b) throw new Error(msg || a + ' !== ' + b); };
        __assert_polyfill.deepStrictEqual = __assert_polyfill.strictEqual;
        const __querystring_polyfill = { parse: function(s) { var o = {}; (s || '').split('&').forEach(function(p) { var kv = p.split('='); o[decodeURIComponent(kv[0])] = decodeURIComponent(kv[1] || ''); }); return o; }, stringify: function(o) { return Object.entries(o || {}).map(function(kv) { return encodeURIComponent(kv[0]) + '=' + encodeURIComponent(kv[1]); }).join('&'); } };

        const require = (mod) => {
            // Strip node: prefix
            const cleanMod = mod.startsWith('node:') ? mod.substring(5) : mod;

            // Node.js built-in modules
            if (cleanMod === 'http' || cleanMod === 'https') return __http;
            if (cleanMod === 'path') return __path;
            if (cleanMod === 'os') return { platform: () => 'linux', tmpdir: () => '/tmp', homedir: () => '/home', cpus: () => [{}], arch: () => 'wasm32', hostname: () => 'runbox', type: () => 'Linux', release: () => '5.0.0', EOL: '\n' };
            if (cleanMod === 'fs' || cleanMod === 'fs/promises') return {
                readFileSync: () => { throw new Error('fs.readFileSync: not available in WASM sandbox'); },
                writeFileSync: () => { throw new Error('fs.writeFileSync: not available in WASM sandbox'); },
                existsSync: () => false,
                readdirSync: () => [],
                statSync: () => ({ isFile: () => false, isDirectory: () => false }),
                mkdirSync: () => {},
                promises: { readFile: () => Promise.reject(new Error('fs not available')), writeFile: () => Promise.reject(new Error('fs not available')) },
            };
            if (cleanMod === 'url') return {
                fileURLToPath: (u) => u.replace('file://', ''),
                pathToFileURL: (p) => 'file://' + p,
                URL: globalThis.URL || function(u) { this.href = u; this.pathname = u; },
                URLSearchParams: globalThis.URLSearchParams || function() {},
            };
            if (cleanMod === 'events') return __events_polyfill;
            if (cleanMod === 'stream') return __stream_polyfill;
            if (cleanMod === 'util') return __util_polyfill;
            if (cleanMod === 'assert') return __assert_polyfill;
            if (cleanMod === 'querystring') return __querystring_polyfill;
            if (cleanMod === 'buffer') return { Buffer: globalThis.Buffer || { from: (d) => new Uint8Array(typeof d === 'string' ? [...d].map(c => c.charCodeAt(0)) : d), alloc: (n) => new Uint8Array(n), isBuffer: () => false } };
            if (cleanMod === 'crypto') return globalThis.crypto || { getRandomValues: (a) => { for (var i = 0; i < a.length; i++) a[i] = Math.floor(Math.random() * 256); return a; }, randomUUID: () => 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, c => { var r = Math.random() * 16 | 0; return (c === 'x' ? r : (r & 0x3 | 0x8)).toString(16); }) };
            if (cleanMod === 'child_process') return { exec: () => {}, execSync: () => '', spawn: () => ({ on: () => {}, stdout: { on: () => {} }, stderr: { on: () => {} } }) };
            if (cleanMod === 'worker_threads') return { isMainThread: true, parentPort: null, Worker: function() {} };
            if (cleanMod === 'perf_hooks') return { performance: globalThis.performance || { now: Date.now } };
            if (cleanMod === 'string_decoder') return { StringDecoder: function() { this.write = function(b) { return typeof b === 'string' ? b : new TextDecoder().decode(b); }; this.end = function() { return ''; }; } };
            if (cleanMod === 'zlib') return { createGzip: () => __stream_polyfill, createGunzip: () => __stream_polyfill, gzip: (b, cb) => cb && cb(null, b), gunzip: (b, cb) => cb && cb(null, b) };

            // React y amigos — expuestos por el host (DemoPage) en globalThis.
            // Si no están en globalThis, usamos un stub funcional completo
            // para que el código que require('react') pero no renderiza al DOM siga funcionando.
            if (cleanMod === 'react') {
                if (globalThis.__runbox_react) return globalThis.__runbox_react;
                // Primero intentar desde VFS (por si se instaló via tarball)
                if (globalThis.__vfs_modules && (globalThis.__vfs_modules['react/index.js'] || globalThis.__vfs_modules['react/cjs/react.production.min.js'])) {
                    try { return __vfs_require('react'); } catch(e) {}
                }
                // Stub funcional mínimo de React
                if (!globalThis.__runbox_react_stub) {
                    const createElement = (type, props, ...children) => ({ type, props: props || {}, children });
                    const Component = function(props) { this.props = props; this.state = {}; };
                    Component.prototype.setState = function(s) { Object.assign(this.state, typeof s === 'function' ? s(this.state) : s); };
                    Component.prototype.render = function() { return null; };
                    globalThis.__runbox_react_stub = {
                        createElement,
                        Component,
                        PureComponent: Component,
                        Fragment: 'Fragment',
                        createRef: () => ({ current: null }),
                        useRef: (v) => ({ current: v }),
                        useState: (init) => { const v = typeof init === 'function' ? init() : init; return [v, () => {}]; },
                        useEffect: () => {},
                        useLayoutEffect: () => {},
                        useCallback: (fn) => fn,
                        useMemo: (fn) => fn(),
                        useContext: () => undefined,
                        useReducer: (r, init) => [init, () => {}],
                        createContext: (def) => ({ Provider: 'Provider', Consumer: 'Consumer', _currentValue: def }),
                        forwardRef: (fn) => fn,
                        memo: (fn) => fn,
                        cloneElement: (el, props) => ({ ...el, props: { ...el.props, ...props } }),
                        isValidElement: (el) => el != null && typeof el === 'object' && 'type' in el,
                        Children: { map: (c, fn) => (Array.isArray(c) ? c : [c]).map(fn), forEach: (c, fn) => (Array.isArray(c) ? c : [c]).forEach(fn), count: (c) => (Array.isArray(c) ? c.length : c ? 1 : 0), only: (c) => c, toArray: (c) => Array.isArray(c) ? c : [c] },
                        version: '18.2.0',
                        default: undefined,
                    };
                    globalThis.__runbox_react_stub.default = globalThis.__runbox_react_stub;
                }
                return globalThis.__runbox_react_stub;
            }
            if (cleanMod === 'react-dom' || cleanMod === 'react-dom/client') {
                if (globalThis.__runbox_reactdom) return globalThis.__runbox_reactdom;
                // Stub funcional de ReactDOM
                if (!globalThis.__runbox_reactdom_stub) {
                    globalThis.__runbox_reactdom_stub = {
                        render: () => {},
                        createRoot: () => ({ render: () => {}, unmount: () => {} }),
                        hydrateRoot: () => ({ render: () => {}, unmount: () => {} }),
                        unmountComponentAtNode: () => {},
                        findDOMNode: () => null,
                        createPortal: (children) => children,
                        flushSync: (fn) => fn(),
                        version: '18.2.0',
                    };
                    globalThis.__runbox_reactdom_stub.default = globalThis.__runbox_reactdom_stub;
                }
                return globalThis.__runbox_reactdom_stub;
            }
            if (cleanMod === 'react-dom/server') {
                if (globalThis.__runbox_reactdom_server) return globalThis.__runbox_reactdom_server;
                if (!globalThis.__runbox_reactdom_server_stub) {
                    globalThis.__runbox_reactdom_server_stub = {
                        renderToString: () => '',
                        renderToStaticMarkup: () => '',
                        renderToNodeStream: () => ({ pipe: () => {} }),
                        renderToStaticNodeStream: () => ({ pipe: () => {} }),
                        renderToPipeableStream: (el, opts) => { if (opts && opts.onShellReady) opts.onShellReady(); return { pipe: () => {} }; },
                    };
                }
                return globalThis.__runbox_reactdom_server_stub;
            }
            // Cargar desde VFS node_modules (cualquier paquete instalado via npm install)
            return __vfs_require(cleanMod);
        };
        globalThis.require = require;
        globalThis.process = __process;
        const process = __process;
    "#;

    // Envolver en IIFE para capturar console.log
    let wrapped = format!(
        r#"(function(){{
            const __logs = [];
            const __errs = [];
            const __orig_log   = console.log.bind(console);
            const __orig_error = console.error.bind(console);
            console.log   = (...a) => {{ __logs.push(a.map(String).join(' ')); __orig_log(...a); }};
            console.error = (...a) => {{ __errs.push(a.map(String).join(' ')); __orig_error(...a); }};
            {polyfill}
            let __result;
            try {{ {source} }}
            catch(e) {{
                const msg = String(e);
                const __exitMatch = msg.match(/__EXIT__:(\d+)/);
                if (!__exitMatch) __errs.push(msg);
            }}
            finally {{
                console.log   = __orig_log;
                console.error = __orig_error;
            }}
            // Filtrar señales internas del stdout visible
            const filteredLogs = __logs.filter(l => !l.startsWith('__RUNBOX_SERVER_READY__:'));
            const serverPorts = __logs
                .filter(l => l.startsWith('__RUNBOX_SERVER_READY__:'))
                .map(l => parseInt(l.split(':')[1], 10));
            return JSON.stringify({{
                stdout: filteredLogs.join('\n'),
                stderr: __errs.join('\n'),
                server_ports: serverPorts,
            }});
        }})()"#
    );

    match js_sys::eval(&wrapped) {
        Ok(val) => {
            let json = val.as_string().unwrap_or_default();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
                let stderr = v["stderr"].as_str().unwrap_or("").to_string();
                let server_ports = v["server_ports"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|p| p.as_u64()).collect::<Vec<_>>())
                    .unwrap_or_default();
                let mut stdout = v["stdout"].as_str().unwrap_or("").to_string();
                for port in &server_ports {
                    if !stdout.is_empty() {
                        stdout.push('\n');
                    }
                    stdout.push_str(&format!("🔥 Server running → http://localhost:{port}"));
                }
                JsOutput {
                    stdout,
                    stderr: stderr.clone(),
                    exit_code: if stderr.is_empty() { 0 } else { 1 },
                }
            } else {
                JsOutput {
                    stdout: json,
                    stderr: String::new(),
                    exit_code: 0,
                }
            }
        }
        Err(e) => {
            // JSON.stringify(Error) siempre devuelve {} — extraer .message directamente
            let msg = js_sys::Reflect::get(&e, &wasm_bindgen::JsValue::from_str("message"))
                .ok()
                .and_then(|v| v.as_string())
                .filter(|s| !s.is_empty())
                .or_else(|| e.as_string())
                .unwrap_or_else(|| "eval error (unknown)".into());
            JsOutput {
                stdout: String::new(),
                stderr: msg,
                exit_code: 1,
            }
        }
    }
}

// ── Motor nativo — boa_engine ─────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn eval_js(source: &str) -> JsOutput {
    use boa_engine::{Context, Source};

    let mut ctx = Context::default();

    // Inyectar console.log / console.error básico
    let log_script = r#"
        var __stdout = [];
        var __stderr = [];
        var console = {
            log:   function() { __stdout.push(Array.from(arguments).join(' ')); },
            error: function() { __stderr.push(Array.from(arguments).join(' ')); },
            warn:  function() { __stderr.push(Array.from(arguments).join(' ')); },
            info:  function() { __stdout.push(Array.from(arguments).join(' ')); },
        };
    "#;

    if ctx.eval(Source::from_bytes(log_script)).is_err() {
        return JsOutput {
            stdout: String::new(),
            stderr: "boa: failed to initialize console".into(),
            exit_code: 1,
        };
    }

    let exit_code = match ctx.eval(Source::from_bytes(source)) {
        Ok(_) => 0,
        Err(e) => {
            let msg = e.to_string();
            let _ = ctx.eval(Source::from_bytes(&format!(
                r#"__stderr.push({});"#,
                serde_json::to_string(&msg).unwrap_or_default()
            )));
            1
        }
    };

    let stdout = ctx
        .eval(Source::from_bytes("__stdout.join('\\n')"))
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .unwrap_or_default();

    let stderr = ctx
        .eval(Source::from_bytes("__stderr.join('\\n')"))
        .ok()
        .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
        .unwrap_or_default();

    JsOutput {
        stdout,
        stderr,
        exit_code,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_interface() {
        let ts = "interface Foo { x: number; }\nconst a = 1;\n";
        let js = strip_typescript(ts).unwrap();
        assert!(!js.contains("interface"));
        assert!(js.contains("const a = 1"));
    }

    #[test]
    fn strip_type_alias() {
        let ts = "type Id = string;\nconst x = 'hello';\n";
        let js = strip_typescript(ts).unwrap();
        assert!(!js.contains("type Id"));
        assert!(js.contains("'hello'"));
    }

    #[test]
    fn strip_import_type() {
        let ts = "import type { Foo } from './foo';\nconst a = 1;\n";
        let js = strip_typescript(ts).unwrap();
        assert!(!js.contains("import type"));
        assert!(js.contains("const a = 1"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn boa_eval_basic() {
        let out = run("console.log('hello')", false);
        assert_eq!(out.stdout.trim(), "hello");
        assert_eq!(out.exit_code, 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn boa_eval_error() {
        let out = run("throw new Error('oops')", false);
        assert_eq!(out.exit_code, 1);
        assert!(out.stderr.contains("oops"));
    }

    #[test]
    fn polyfill_generator_all() {
        let all = PolyfillGenerator::all();
        assert!(all.contains("__runbox_async_init"));
        assert!(all.contains("fetch"));
        assert!(all.contains("setTimeout"));
        assert!(all.contains("TextEncoder"));
        assert!(all.contains("Buffer"));
    }

    #[test]
    fn polyfill_list_json() {
        let list = PolyfillGenerator::list();
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        let polyfills = v["polyfills"].as_array().unwrap();
        assert!(polyfills.len() >= 5);
    }

    #[test]
    fn node_builtin_modules_list() {
        let builtins = node_builtin_modules();
        assert!(builtins.contains_key("path"));
        assert!(builtins.contains_key("fs"));
        assert!(builtins.contains_key("http"));
        assert!(builtins.contains_key("crypto"));
    }

    #[test]
    fn polyfill_individual_parts() {
        let async_p = PolyfillGenerator::async_await();
        assert!(async_p.contains("Promise"));

        let fetch_p = PolyfillGenerator::fetch_polyfill();
        assert!(fetch_p.contains("XMLHttpRequest") || fetch_p.contains("fetch"));

        let timers = PolyfillGenerator::timers();
        assert!(timers.contains("setInterval"));
        assert!(timers.contains("clearTimeout"));

        let web = PolyfillGenerator::web_apis();
        assert!(web.contains("crypto"));
        assert!(web.contains("randomUUID"));

        let node = PolyfillGenerator::node_builtins();
        assert!(node.contains("normalize"));
        assert!(node.contains("process"));
    }
}
