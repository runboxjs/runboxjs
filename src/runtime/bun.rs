use super::{ExecOutput, Runtime};
use crate::error::{Result, RunboxError};
use crate::process::ProcessManager;
use crate::shell::Command;
use crate::vfs::Vfs;
/// Runtime de Bun.
/// Nativo: intenta ejecutar el binario `bun` del sistema usando el VFS materializado.
/// WASM: delega en el callback JS `runbox_js_eval` provisto por el host.
#[cfg(target_arch = "wasm32")]
use js_sys;

pub struct BunRuntime;

impl Runtime for BunRuntime {
    fn name(&self) -> &'static str {
        "bun"
    }

    fn exec(&self, cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
        if matches!(cmd.program.as_str(), "node" | "nodejs" | "tsx" | "ts-node") {
            let mut args = vec!["run".to_string()];
            args.extend(cmd.args.clone());
            let shimmed = Command {
                program: "bun".into(),
                args,
                env: cmd.env.clone(),
                stdin: None,
                redirect: None,
            };
            return bun_run(&shimmed, vfs, pm);
        }

        let subcommand = cmd.args.first().map(String::as_str).unwrap_or("");

        match subcommand {
            "run" => bun_run(cmd, vfs, pm),
            "install" | "i" => bun_install(cmd, vfs, pm),
            "add" => bun_add(cmd, vfs, pm),
            "build" => bun_build(cmd, vfs, pm),
            "test" => bun_test(cmd, vfs, pm),
            "repl" => Ok(err_out("bun repl: not supported in sandbox")),
            "" => Ok(err_out("bun: specify a subcommand")),
            other => Err(RunboxError::Runtime(format!(
                "bun: unknown subcommand '{other}'"
            ))),
        }
    }
}

// ── Subcomandos ───────────────────────────────────────────────────────────────

fn bun_run(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    let file = cmd
        .args
        .get(1)
        .ok_or_else(|| RunboxError::Runtime("bun run requires a file or script name".into()))?;

    // Si parece un script npm (sin extensión y sin /) buscar en package.json
    if !file.contains('.') && !file.contains('/') {
        return run_package_script(file, cmd, vfs, pm);
    }

    let path = if file.starts_with('/') {
        file.clone()
    } else {
        format!("/{file}")
    };
    if !vfs.exists(&path) {
        return Err(RunboxError::NotFound(path));
    }

    spawn_bun(cmd, vfs, pm, &path)
}

fn bun_install(_cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    // Delegar al PackageManagerRuntime con soporte real de package.json
    use crate::runtime::npm::PackageManagerRuntime;
    let install_cmd = Command::parse("bun install")?;
    PackageManagerRuntime::bun_via_npm().exec(&install_cmd, vfs, pm)
}

fn bun_add(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    use crate::runtime::npm::PackageManagerRuntime;
    let mut args = vec!["add".to_string()];
    args.extend(cmd.args.iter().skip(1).cloned());
    let add_cmd = Command {
        program: "bun".into(),
        args,
        env: vec![],
        stdin: None,
        redirect: None,
    };
    PackageManagerRuntime::bun_via_npm().exec(&add_cmd, vfs, pm)
}

fn bun_build(cmd: &Command, _vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    let pid = pm.spawn("bun", cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(
        "[bun build] bundling... (native bun required for full execution)\n",
    ))
}

fn bun_test(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    // Buscar archivos *.test.ts / *.spec.ts en el VFS
    let test_files = find_test_files(vfs);
    if test_files.is_empty() {
        let pid = pm.spawn("bun", cmd.args.clone());
        pm.exit(pid, 0)?;
        return Ok(ok_out("No test files found (*.test.ts / *.spec.ts)\n"));
    }
    spawn_bun(cmd, vfs, pm, &test_files[0])
}

// ── Ejecutor ──────────────────────────────────────────────────────────────────

