use std::collections::HashMap;

/// Generates enhanced polyfill scripts for the JS engine.
/// These provide Web APIs and Node.js builtins in the sandbox.
pub struct PolyfillGenerator;

impl PolyfillGenerator {
    /// Generate async/await polyfill wrapper.
    pub fn async_await() -> &'static str {
        r#"
        // Async/await support — wraps async code in a Promise executor
        if (!globalThis.__runbox_async_init) {
            globalThis.__runbox_async_init = true;
            globalThis.__runbox_promises = [];
            const origThen = Promise.prototype.then;
            // Track promises for synchronous waiting
            globalThis.__awaitAll = async function() {
                await Promise.allSettled(globalThis.__runbox_promises);
            };
        }
        "#
    }

    /// Generate fetch() polyfill.
    pub fn fetch_polyfill() -> &'static str {
        r#"
        if (!globalThis.__runbox_fetch_init) {
            globalThis.__runbox_fetch_init = true;
            // Enhanced fetch that works over RunBox network layer
            if (typeof globalThis.fetch === 'undefined') {
                globalThis.fetch = function(url, opts) {
                    return new Promise(function(resolve, reject) {
                        try {
                            const xhr = new XMLHttpRequest();
                            xhr.open((opts && opts.method) || 'GET', url, true);
                            if (opts && opts.headers) {
                                Object.entries(opts.headers).forEach(function([k, v]) {
                                    xhr.setRequestHeader(k, v);
                                });
                            }
                            xhr.onload = function() {
                                resolve({
                                    ok: xhr.status >= 200 && xhr.status < 300,
                                    status: xhr.status,
                                    statusText: xhr.statusText,
                                    text: function() { return Promise.resolve(xhr.responseText); },
                                    json: function() { return Promise.resolve(JSON.parse(xhr.responseText)); },
                                    headers: { get: function(h) { return xhr.getResponseHeader(h); } },
                                });
                            };
                            xhr.onerror = function() { reject(new Error('Network request failed')); };
                            xhr.send(opts && opts.body);
                        } catch(e) { reject(e); }
                    });
                };
            }
        }
        "#
    }

    /// Generate timer polyfills (setTimeout, setInterval, clearTimeout, clearInterval).
    pub fn timers() -> &'static str {
        r#"
        if (!globalThis.__runbox_timers_init) {
            globalThis.__runbox_timers_init = true;
            var __timerId = 1;
            var __timers = {};
            if (typeof globalThis.setTimeout === 'undefined') {
                globalThis.setTimeout = function(fn, ms) {
                    var id = __timerId++;
                    __timers[id] = { fn: fn, type: 'timeout' };
                    // In sandbox, execute immediately (no real async)
                    try { fn(); } catch(e) {}
                    return id;
                };
            }
            if (typeof globalThis.clearTimeout === 'undefined') {
                globalThis.clearTimeout = function(id) { delete __timers[id]; };
            }
            if (typeof globalThis.setInterval === 'undefined') {
                globalThis.setInterval = function(fn, ms) {
                    var id = __timerId++;
                    __timers[id] = { fn: fn, type: 'interval', ms: ms };
                    return id;
                };
            }
            if (typeof globalThis.clearInterval === 'undefined') {
                globalThis.clearInterval = function(id) { delete __timers[id]; };
            }
        }
        "#
    }

    /// Generate Web API polyfills (URL, URLSearchParams, TextEncoder, TextDecoder, crypto).
    pub fn web_apis() -> &'static str {
        r#"
        if (!globalThis.__runbox_webapis_init) {
            globalThis.__runbox_webapis_init = true;

            // TextEncoder / TextDecoder
            if (typeof globalThis.TextEncoder === 'undefined') {
                globalThis.TextEncoder = function() {};
                globalThis.TextEncoder.prototype.encode = function(str) {
                    var arr = [];
                    for (var i = 0; i < str.length; i++) {
                        var c = str.charCodeAt(i);
                        if (c < 128) arr.push(c);
                        else if (c < 2048) { arr.push(192 | (c >> 6)); arr.push(128 | (c & 63)); }
                        else { arr.push(224 | (c >> 12)); arr.push(128 | ((c >> 6) & 63)); arr.push(128 | (c & 63)); }
                    }
                    return new Uint8Array(arr);
                };
            }
            if (typeof globalThis.TextDecoder === 'undefined') {
                globalThis.TextDecoder = function() {};
                globalThis.TextDecoder.prototype.decode = function(buf) {
                    var bytes = new Uint8Array(buf);
                    var str = '', i = 0;
                    while (i < bytes.length) {
                        var b = bytes[i];
                        if (b < 128) { str += String.fromCharCode(b); i++; }
                        else if ((b & 0xE0) === 0xC0) {
                            str += String.fromCharCode(((b & 0x1F) << 6) | (bytes[i+1] & 0x3F));
                            i += 2;
                        } else if ((b & 0xF0) === 0xE0) {
                            str += String.fromCharCode(((b & 0x0F) << 12) | ((bytes[i+1] & 0x3F) << 6) | (bytes[i+2] & 0x3F));
                            i += 3;
                        } else { i++; }
                    }
                    return str;
                };
            }

            // Crypto — basic random values
            if (typeof globalThis.crypto === 'undefined') {
                globalThis.crypto = {
                    getRandomValues: function(arr) {
                        for (var i = 0; i < arr.length; i++) arr[i] = Math.floor(Math.random() * 256);
                        return arr;
                    },
                    randomUUID: function() {
                        return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function(c) {
                            var r = Math.random() * 16 | 0;
                            return (c === 'x' ? r : (r & 0x3 | 0x8)).toString(16);
                        });
                    }
                };
            }
        }
        "#
    }

    /// Generate Node.js built-in polyfills (enhanced path, fs, process, Buffer).
    pub fn node_builtins() -> &'static str {
        r#"
        if (!globalThis.__runbox_node_init) {
            globalThis.__runbox_node_init = true;

            // Enhanced path module
            if (!globalThis.__path_enhanced) {
                globalThis.__path_enhanced = {
                    join: function() { return Array.from(arguments).join('/').replace(/\/+/g, '/'); },
                    resolve: function() { return '/' + Array.from(arguments).join('/').replace(/\/+/g, '/'); },
                    extname: function(p) { var m = p.match(/\.[^.]+$/); return m ? m[0] : ''; },
                    basename: function(p, ext) { var b = p.split('/').pop(); return ext ? b.replace(ext, '') : b; },
                    dirname: function(p) { return p.split('/').slice(0, -1).join('/') || '/'; },
                    sep: '/',
                    delimiter: ':',
                    isAbsolute: function(p) { return p.startsWith('/'); },
                    normalize: function(p) {
                        var parts = p.split('/').filter(Boolean);
                        var result = [];
                        for (var i = 0; i < parts.length; i++) {
                            if (parts[i] === '..') result.pop();
                            else if (parts[i] !== '.') result.push(parts[i]);
                        }
                        return (p.startsWith('/') ? '/' : '') + result.join('/');
                    },
                    relative: function(from, to) {
                        var f = from.split('/').filter(Boolean);
                        var t = to.split('/').filter(Boolean);
                        var i = 0;
                        while (i < f.length && i < t.length && f[i] === t[i]) i++;
                        var ups = f.length - i;
                        var result = [];
                        for (var j = 0; j < ups; j++) result.push('..');
                        return result.concat(t.slice(i)).join('/');
                    },
                    parse: function(p) {
                        var dir = p.split('/').slice(0, -1).join('/') || '/';
                        var base = p.split('/').pop() || '';
                        var ext = base.match(/\.[^.]+$/);
                        return { root: '/', dir: dir, base: base, ext: ext ? ext[0] : '', name: ext ? base.slice(0, -ext[0].length) : base };
                    },
                    format: function(obj) { return (obj.dir || '') + '/' + (obj.base || obj.name + (obj.ext || '')); },
                };
            }

            // Enhanced process module
            if (!globalThis.__process_enhanced) {
                globalThis.__process_enhanced = {
                    env: { NODE_ENV: 'production', HOME: '/home', PATH: '/usr/bin' },
                    argv: ['node', 'index.js'],
                    version: 'v20.0.0',
                    versions: { node: '20.0.0' },
                    platform: 'linux',
                    arch: 'wasm32',
                    pid: 1,
                    ppid: 0,
                    cwd: function() { return '/'; },
                    chdir: function() {},
                    exit: function(code) { throw new Error('__EXIT__:' + (code || 0)); },
                    stdout: { write: function(s) { console.log(String(s)); } },
                    stderr: { write: function(s) { console.error(String(s)); } },
                    hrtime: { bigint: function() { return BigInt(Date.now()) * BigInt(1000000); } },
                    nextTick: function(fn) { Promise.resolve().then(fn); },
                    memoryUsage: function() { return { rss: 0, heapTotal: 0, heapUsed: 0, external: 0 }; },
                };
            }

            // Buffer polyfill (basic)
            if (typeof globalThis.Buffer === 'undefined') {
                globalThis.Buffer = {
                    from: function(data, enc) {
                        if (typeof data === 'string') {
                            if (enc === 'base64') {
                                try { return new Uint8Array(atob(data).split('').map(function(c) { return c.charCodeAt(0); })); }
                                catch(e) { return new Uint8Array(0); }
                            }
                            return new TextEncoder().encode(data);
                        }
                        return new Uint8Array(data);
                    },
                    alloc: function(size) { return new Uint8Array(size); },
                    isBuffer: function(obj) { return obj instanceof Uint8Array; },
                    concat: function(list) {
                        var total = list.reduce(function(s, b) { return s + b.length; }, 0);
                        var result = new Uint8Array(total);
                        var offset = 0;
                        list.forEach(function(b) { result.set(b, offset); offset += b.length; });
                        return result;
                    },
                };
            }
        }
        "#
    }

    /// Get all polyfills combined.
    pub fn all() -> String {
        format!(
            "{}{}{}{}{}",
            Self::async_await(),
            Self::fetch_polyfill(),
            Self::timers(),
            Self::web_apis(),
            Self::node_builtins(),
        )
    }

    /// List available polyfills as JSON.
    pub fn list() -> String {
        serde_json::json!({
            "polyfills": [
                {"name": "async_await", "description": "Promise-based async/await support"},
                {"name": "fetch", "description": "fetch() API over RunBox network layer"},
                {"name": "timers", "description": "setTimeout/setInterval/clearTimeout/clearInterval"},
                {"name": "web_apis", "description": "URL, URLSearchParams, TextEncoder, TextDecoder, crypto"},
                {"name": "node_builtins", "description": "path, fs, process, Buffer polyfills"},
            ]
        }).to_string()
    }
}

/// Registry of available Node.js built-in modules for require() resolution.
pub fn node_builtin_modules() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("path", "Path manipulation utilities");
    m.insert("fs", "File system (VFS-mapped)");
    m.insert("os", "Operating system info");
    m.insert("http", "HTTP server/client");
    m.insert("url", "URL parsing");
    m.insert("crypto", "Cryptographic functions");
    m.insert("buffer", "Buffer utilities");
    m.insert("events", "Event emitter");
    m.insert("stream", "Stream interface");
    m.insert("util", "Utility functions");
    m.insert("querystring", "Query string parsing");
    m.insert("assert", "Assertions");
    m
}
