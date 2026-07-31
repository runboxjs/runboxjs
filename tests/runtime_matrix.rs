use runbox::error::{Result as RunboxResult, RunboxError};
use runbox::process::ProcessManager;
use runbox::runtime::bun::BunRuntime;
use runbox::runtime::git::GitRuntime;
use runbox::runtime::npm::PackageManagerRuntime;
use runbox::runtime::shell_builtins::ShellBuiltins;
use runbox::runtime::{ExecOutput, Runtime};
use runbox::shell::{Command, RuntimeTarget};
use runbox::vfs::Vfs;

fn has_system_bun() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok()
}

fn exec_line(line: &str, vfs: &mut Vfs, pm: &mut ProcessManager) -> RunboxResult<ExecOutput> {
    let cmd = Command::parse(line)?;
    match RuntimeTarget::detect(&cmd) {
        RuntimeTarget::Bun => BunRuntime.exec(&cmd, vfs, pm),
        RuntimeTarget::Git => GitRuntime.exec(&cmd, vfs, pm),
        RuntimeTarget::Npm => PackageManagerRuntime::npm().exec(&cmd, vfs, pm),
        RuntimeTarget::Pnpm => PackageManagerRuntime::pnpm().exec(&cmd, vfs, pm),
        RuntimeTarget::Yarn => PackageManagerRuntime::yarn().exec(&cmd, vfs, pm),
        RuntimeTarget::Shell => ShellBuiltins.exec(&cmd, vfs, pm),
        RuntimeTarget::Python | RuntimeTarget::Curl | RuntimeTarget::Unknown => {
            Err(RunboxError::Shell(format!(
                "{}: command not supported in test dispatcher",
                cmd.program
            )))
        }
    }
}

fn write_json(path: &str, value: serde_json::Value, vfs: &mut Vfs) {
    vfs.write(path, value.to_string().into_bytes())
        .expect("write JSON file");
}

#[test]
fn package_manager_install_matrix_creates_expected_locks_and_modules() {
    let matrix = [
        ("npm install", "/package-lock.json"),
        ("pnpm install", "/pnpm-lock.yaml"),
        ("yarn install", "/yarn.lock"),
        ("bun install", "/bun.lock"),
    ];

    for (command, lock_file) in matrix {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();

        write_json(
            "/package.json",
            serde_json::json!({
                "name": "matrix-app",
                "version": "1.0.0",
                "dependencies": { "nanoid": "^5.1.6" }
            }),
            &mut vfs,
        );

        let out = exec_line(command, &mut vfs, &mut pm).expect("install command should run");
        assert_eq!(out.exit_code, 0, "command failed: {command}");
        assert!(
            vfs.exists(lock_file),
            "{command} should create lock file {lock_file}"
        );
        assert!(
            vfs.exists("/node_modules/nanoid/package.json"),
            "{command} should create node_modules entry for nanoid"
        );
    }
}

#[test]
fn package_manager_run_start_matrix_executes_scripts() {
    let matrix = [
        "npm run start",
        "pnpm run start",
        "yarn run start",
        "bun run start",
    ];

    for command in matrix {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();

        write_json(
            "/package.json",
            serde_json::json!({
                "name": "script-matrix",
                "version": "1.0.0",
                "scripts": { "start": "node /index.js" }
            }),
            &mut vfs,
        );
        vfs.write("/index.js", b"console.log('SCRIPT_OK')".to_vec())
            .expect("write index");

        let out = exec_line(command, &mut vfs, &mut pm).expect("run command should succeed");
        assert_eq!(out.exit_code, 0, "script failed for {command}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("SCRIPT_OK"),
            "expected SCRIPT_OK in output for {command}, got: {stdout}"
        );
    }
}

#[test]
fn direct_node_command_is_supported() {
    let mut vfs = Vfs::new();
    let mut pm = ProcessManager::new();

    vfs.write("/index.js", b"console.log('NODE_DIRECT_OK')".to_vec())
        .expect("write index");

    let out = exec_line("node /index.js", &mut vfs, &mut pm).expect("node command should run");
    assert_eq!(out.exit_code, 0);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("NODE_DIRECT_OK"));
}