/// Intenta ejecutar el binario `bun` del sistema; si no está disponible
/// usa boa_engine (native) o js_sys::eval (WASM).
fn spawn_bun(
    cmd: &Command,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
    file_path: &str,
) -> Result<ExecOutput> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // 1. Intentar bun del sistema
        if let Ok(output) = try_spawn_system_bun(cmd, vfs) {
            let pid = pm.spawn("bun", cmd.args.clone());
            pm.exit(pid, output.exit_code)?;
            return Ok(output);
        }

        // 2. Intentar node del sistema (tiene require() nativo)
        if !file_path.is_empty()
            && let Ok(output) = try_spawn_system_node(cmd, vfs, file_path)
        {
            let pid = pm.spawn("node", cmd.args.clone());
            pm.exit(pid, output.exit_code)?;
            return Ok(output);
        }

        // 3. Fallback: boa_engine con require() polyfill desde VFS
        if !file_path.is_empty()
            && let Ok(source) = vfs.read(file_path)
        {
            let src = String::from_utf8_lossy(source).into_owned();
            let is_ts = file_path.ends_with(".ts") || file_path.ends_with(".tsx");
            let preamble = build_require_preamble(vfs);
            let full_src = format!("{preamble}\n{src}");
            let out = super::js_engine::run(&full_src, is_ts);
            let pid = pm.spawn("bun", cmd.args.clone());
            pm.exit(pid, out.exit_code)?;
            return Ok(ExecOutput {
                stdout: out.stdout.into_bytes(),
                stderr: out.stderr.into_bytes(),
                exit_code: out.exit_code,
            });
        }
    }

    // WASM: js_sys::eval vía el motor del browser
    #[cfg(target_arch = "wasm32")]
    if !file_path.is_empty() {
        if let Ok(source) = vfs.read(file_path) {
            let src = String::from_utf8_lossy(source).into_owned();
            let is_ts = file_path.ends_with(".ts") || file_path.ends_with(".tsx");

            // Precargar node_modules del VFS en globalThis para que require() funcione
            // con cualquier paquete instalado (react-icons, lodash, etc.)
            preload_vfs_modules(vfs);

            let out = super::js_engine::run(&src, is_ts);
            let pid = pm.spawn("bun", cmd.args.clone());
            pm.exit(pid, out.exit_code)?;
            return Ok(ExecOutput {
                stdout: out.stdout.into_bytes(),
                stderr: out.stderr.into_bytes(),
                exit_code: out.exit_code,
            });
        }
    }

    let pid = pm.spawn("bun", cmd.args.clone());
    pm.exit(pid, 1)?;
    let file = cmd.args.get(1).map(String::as_str).unwrap_or("?");
    Ok(err_out(format!("bun: could not execute '{file}'")))
}

#[cfg(not(target_arch = "wasm32"))]
fn try_spawn_system_bun(cmd: &Command, vfs: &mut Vfs) -> std::io::Result<ExecOutput> {
    use crate::network::materialize_vfs;
    use std::process::Command as SysCmd;
    use tempfile::TempDir;

    let tmp = TempDir::new()?;
    materialize_vfs(vfs, tmp.path()).unwrap_or_default();

    let output = SysCmd::new("bun")
        .args(&cmd.args)
        .current_dir(tmp.path())
        .output()?;

    Ok(ExecOutput {
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.status.code().unwrap_or(1),
    })
}

/// Intenta ejecutar con `node` del sistema, materializando el VFS a un tmpdir.
/// Soporta `require()` nativo y `node_modules/`.
#[cfg(not(target_arch = "wasm32"))]
fn try_spawn_system_node(
    cmd: &Command,
    vfs: &mut Vfs,
    file_path: &str,
) -> std::io::Result<ExecOutput> {
    use crate::network::materialize_vfs;
    use std::process::Command as SysCmd;
    use tempfile::TempDir;

    // Verificar que node existe
    SysCmd::new("node").arg("--version").output()?;

    let tmp = TempDir::new()?;
    materialize_vfs(vfs, tmp.path()).unwrap_or_default();

    // Construir la ruta del archivo relativa al tmpdir
    let rel_path = file_path.trim_start_matches('/');
    let output = SysCmd::new("node")
        .arg(rel_path)
        .args(cmd.args.iter().skip(2)) // args después del filename
        .current_dir(tmp.path())
        .output()?;

    Ok(ExecOutput {
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.status.code().unwrap_or(1),
    })
}

fn run_package_script(
    script: &str,
    cmd: &Command,
    vfs: &mut Vfs,
    pm: &mut ProcessManager,
) -> Result<ExecOutput> {
    use crate::runtime::npm::PackageManagerRuntime;
    let mut args = vec!["run".to_string(), script.to_string()];
    args.extend(cmd.args.iter().skip(2).cloned());
    let run_cmd = Command {
        program: "bun".into(),
        args,
        env: cmd.env.clone(),
        stdin: None,
        redirect: None,
    };
    PackageManagerRuntime::bun_via_npm().exec(&run_cmd, vfs, pm)
}

/// Construye un preamble JS con require() polyfill y módulos del VFS
/// para que boa_engine pueda resolver require('lodash') etc.
#[cfg(not(target_arch = "wasm32"))]
fn build_require_preamble(vfs: &Vfs) -> String {
    let mut modules: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    // Recolectar archivos del proyecto (raíz del VFS)
    collect_require_files(vfs, "/", "/", &mut modules, 0);

    // Recolectar node_modules
    if let Ok(pkg_names) = vfs.list("/node_modules") {
        for pkg in &pkg_names {
            if pkg.starts_with('@') {
                let scope_root = format!("/node_modules/{pkg}");
                if let Ok(scoped) = vfs.list(&scope_root) {
                    for s in &scoped {
                        collect_require_files(
                            vfs,
                            &format!("{scope_root}/{s}"),
                            &format!("{scope_root}/{s}"),
                            &mut modules,
                            0,
                        );
                    }
                }
            } else {
                collect_require_files(
                    vfs,
                    &format!("/node_modules/{pkg}"),
                    &format!("/node_modules/{pkg}"),
                    &mut modules,
                    0,
                );
            }
        }
    }

    let json = serde_json::to_string(&modules).unwrap_or("{}".into());

    // Polyfill require() que resuelve desde el mapa de módulos
    format!(
        r#"var __vfs = {json};
var __cache = {{}};
function require(name) {{
  if (__cache[name] !== undefined) return __cache[name];
  var nm = '/node_modules/';
  var pkgKey = nm + name + '/package.json';
  if (__vfs[pkgKey]) {{
    try {{
      var pkg = JSON.parse(__vfs[pkgKey]);
      var main = (pkg.main || 'index.js').replace(/^\.\//, '');
      var entry = nm + name + '/' + main;
      var tries = [entry, entry + '.js', entry + '/index.js'];
      for (var i = 0; i < tries.length; i++) {{
        if (__vfs[tries[i]] !== undefined) {{
          var m = {{ exports: {{}} }};
          __cache[name] = m.exports;
          (new Function('module','exports','require', __vfs[tries[i]]))(m, m.exports, require);
          __cache[name] = m.exports;
          return m.exports;
        }}
      }}
    }} catch(e) {{}}
  }}
  var candidates = [
    nm + name + '/index.js',
    nm + name + '/index.cjs',
    nm + name + '.js',
    '/' + name,
    '/' + name + '.js',
    '/' + name + '/index.js',
    name
  ];
  for (var j = 0; j < candidates.length; j++) {{
    if (__vfs[candidates[j]] !== undefined) {{
      var m2 = {{ exports: {{}} }};
      __cache[name] = m2.exports;
      try {{
        (new Function('module','exports','require', __vfs[candidates[j]]))(m2, m2.exports, require);
      }} catch(e2) {{}}
      __cache[name] = m2.exports;
      return m2.exports;
    }}
  }}
  __cache[name] = {{}};
  return {{}};
}}
var module = {{ exports: {{}} }};
var exports = module.exports;
"#
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_require_files(
    vfs: &Vfs,
    dir: &str,
    _root: &str,
    out: &mut std::collections::HashMap<String, String>,
    depth: usize,
) {
    if depth > 6 || out.len() > 800 {
        return;
    }
    let entries = match vfs.list(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in &entries {
        if entry.starts_with('.')
            || *entry == "node_modules"
            || *entry == "__tests__"
            || *entry == "test"
            || *entry == "tests"
        {
            continue;
        }
        let full = if dir == "/" {
            format!("/{entry}")
        } else {
            format!("{dir}/{entry}")
        };
        if let Ok(bytes) = vfs.read(&full) {
            let ext = entry.rsplit('.').next().unwrap_or("");
            if !matches!(ext, "js" | "cjs" | "mjs" | "json") {
                continue;
            }
            if bytes.len() > 512_000 {
                continue;
            }
            if let Ok(content) = std::str::from_utf8(bytes) {
                out.insert(full.clone(), content.to_string());
            }
        } else {
            // Es un directorio
            collect_require_files(vfs, &full, _root, out, depth + 1);
        }
    }
}

fn find_test_files(vfs: &Vfs) -> Vec<String> {
    let mut found = vec![];
    collect_tests(vfs, "/", &mut found);
    found
}

fn collect_tests(vfs: &Vfs, path: &str, out: &mut Vec<String>) {
    let entries = match vfs.list(path) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries {
        let full = if path == "/" {
            format!("/{entry}")
        } else {
            format!("{path}/{entry}")
        };
        if vfs.read(&full).is_ok() {
            if entry.contains(".test.") || entry.contains(".spec.") {
                out.push(full);
            }
        } else {
            collect_tests(vfs, &full, out);
        }
    }
}

/// Serializa archivos del proyecto y paquetes npm en globalThis.__vfs_modules.
/// Cada paquete se evalúa en su propio eval() independiente para que el fallo
/// de un paquete grande (ej. react-icons) no impida cargar los demás.
#[cfg(target_arch = "wasm32")]
fn preload_vfs_modules(vfs: &crate::vfs::Vfs) {
    // Asegurar que __vfs_modules existe
    let _ = js_sys::eval("if(!globalThis.__vfs_modules)globalThis.__vfs_modules={};");

    // Inyectar lista de deps de package.json para mejor diagnóstico en require()
    if let Ok(pkg_bytes) = vfs.read("/package.json") {
        if let Ok(pkg_str) = std::str::from_utf8(pkg_bytes) {
            let script = format!(
                r#"try {{
                    var __pkg = {pkg_str};
                    globalThis.__runbox_pkg_deps = Object.assign({{}}, __pkg.dependencies || {{}}, __pkg.devDependencies || {{}});
                }} catch(e) {{}}"#
            );
            let _ = js_sys::eval(&script);
        }
    }

    // 1. Archivos del proyecto — siempre pequeños, eval propio
    {
        let mut project: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        collect_project_files(vfs, "/", &mut project);
        eval_into_vfs_modules(&project);
    }

    // 2. Un eval por paquete npm — aísla fallos de paquetes grandes
    if let Ok(pkg_names) = vfs.list("/node_modules") {
        for pkg_name in pkg_names {
            if pkg_name.starts_with('@') {
                // Scoped package: @scope/package — scan inner directory
                let scope_root = format!("/node_modules/{pkg_name}");
                if let Ok(scoped_pkgs) = vfs.list(&scope_root) {
                    for scoped_name in scoped_pkgs {
                        let full_name = format!("{pkg_name}/{scoped_name}");
                        let pkg_root = format!("/node_modules/{full_name}");
                        let mut pkg_files: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();
                        collect_npm_package(vfs, &pkg_root, &full_name, &mut pkg_files);
                        eval_into_vfs_modules(&pkg_files);
                    }
                }
            } else {
                let pkg_root = format!("/node_modules/{pkg_name}");
                let mut pkg_files: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                collect_npm_package(vfs, &pkg_root, &pkg_name, &mut pkg_files);
                eval_into_vfs_modules(&pkg_files);
            }
        }
    }
}

/// Escapa U+2028 y U+2029 en JSON para evitar SyntaxError en eval().
/// Estos caracteres son terminadores de línea en JS (pre-ES2019) y provocan
/// que eval() falle aunque el JSON sea válido.
#[cfg(target_arch = "wasm32")]
fn escape_json_for_eval(json: &str) -> String {
    // Fast path: la mayoría de archivos no contienen estos caracteres
    if !json.contains('\u{2028}') && !json.contains('\u{2029}') {
        return json.to_string();
    }
    json.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Hace `Object.assign(globalThis.__vfs_modules, map)` via eval.
/// Si el batch eval falla (JSON grande o contenido problemático),
/// hace eval por clave individual como fallback.
#[cfg(target_arch = "wasm32")]
fn eval_into_vfs_modules(map: &std::collections::HashMap<String, String>) {
    if map.is_empty() {
        return;
    }
    // Intentar todo junto primero (más rápido)
    if let Ok(json) = serde_json::to_string(map) {
        let safe_json = escape_json_for_eval(&json);
        let script = format!("Object.assign(globalThis.__vfs_modules,{safe_json});");
        if js_sys::eval(&script).is_ok() {
            return;
        }
    }
    // Si falla el batch, hacer eval por clave individual
    for (key, value) in map {
        if let (Ok(k_json), Ok(v_json)) = (serde_json::to_string(key), serde_json::to_string(value))
        {
            let safe_v = escape_json_for_eval(&v_json);
            let script = format!("globalThis.__vfs_modules[{k_json}]={safe_v};");
            let _ = js_sys::eval(&script);
        }
    }
}

/// Carga los archivos relevantes de un único paquete npm:
/// - package.json (para resolver el entry point)
/// - el archivo main/index y sus dependencias directas dentro del paquete
/// Evita cargar miles de archivos de paquetes grandes como react-icons.
#[cfg(target_arch = "wasm32")]
fn collect_npm_package(
    vfs: &crate::vfs::Vfs,
    pkg_root: &str,
    pkg_name: &str,
    out: &mut std::collections::HashMap<String, String>,
) {
    // ALWAYS pre-load package.json and the main entry file first, before the
    // recursive walk. Packages like lodash have 600+ files — the MAX_FILES cap
    // would be hit before reaching lodash.js alphabetically, causing require()
    // to fail even when the package was successfully downloaded.
    let pkg_json_path = format!("{pkg_root}/package.json");
    let main_entry = if let Ok(bytes) = vfs.read(&pkg_json_path) {
        if let Ok(content) = std::str::from_utf8(bytes) {
            out.insert(format!("{pkg_name}/package.json"), content.to_string());
            serde_json::from_str::<serde_json::Value>(content)
                .ok()
                .and_then(|pj| {
                    pj.get("main")
                        .or_else(|| pj.get("module"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim_start_matches("./").to_string())
                })
                .unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Pre-load the main entry file (e.g. "lodash.js") regardless of file count.
    if !main_entry.is_empty() {
        let main_path = format!("{pkg_root}/{main_entry}");
        if let Ok(bytes) = vfs.read(&main_path) {
            if bytes.len() <= 2_000_000 {
                if let Ok(content) = std::str::from_utf8(bytes) {
                    out.insert(format!("{pkg_name}/{main_entry}"), content.to_string());
                }
            }
        }
    }

    // Recursively load remaining files from the package directory tree.
    // This ensures that internal require() calls within packages (like
    // react requiring './cjs/react.production.min.js') can find their targets.
    collect_npm_package_recursive(vfs, pkg_root, pkg_root, pkg_name, out, 0);
}

/// Recursively walks the package directory tree and loads all .js/.cjs/.mjs/.json
/// files into the VFS module map. Caps recursion depth to avoid infinite loops
/// and limits total files per package to prevent memory blowup for huge packages.
#[cfg(target_arch = "wasm32")]
fn collect_npm_package_recursive(
    vfs: &crate::vfs::Vfs,
    pkg_root: &str,
    current_dir: &str,
    pkg_name: &str,
    out: &mut std::collections::HashMap<String, String>,
    depth: usize,
) {
    // Safety limits: max 8 levels deep, max 500 files per package
    const MAX_DEPTH: usize = 8;
    const MAX_FILES: usize = 500;
    // 1.5 MB limit — must accommodate lodash.js (~540KB), d3.js (~550KB), etc.
    const MAX_FILE_SIZE: usize = 1_500_000;

    if depth > MAX_DEPTH || out.len() > MAX_FILES {
        return;
    }

    // Determine the package's main entry file so we never skip it
    let main_entry = {
        let pkg_json_path = format!("{pkg_root}/package.json");
        vfs.read(&pkg_json_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
            .and_then(|pj| {
                pj.get("main")
                    .or_else(|| pj.get("module"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim_start_matches("./").to_string())
            })
            .unwrap_or_default()
    };

    let entries = match vfs.list(current_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        // Skip irrelevant files and directories
        if entry.starts_with('.')
            || entry == "node_modules"
            || entry == "__tests__"
            || entry == "test"
            || entry == "tests"
            || entry == "docs"
            || entry == "doc"
            || entry == "examples"
            || entry == "example"
            || entry == ".github"
        {
            continue;
        }

        let full_path = format!("{current_dir}/{entry}");

        if let Ok(bytes) = vfs.read(&full_path) {
            // It's a file — check extension
            let ext = entry.rsplit('.').next().unwrap_or("");

            // Skip non-useful files
            if matches!(
                ext,
                "wasm"
                    | "map"
                    | "md"
                    | "txt"
                    | "ts"
                    | "tsx"
                    | "d"
                    | "flow"
                    | "lock"
                    | "yml"
                    | "yaml"
                    | "toml"
                    | "log"
                    | "png"
                    | "jpg"
                    | "gif"
                    | "svg"
                    | "ico"
                    | "woff"
                    | "woff2"
                    | "ttf"
                    | "eot"
                    | "css"
            ) {
                continue;
            }

            // Skip .d.ts files
            if entry.ends_with(".d.ts") || entry.ends_with(".d.mts") || entry.ends_with(".d.cts") {
                continue;
            }

            if !matches!(ext, "js" | "mjs" | "cjs" | "json") {
                continue;
            }

            // Check if this is the package's main entry file
            let rel = full_path
                .strip_prefix(&format!("{pkg_root}/"))
                .unwrap_or(&entry);
            let is_main_entry = !main_entry.is_empty() && rel == main_entry;

            // Skip large files UNLESS it's the main entry (lodash.js ~540KB, d3.js ~550KB)
            if bytes.len() > MAX_FILE_SIZE || (!is_main_entry && bytes.len() > 512_000) {
                continue;
            }

            if let Ok(content) = std::str::from_utf8(bytes) {
                let key = format!("{pkg_name}/{rel}");
                out.insert(key, content.to_string());
            }

            if out.len() > MAX_FILES {
                return;
            }
        } else {
            // It's a directory — recurse
            collect_npm_package_recursive(vfs, pkg_root, &full_path, pkg_name, out, depth + 1);
        }
    }
}

/// Carga archivos locales del proyecto (fuera de node_modules) para que
/// require('./components/Foo') funcione en el eval del sandbox.
/// Guarda cada archivo con dos claves: `path/file.js` y `./path/file.js`.
#[cfg(target_arch = "wasm32")]
fn collect_project_files(
    vfs: &crate::vfs::Vfs,
    path: &str,
    out: &mut std::collections::HashMap<String, String>,
) {
    let entries = match vfs.list(path) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in &entries {
        // Saltar node_modules y archivos ocultos
        if entry == "node_modules" || entry.starts_with('.') {
            continue;
        }

        let full = if path == "/" {
            format!("/{entry}")
        } else {
            format!("{path}/{entry}")
        };

        if let Ok(bytes) = vfs.read(&full) {
            let ext = entry.rsplit('.').next().unwrap_or("");
            if matches!(ext, "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "json") {
                if let Ok(content) = std::str::from_utf8(bytes) {
                    // Clave sin slash inicial: "components/Foo.js"
                    let bare = full.trim_start_matches('/').to_string();
                    out.insert(bare.clone(), content.to_string());
                    // Clave con ./ para require desde el directorio raíz: "./components/Foo.js"
                    out.insert(format!("./{bare}"), content.to_string());
                }
            }
        } else {
            // Es un directorio — recursión
            collect_project_files(vfs, &full, out);
        }
    }
}

fn ok_out(s: impl AsRef<str>) -> ExecOutput {
    ExecOutput {
        stdout: s.as_ref().as_bytes().to_vec(),
        stderr: vec![],
        exit_code: 0,
    }
}

fn err_out(s: impl AsRef<str>) -> ExecOutput {
    ExecOutput {
        stdout: vec![],
        stderr: s.as_ref().as_bytes().to_vec(),
        exit_code: 1,
    }
}