/// Tests that npm install creates functional stubs and require() works
/// even when the registry is not reachable (boa fallback with VFS modules).
#[test]
fn install_then_run_with_require_uses_stubs() {
    let mut vfs = Vfs::new();
    let mut pm = ProcessManager::new();

    // Setup package.json with lodash dependency
    write_json(
        "/package.json",
        serde_json::json!({
            "name": "stub-test",
            "version": "1.0.0",
            "dependencies": { "lodash": "^4.17.21" },
            "scripts": { "start": "node /index.js" }
        }),
        &mut vfs,
    );

    // User's code that uses require('lodash')
    vfs.write(
        "/index.js",
        b"var _ = require('lodash');\nconsole.log('REQUIRE_OK', typeof _);\n".to_vec(),
    )
    .expect("write index");

    // Run npm install — on native without registry, creates stubs
    let install_out = exec_line("npm install", &mut vfs, &mut pm).expect("npm install");
    assert_eq!(install_out.exit_code, 0, "npm install should succeed");

    // Verify stubs were created
    assert!(
        vfs.exists("/node_modules/lodash/package.json"),
        "lodash package.json should exist"
    );
    assert!(
        vfs.exists("/node_modules/lodash/index.js"),
        "lodash index.js stub should exist"
    );

    // Run the script — should find lodash via stub
    let run_out = exec_line("npm run start", &mut vfs, &mut pm).expect("npm run start");
    let stdout = String::from_utf8_lossy(&run_out.stdout);
    let stderr = String::from_utf8_lossy(&run_out.stderr);
    assert!(
        stdout.contains("REQUIRE_OK"),
        "require('lodash') should resolve via stub.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// Tests that require() works for locally installed packages in the boa fallback.
#[test]
fn require_resolves_vfs_module_in_boa_fallback() {
    let mut vfs = Vfs::new();
    let mut pm = ProcessManager::new();

    // Manually set up a local module in node_modules
    vfs.write(
        "/node_modules/mylib/package.json",
        br#"{"name":"mylib","version":"1.0.0","main":"index.js"}"#.to_vec(),
    )
    .expect("write mylib package.json");
    vfs.write(
        "/node_modules/mylib/index.js",
        b"module.exports = { greet: function() { return 'HELLO_FROM_MYLIB'; } };".to_vec(),
    )
    .expect("write mylib index.js");

    vfs.write(
        "/index.js",
        b"var lib = require('mylib');\nconsole.log(lib.greet());\n".to_vec(),
    )
    .expect("write index");

    let out = exec_line("node /index.js", &mut vfs, &mut pm).expect("node command");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // On systems with bun/node, this runs natively.
    // On systems without bun/node, this runs via boa with our require() polyfill.
    // Both should succeed.
    assert!(
        stdout.contains("HELLO_FROM_MYLIB"),
        "require('mylib') should resolve.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// Tests pnpm and yarn install + run start also work with stubs.
#[test]
fn pnpm_and_yarn_install_then_run_with_require() {
    for pm_name in ["pnpm", "yarn"] {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();

        write_json(
            "/package.json",
            serde_json::json!({
                "name": format!("{pm_name}-stub-test"),
                "version": "1.0.0",
                "dependencies": { "dayjs": "^1.11.10" },
                "scripts": { "start": "node /index.js" }
            }),
            &mut vfs,
        );

        vfs.write(
            "/index.js",
            b"var d = require('dayjs');\nconsole.log('DAYJS_OK', typeof d);\n".to_vec(),
        )
        .expect("write index");

        let install = exec_line(&format!("{pm_name} install"), &mut vfs, &mut pm)
            .unwrap_or_else(|e| panic!("{pm_name} install failed: {e}"));
        assert_eq!(install.exit_code, 0, "{pm_name} install failed");

        assert!(
            vfs.exists("/node_modules/dayjs/package.json"),
            "{pm_name}: dayjs package.json should exist"
        );

        let run = exec_line(&format!("{pm_name} run start"), &mut vfs, &mut pm)
            .unwrap_or_else(|e| panic!("{pm_name} run start failed: {e}"));
        let stdout = String::from_utf8_lossy(&run.stdout);
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(
            stdout.contains("DAYJS_OK"),
            "{pm_name}: require('dayjs') should resolve.\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

#[test]
fn modular_app_with_local_package_runs_in_bun_runtime() {
    if !has_system_bun() {
        // Native fallback JS engine does not emulate require() modules fully.
        return;
    }

    let mut vfs = Vfs::new();
    let mut pm = ProcessManager::new();

    vfs.write(
        "/node_modules/clsx/package.json",
        serde_json::json!({
            "name": "clsx",
            "version": "2.1.1",
            "main": "dist/clsx.js",
            "exports": { ".": { "default": { "default": "./dist/clsx.js" } } }
        })
        .to_string()
        .into_bytes(),
    )
    .expect("write clsx package json");
    vfs.write(
        "/node_modules/clsx/dist/clsx.js",
        b"module.exports = (...a) => a.filter(Boolean).join(' ');".to_vec(),
    )
    .expect("write clsx implementation");
    vfs.write("/lib/math.js", b"module.exports = (a,b) => a + b;".to_vec())
        .expect("write local module");
    vfs.write(
        "/index.js",
        br#"const clsx = require('clsx');
const sum = require('./lib/math.js');
console.log('MOD_OK', clsx('a', false && 'b', 'c'), sum(1,2));"#
            .to_vec(),
    )
    .expect("write app entrypoint");

    let out = exec_line("bun run /index.js", &mut vfs, &mut pm).expect("bun should execute");
    assert_eq!(out.exit_code, 0);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("MOD_OK a c 3"),
        "unexpected output: {stdout}"
    );
}

#[test]
fn local_http_server_script_starts_and_stops_cleanly() {
    if !has_system_bun() {
        return;
    }

    let mut vfs = Vfs::new();
    let mut pm = ProcessManager::new();

    vfs.write(
        "/index.js",
        br#"const http = require('http');
const server = http.createServer((req, res) => { res.end('ok'); });
server.listen(0, () => {
  const p = server.address().port;
  console.log('SERVER_OK', p);
  server.close(() => console.log('SERVER_CLOSED'));
});"#
            .to_vec(),
    )
    .expect("write server script");

    let out = exec_line("bun run /index.js", &mut vfs, &mut pm).expect("server script should run");
    assert_eq!(out.exit_code, 0);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("SERVER_OK"), "stdout: {stdout}");
    assert!(stdout.contains("SERVER_CLOSED"), "stdout: {stdout}");
}

#[test]
fn git_end_to_end_branch_merge_and_log_flow() {
    let mut vfs = Vfs::new();
    let mut pm = ProcessManager::new();

    assert_eq!(
        exec_line("git init", &mut vfs, &mut pm)
            .expect("git init")
            .exit_code,
        0
    );

    vfs.write("/app.txt", b"v1".to_vec()).expect("write v1");
    assert_eq!(
        exec_line("git add .", &mut vfs, &mut pm)
            .expect("git add")
            .exit_code,
        0
    );
    assert_eq!(
        exec_line(r#"git commit -m "initial""#, &mut vfs, &mut pm)
            .expect("git commit initial")
            .exit_code,
        0
    );

    assert_eq!(
        exec_line("git branch feature", &mut vfs, &mut pm)
            .expect("git branch")
            .exit_code,
        0
    );
    assert_eq!(
        exec_line("git checkout feature", &mut vfs, &mut pm)
            .expect("git checkout feature")
            .exit_code,
        0
    );

    vfs.write("/app.txt", b"v2".to_vec()).expect("write v2");
    assert_eq!(
        exec_line("git add .", &mut vfs, &mut pm)
            .expect("git add feature")
            .exit_code,
        0
    );
    assert_eq!(
        exec_line(r#"git commit -m "feature""#, &mut vfs, &mut pm)
            .expect("git commit feature")
            .exit_code,
        0
    );

    assert_eq!(
        exec_line("git checkout main", &mut vfs, &mut pm)
            .expect("git checkout main")
            .exit_code,
        0
    );
    let merge_out = exec_line("git merge feature", &mut vfs, &mut pm).expect("git merge");
    assert_eq!(merge_out.exit_code, 0);
    let merge_stdout = String::from_utf8_lossy(&merge_out.stdout);
    assert!(
        merge_stdout.contains("Fast-forward") || merge_stdout.contains("Already up to date"),
        "merge output: {merge_stdout}"
    );

    let log_out = exec_line("git log --oneline", &mut vfs, &mut pm).expect("git log");
    let log_stdout = String::from_utf8_lossy(&log_out.stdout);
    assert!(log_stdout.contains("initial"), "log output: {log_stdout}");
    assert!(log_stdout.contains("feature"), "log output: {log_stdout}");
}

#[test]
fn git_push_pull_and_remote_commands_smoke() {
    let mut vfs = Vfs::new();
    let mut pm = ProcessManager::new();

    exec_line("git init", &mut vfs, &mut pm).expect("git init");
    vfs.write("/readme.md", b"# repo".to_vec())
        .expect("write readme");
    exec_line("git add .", &mut vfs, &mut pm).expect("git add");
    exec_line(r#"git commit -m "seed""#, &mut vfs, &mut pm).expect("git commit");

    let pull_out = exec_line("git pull", &mut vfs, &mut pm).expect("git pull");
    assert_eq!(pull_out.exit_code, 0);
    assert!(
        String::from_utf8_lossy(&pull_out.stdout).contains("Already up to date"),
        "pull output: {}",
        String::from_utf8_lossy(&pull_out.stdout)
    );

    let push_out = exec_line("git push", &mut vfs, &mut pm).expect("git push");
    assert_eq!(push_out.exit_code, 1);
    assert!(
        String::from_utf8_lossy(&push_out.stderr)
            .contains("does not appear to be a git repository"),
        "push stderr: {}",
        String::from_utf8_lossy(&push_out.stderr)
    );

    assert_eq!(
        exec_line(
            "git remote add origin https://example.invalid/repo.git",
            &mut vfs,
            &mut pm
        )
        .expect("git remote add")
        .exit_code,
        0
    );
    let remotes = exec_line("git remote -v", &mut vfs, &mut pm).expect("git remote -v");
    assert!(
        String::from_utf8_lossy(&remotes.stdout).contains("origin"),
        "remote list output: {}",
        String::from_utf8_lossy(&remotes.stdout)
    );
    assert_eq!(
        exec_line("git remote remove origin", &mut vfs, &mut pm)
            .expect("git remote remove")
            .exit_code,
        0
    );
}
