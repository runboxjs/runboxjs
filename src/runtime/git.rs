use super::{ExecOutput, Runtime};
use crate::error::Result;
use crate::process::ProcessManager;
use crate::shell::Command;
use crate::vfs::Vfs;
use serde::{Deserialize, Serialize};
/// Runtime de Git — implementación en memoria sobre el VFS.
/// Local: init, add, commit, status, log, diff, branch, checkout, reset.
/// Red:   clone, fetch, pull (HTTP smart protocol via reqwest / Service Worker).
use std::collections::HashMap;

// ── Tipos de objetos Git ──────────────────────────────────────────────────────

fn sha1(data: &[u8]) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(data);
    h.digest().to_string()
}

fn blob_sha(content: &[u8]) -> String {
    let header = format!("blob {}\0", content.len());
    let mut data = header.into_bytes();
    data.extend_from_slice(content);
    sha1(&data)
}

fn commit_sha(content: &str) -> String {
    let header = format!("commit {}\0", content.len());
    let mut data = header.into_bytes();
    data.extend_from_slice(content.as_bytes());
    sha1(&data)
}

// ── Index (staging area) ──────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    /// path → sha1
    staged: HashMap<String, String>,
}

impl Index {
    fn load(vfs: &Vfs) -> Self {
        vfs.read("/.git/index")
            .ok()
            .and_then(|b| serde_json::from_slice(b).ok())
            .unwrap_or_default()
    }

    fn save(&self, vfs: &mut Vfs) -> Result<()> {
        let json = serde_json::to_vec(self).map_err(|e| {
            crate::error::RunboxError::Runtime(format!("index serialization failed: {e}"))
        })?;
        vfs.write("/.git/index", json)
    }
}

// ── Commit ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct GitCommit {
    sha: String,
    message: String,
    author: String,
    timestamp: String,
    parent: Option<String>,
    /// staged snapshot: path → sha1
    tree: HashMap<String, String>,
}

fn load_log(vfs: &Vfs) -> Vec<GitCommit> {
    vfs.read("/.git/log")
        .ok()
        .and_then(|b| serde_json::from_slice(b).ok())
        .unwrap_or_default()
}

fn save_log(vfs: &mut Vfs, log: &[GitCommit]) -> Result<()> {
    let json = serde_json::to_vec(log).map_err(|e| {
        crate::error::RunboxError::Runtime(format!("log serialization failed: {e}"))
    })?;
    vfs.write("/.git/log", json)
}

fn head_sha(vfs: &Vfs) -> Option<String> {
    let branch = current_branch(vfs)?;
    let ref_path = format!("/.git/refs/heads/{branch}");
    vfs.read(&ref_path)
        .ok()
        .map(|b| String::from_utf8_lossy(b).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn current_branch(vfs: &Vfs) -> Option<String> {
    vfs.read("/.git/HEAD").ok().and_then(|b| {
        let s = String::from_utf8_lossy(b).into_owned();
        s.strip_prefix("ref: refs/heads/")
            .map(|b| b.trim().to_string())
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn now_str() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S %z")
        .to_string()
}

#[cfg(target_arch = "wasm32")]
fn now_str() -> String {
    let date = js_sys::Date::new_0();
    let year = date.get_utc_full_year();
    let month = date.get_utc_month() + 1;
    let day = date.get_utc_date();
    let hours = date.get_utc_hours();
    let minutes = date.get_utc_minutes();
    let seconds = date.get_utc_seconds();
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02} +0000")
}

// ── Runtime ───────────────────────────────────────────────────────────────────

pub struct GitRuntime;

impl Runtime for GitRuntime {
    fn name(&self) -> &'static str {
        "git"
    }

    fn exec(&self, cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
        let sub = cmd.args.first().map(String::as_str).unwrap_or("");

        match sub {
            "init" => git_init(cmd, vfs, pm),
            "add" => git_add(cmd, vfs, pm),
            "commit" => git_commit(cmd, vfs, pm),
            "status" => git_status(cmd, vfs, pm),
            "log" => git_log(cmd, vfs, pm),
            "diff" => git_diff(cmd, vfs, pm),
            "branch" => git_branch(cmd, vfs, pm),
            "checkout" => git_checkout(cmd, vfs, pm),
            "merge" => git_merge(cmd, vfs, pm),
            "reset" => git_reset(cmd, vfs, pm),
            "clone" => git_clone(cmd, vfs, pm),
            "fetch" => git_fetch(cmd, vfs, pm),
            "pull" => git_pull(cmd, vfs, pm),
            "push" => git_push(cmd, vfs, pm),
            "remote" => git_remote(cmd, vfs, pm),
            "config" => git_config(cmd, vfs, pm),
            "" => Ok(err_out(
                "git: no subcommand given. Try: init add commit status log diff branch checkout",
            )),
            other => Ok(err_out(format!("git: unknown subcommand '{other}'"))),
        }
    }
}

// ── Implementaciones ──────────────────────────────────────────────────────────

fn git_init(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    let path = cmd.args.get(1).map(String::as_str).unwrap_or("/");
    let git = if path == "/" {
        "/.git".to_string()
    } else {
        format!("{path}/.git")
    };

    // Crear el directorio .git primero
    vfs.mkdir(&git)?;
    vfs.mkdir(&format!("{git}/refs"))?;
    vfs.mkdir(&format!("{git}/refs/heads"))?;
    vfs.mkdir(&format!("{git}/objects"))?;

    // Ahora escribir los archivos
    vfs.write(&format!("{git}/HEAD"), b"ref: refs/heads/main\n".to_vec())?;
    vfs.write(&format!("{git}/config"), default_git_config().into_bytes())?;
    vfs.write(
        &format!("{git}/description"),
        b"Unnamed repository\n".to_vec(),
    )?;

    let pid = pm.spawn("git", cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(format!("Initialized empty Git repository in {git}")))
}

fn git_add(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    if !vfs.exists("/.git/HEAD") {
        return Ok(err_out("fatal: not a git repository"));
    }

    let targets: Vec<&str> = cmd.args.iter().skip(1).map(String::as_str).collect();
    if targets.is_empty() {
        return Ok(err_out(
            "Nothing specified, nothing added.\nHint: git add . to stage all files",
        ));
    }

    let mut index = Index::load(vfs);
    let mut staged = vec![];

    let paths_to_add: Vec<String> = if targets.contains(&".") || targets.contains(&"--all") {
        // Recopilar todos los archivos del working tree (excluyendo .git)
        collect_files(vfs, "/")
            .into_iter()
            .filter(|p| !p.starts_with("/.git/"))
            .collect()
    } else {
        targets
            .iter()
            .map(|t| {
                if t.starts_with('/') {
                    t.to_string()
                } else {
                    format!("/{t}")
                }
            })
            .collect()
    };

    for path in paths_to_add {
        match vfs.read(&path) {
            Ok(content) => {
                let sha = blob_sha(content);
                // Store blob content in object store for later restore (reset --hard)
                let obj_path = format!("/.git/objects/{sha}");
                if !vfs.exists(&obj_path) {
                    let _ = vfs.write(&obj_path, content.to_vec());
                }
                index.staged.insert(path.clone(), sha);
                staged.push(path);
            }
            Err(_) => {
                // Si no existe, eliminarlo del index (git add de archivo borrado)
                if index.staged.remove(&path).is_some() {
                    staged.push(format!("{path} (deleted)"));
                }
            }
        }
    }

    index.save(vfs)?;
    let pid = pm.spawn("git", cmd.args.clone());
    pm.exit(pid, 0)?;

    if staged.is_empty() {
        Ok(ok_out(""))
    } else {
        Ok(ok_out(
            staged
                .iter()
                .map(|p| format!("staged: {p}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    }
}

fn git_commit(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    if !vfs.exists("/.git/HEAD") {
        return Ok(err_out("fatal: not a git repository"));
    }

    // Parsear -m "mensaje"
    let message = parse_flag_value(&cmd.args, "-m")
        .or_else(|| parse_flag_value(&cmd.args, "--message"))
        .unwrap_or_else(|| "chore: update".into());

    let index = Index::load(vfs);
    if index.staged.is_empty() {
        return Ok(ok_out("nothing to commit, working tree clean"));
    }

    let mut log = load_log(vfs);
    let parent = head_sha(vfs);

    // Generar contenido del commit con árbol determinista (orden por path)
    let timestamp = now_str();
    let mut tree_entries: Vec<_> = index.staged.iter().collect();
    tree_entries.sort_by_key(|(k, _)| k.as_str());
    let tree_str = tree_entries
        .iter()
        .map(|(path, sha)| format!("{path}\t{sha}"))
        .collect::<Vec<_>>()
        .join("\n");
    let tree_sha = sha1(tree_str.as_bytes());
    let commit_content = format!(
        "tree {tree}\nparent {parent}\nauthor RunBox <runbox@local> {ts}\n\n{msg}",
        tree = tree_sha,
        parent = parent.as_deref().unwrap_or(""),
        ts = timestamp,
        msg = message,
    );
    let sha = commit_sha(&commit_content);

    let commit = GitCommit {
        sha: sha.clone(),
        message: message.clone(),
        author: "RunBox User <runbox@local>".into(),
        timestamp,
        parent,
        tree: index.staged.clone(),
    };

    log.push(commit);
    save_log(vfs, &log)?;

    // Actualizar HEAD ref
    let branch = current_branch(vfs).unwrap_or_else(|| "main".into());
    vfs.write(
        &format!("/.git/refs/heads/{branch}"),
        format!("{sha}\n").into_bytes(),
    )?;
    vfs.write("/.git/COMMIT_EDITMSG", message.clone().into_bytes())?;

    // Limpiar index
    let empty = Index::default();
    empty.save(vfs)?;

    let pid = pm.spawn("git", cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(format!(
        "[{branch} {short}] {message}",
        short = &sha[..7]
    )))
}

fn git_status(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    if !vfs.exists("/.git/HEAD") {
        return Ok(err_out("fatal: not a git repository"));
    }

    let branch = current_branch(vfs).unwrap_or_else(|| "main".into());
    let mut lines = vec![format!("On branch {branch}")];

    let index = Index::load(vfs);
    let log = load_log(vfs);
    let last_tree = log.last().map(|c| &c.tree);

    // Archivos staged (en index pero no en HEAD)
    let mut staged_new = vec![];
    let mut staged_mod = vec![];
    let mut staged_del = vec![];
    for (path, sha) in &index.staged {
        match last_tree.and_then(|t| t.get(path)) {
            None => staged_new.push(path.as_str()),
            Some(prev) => {
                if prev != sha {
                    staged_mod.push(path.as_str())
                }
            }
        }
    }
    if let Some(tree) = last_tree {
        for path in tree.keys() {
            if !index.staged.contains_key(path) {
                staged_del.push(path.as_str());
            }
        }
    }

    // Archivos en working tree no staged
    let wt_files: Vec<String> = collect_files(vfs, "/")
        .into_iter()
        .filter(|p| !p.starts_with("/.git/"))
        .collect();

    let mut unstaged = vec![];
    let mut untracked = vec![];
    for path in &wt_files {
        if index.staged.contains_key(path) {
            // staged, verificar si el working tree difiere
            let wt_sha = vfs.read(path).map(blob_sha).unwrap_or_default();
            if index.staged.get(path).map(String::as_str) != Some(&wt_sha) {
                unstaged.push(path.as_str());
            }
        } else if last_tree.map(|t| !t.contains_key(path)).unwrap_or(true) {
            untracked.push(path.as_str());
        }
    }

    if staged_new.is_empty()
        && staged_mod.is_empty()
        && staged_del.is_empty()
        && unstaged.is_empty()
        && untracked.is_empty()
    {
        lines.push("nothing to commit, working tree clean".into());
    } else {
        if !staged_new.is_empty() || !staged_mod.is_empty() || !staged_del.is_empty() {
            lines.push("\nChanges to be committed:".into());
            for p in staged_new {
                lines.push(format!("  new file:   {p}"));
            }
            for p in staged_mod {
                lines.push(format!("  modified:   {p}"));
            }
            for p in staged_del {
                lines.push(format!("  deleted:    {p}"));
            }
        }
        if !unstaged.is_empty() {
            lines.push("\nChanges not staged for commit:".into());
            for p in unstaged {
                lines.push(format!("  modified:   {p}"));
            }
        }
        if !untracked.is_empty() {
            lines.push("\nUntracked files:".into());
            for p in untracked {
                lines.push(format!("  {p}"));
            }
        }
    }

    let pid = pm.spawn("git", cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(lines.join("\n")))
}

fn git_log(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    if !vfs.exists("/.git/HEAD") {
        return Ok(err_out("fatal: not a git repository"));
    }

    let log = load_log(vfs);
    if log.is_empty() {
        let pid = pm.spawn("git", cmd.args.clone());
        pm.exit(pid, 0)?;
        return Ok(ok_out("(no commits yet)"));
    }

    let oneline = cmd.args.iter().any(|a| a == "--oneline");
    let mut lines = vec![];

    for commit in log.iter().rev() {
        if oneline {
            lines.push(format!("{} {}", &commit.sha[..7], commit.message));
        } else {
            lines.push(format!("commit {}", commit.sha));
            lines.push(format!("Author: {}", commit.author));
            lines.push(format!("Date:   {}", commit.timestamp));
            lines.push(String::new());
            lines.push(format!("    {}", commit.message));
            lines.push(String::new());
        }
    }

    let pid = pm.spawn("git", cmd.args.clone());
    pm.exit(pid, 0)?;
    Ok(ok_out(lines.join("\n")))
}

fn git_diff(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    if !vfs.exists("/.git/HEAD") {
        return Ok(err_out("fatal: not a git repository"));
    }

    let staged_flag = cmd.args.iter().any(|a| a == "--staged" || a == "--cached");
    let index = Index::load(vfs);
    let log = load_log(vfs);
    let last_tree = log.last().map(|c| &c.tree);
    let mut output = vec![];

    if staged_flag {
        // diff entre HEAD y index
        if let Some(tree) = last_tree {
            for (path, staged_sha) in &index.staged {
                match tree.get(path) {
                    None => output.push(format!("diff --git a{path} b{path}\nnew file mode 100644\n--- /dev/null\n+++ b{path}")),
                    Some(head_sha) if head_sha != staged_sha => {
                        output.push(format!("diff --git a{path} b{path}\n--- a{path}\n+++ b{path}\n(modified)"));
                    }
                    _ => {}
                }
            }
        }
    } else {
        // diff entre index y working tree
        for (path, staged_sha) in &index.staged {
            if let Ok(content) = vfs.read(path) {
                let wt_sha = blob_sha(content);
                if &wt_sha != staged_sha {
                    // Reuse already-read content instead of a second VFS call
                    let old = String::from_utf8_lossy(content).into_owned();
                    output.push(format!(
                        "diff --git a{path} b{path}\n--- a{path}\n+++ b{path}\n{old}"
                    ));
                }
            }
        }
    }

    let pid = pm.spawn("git", cmd.args.clone());
    pm.exit(pid, 0)?;

    if output.is_empty() {
        Ok(ok_out(""))
    } else {
        Ok(ok_out(output.join("\n---\n")))
    }
}

fn git_branch(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    if !vfs.exists("/.git/HEAD") {
        return Ok(err_out("fatal: not a git repository"));
    }

    let delete = cmd.args.iter().any(|a| a == "-d" || a == "-D");
    let new_branch = cmd
        .args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .cloned();

    let pid = pm.spawn("git", cmd.args.clone());

    if let Some(name) = new_branch {
        if delete {
            vfs.remove(&format!("/.git/refs/heads/{name}"))?;
            pm.exit(pid, 0)?;
            return Ok(ok_out(format!("Deleted branch {name}")));
        }
        // Crear nueva rama apuntando al HEAD actual
        let sha = head_sha(vfs).unwrap_or_default();
        vfs.write(
            &format!("/.git/refs/heads/{name}"),
            format!("{sha}\n").into_bytes(),
        )?;
        pm.exit(pid, 0)?;
        return Ok(ok_out(format!("Created branch '{name}'")));
    }

    // Listar ramas
    let current = current_branch(vfs).unwrap_or_default();
    let refs_path = "/.git/refs/heads";
    let branches = vfs.list(refs_path).unwrap_or_default();
    let output = branches
        .iter()
        .map(|b| {
            if b == &current {
                format!("* {b}")
            } else {
                format!("  {b}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    pm.exit(pid, 0)?;
    Ok(ok_out(if output.is_empty() {
        format!("* {current} (no commits yet)")
    } else {
        output
    }))
}

fn git_checkout(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    if !vfs.exists("/.git/HEAD") {
        return Ok(err_out("fatal: not a git repository"));
    }

    let create_flag = cmd.args.iter().any(|a| a == "-b");
    let target = cmd
        .args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .cloned();

    let target = match target {
        Some(t) => t,
        None => return Ok(err_out("git checkout: specify a branch")),
    };

    let pid = pm.spawn("git", cmd.args.clone());

    if create_flag {
        let sha = head_sha(vfs).unwrap_or_default();
        vfs.write(
            &format!("/.git/refs/heads/{target}"),
            format!("{sha}\n").into_bytes(),
        )?;
    } else if !vfs.exists(&format!("/.git/refs/heads/{target}")) {
        pm.exit(pid, 1)?;
        return Ok(err_out(format!(
            "error: pathspec '{target}' did not match any branch"
        )));
    }

    vfs.write(
        "/.git/HEAD",
        format!("ref: refs/heads/{target}\n").into_bytes(),
    )?;
    pm.exit(pid, 0)?;
    Ok(ok_out(format!("Switched to branch '{target}'")))
}

fn git_reset(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    if !vfs.exists("/.git/HEAD") {
        return Ok(err_out("fatal: not a git repository"));
    }

    let hard = cmd.args.iter().any(|a| a == "--hard");
    let soft = cmd.args.iter().any(|a| a == "--soft");

    let mut index = Index::load(vfs);
    let pid = pm.spawn("git", cmd.args.clone());

    if hard {
        // Restaurar working tree y el index al último commit
        let log = load_log(vfs);
        if let Some(last) = log.last() {
            // Restaurar archivos desde el object store creado en git add
            for (path, sha) in &last.tree {
                let obj_path = format!("/.git/objects/{sha}");
                if let Ok(blob) = vfs.read(&obj_path) {
                    let _ = vfs.write(path, blob.to_vec());
                }
            }
            index.staged = last.tree.clone();
            index.save(vfs)?;
        }
        pm.exit(pid, 0)?;
        return Ok(ok_out("HEAD is now at last commit (hard reset)"));
    }

    if !soft {
        // --mixed (default): limpiar index pero mantener working tree
        index.staged.clear();
        index.save(vfs)?;
    }

    pm.exit(pid, 0)?;
    Ok(ok_out("Unstaged changes after reset"))
}

fn git_merge(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    if !vfs.exists("/.git/HEAD") {
        return Ok(err_out("fatal: not a git repository"));
    }

    let target = match cmd.args.get(1) {
        Some(t) => t.clone(),
        None => return Ok(err_out("git merge: specify a branch to merge")),
    };

    let current = current_branch(vfs).unwrap_or_else(|| "main".into());
    let pid = pm.spawn("git", cmd.args.clone());

    if target == current {
        pm.exit(pid, 0)?;
        return Ok(ok_out("Already up to date."));
    }

    let target_ref = format!("/.git/refs/heads/{target}");
    if !vfs.exists(&target_ref) {
        pm.exit(pid, 1)?;
        return Ok(err_out(format!("merge: branch '{target}' not found")));
    }

    let target_sha = vfs
        .read(&target_ref)
        .ok()
        .map(|b| String::from_utf8_lossy(b).trim().to_string())
        .unwrap_or_default();
    let current_sha = head_sha(vfs).unwrap_or_default();

    if target_sha.is_empty() {
        pm.exit(pid, 1)?;
        return Ok(err_out(format!(
            "merge: branch '{target}' not found or has no commits"
        )));
    }

    if target_sha == current_sha {
        pm.exit(pid, 0)?;
        return Ok(ok_out("Already up to date."));
    }

    let log = load_log(vfs);
    let target_commit = match log.iter().find(|c| c.sha == target_sha) {
        Some(c) => c,
        None => {
            pm.exit(pid, 1)?;
            return Ok(err_out(format!("merge: commit {target_sha} not found")));
        }
    };

    // Simulación de merge fast-forward: mover HEAD actual al commit de la rama objetivo.
    vfs.write(
        &format!("/.git/refs/heads/{current}"),
        format!("{target_sha}\n").into_bytes(),
    )?;

    let mut index = Index::load(vfs);
    index.staged = target_commit.tree.clone();
    index.save(vfs)?;

    pm.exit(pid, 0)?;
    Ok(ok_out(format!(
        "Updating {old}..{new}\nFast-forward\n",
        old = &current_sha[..7.min(current_sha.len())],
        new = &target_sha[..7.min(target_sha.len())],
    )))
}

// ── Operaciones de red ────────────────────────────────────────────────────────

fn git_clone(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    #[cfg(target_arch = "wasm32")]
    let _ = vfs;

    let url = match cmd.args.get(1) {
        Some(u) => u.clone(),
        None => return Ok(err_out("git clone: specify a URL")),
    };
    let dest = cmd
        .args
        .get(2)
        .cloned()
        .unwrap_or_else(|| repo_name_from_url(&url));

    let pid = pm.spawn("git", cmd.args.clone());

    #[cfg(not(target_arch = "wasm32"))]
    {
        match http_git_clone(&url, &dest, vfs) {
            Ok(msg) => {
                pm.exit(pid, 0)?;
                return Ok(ok_out(msg));
            }
            Err(e) => {
                pm.exit(pid, 1)?;
                return Ok(err_out(format!("git clone failed: {e}")));
            }
        }
    }

    // WASM — necesita Service Worker con acceso a red
    #[allow(unreachable_code)]
    pm.exit(pid, 0)?;
    Ok(ok_out(format!(
        "Cloning into '{dest}'...\n\
         [runbox] Network operations in WASM require Service Worker with CORS proxy.\n\
         Configure sw_proxy_url in RunboxInstance to enable git clone.\n"
    )))
}

fn git_fetch(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    let remote = cmd.args.get(1).map(String::as_str).unwrap_or("origin");
    let remote_url = read_remote_url(vfs, remote);
    #[cfg(target_arch = "wasm32")]
    let _ = &remote_url;

    let pid = pm.spawn("git", cmd.args.clone());

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(url) = remote_url {
        match http_git_ls_refs(&url) {
            Ok(refs) => {
                for (sha, refname) in &refs {
                    let path = format!("/.git/{refname}");
                    vfs.write(&path, format!("{sha}\n").into_bytes())?;
                }
                pm.exit(pid, 0)?;
                return Ok(ok_out(format!(
                    "From {url}\n   fetched {} refs\n",
                    refs.len()
                )));
            }
            Err(e) => {
                pm.exit(pid, 1)?;
                return Ok(err_out(format!("git fetch: {e}")));
            }
        }
    }

    pm.exit(pid, 0)?;
    Ok(ok_out(format!(
        "Fetching {remote}...\n(network operations require native build or Service Worker)\n"
    )))
}

fn git_pull(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    // fetch + merge
    let fetch_cmd = Command {
        program: "git".into(),
        args: vec!["fetch".into()],
        env: vec![],
        stdin: None,
        redirect: None,
    };
    let fetch_out = git_fetch(&fetch_cmd, vfs, pm)?;

    if fetch_out.exit_code != 0 {
        let pid = pm.spawn("git", cmd.args.clone());
        pm.exit(pid, 1)?;
        return Ok(ExecOutput {
            stdout: fetch_out.stdout,
            stderr: fetch_out.stderr,
            exit_code: 1,
        });
    }

    let pid = pm.spawn("git", cmd.args.clone());
    pm.exit(pid, 0)?;

    let msg = format!(
        "{}Already up to date.\n",
        String::from_utf8_lossy(&fetch_out.stdout)
    );
    Ok(ok_out(msg))
}

fn git_push(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    let remote = cmd.args.get(1).map(String::as_str).unwrap_or("origin");
    let branch = cmd
        .args
        .get(2)
        .cloned()
        .or_else(|| current_branch(vfs))
        .unwrap_or_else(|| "main".into());

    let url = match read_remote_url(vfs, remote) {
        Some(u) => u,
        None => {
            let pid = pm.spawn("git", cmd.args.clone());
            pm.exit(pid, 1)?;
            return Ok(err_out(format!(
                "fatal: '{remote}' does not appear to be a git repository\n\
                 Use: git remote add {remote} <url>"
            )));
        }
    };

    let creds = GitCredentials::load(vfs);
    #[cfg(target_arch = "wasm32")]
    let _ = &creds;
    let pid = pm.spawn("git", cmd.args.clone());

    #[cfg(not(target_arch = "wasm32"))]
    {
        match http_git_push(&url, &branch, vfs, &creds) {
            Ok(msg) => {
                pm.exit(pid, 0)?;
                Ok(ok_out(msg))
            }
            Err(e) => {
                pm.exit(pid, 1)?;
                Ok(err_out(format!("error: failed to push: {e}")))
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        pm.exit(pid, 0)?;
        Ok(ok_out(format!(
            "Pushing to {url} ({branch})...\n\
             (git push requires native build or authenticated Service Worker)\n"
        )))
    }
}

// ── Credenciales y git config ─────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GitCredentials {
    pub username: Option<String>,
    pub email: Option<String>,
    pub token: Option<String>, // Personal Access Token (GitHub, GitLab, etc.)
    pub password: Option<String>, // HTTP Basic password (fallback)
}

impl GitCredentials {
    pub fn load(vfs: &Vfs) -> Self {
        vfs.read("/.git/credentials")
            .ok()
            .and_then(|b| serde_json::from_slice(b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, vfs: &mut Vfs) -> Result<()> {
        let json = serde_json::to_vec(self).map_err(|e| {
            crate::error::RunboxError::Runtime(format!("credentials serialization failed: {e}"))
        })?;
        vfs.write("/.git/credentials", json)
    }

    /// Authorization header value para HTTP.
    pub fn auth_header(&self) -> Option<String> {
        if let Some(token) = &self.token {
            return Some(format!("token {token}"));
        }
        if let (Some(user), Some(pass)) = (&self.username, &self.password) {
            use std::fmt::Write;
            let mut cred = String::new();
            let _ = write!(cred, "{user}:{pass}");
            return Some(format!("Basic {}", base64_encode(cred.as_bytes())));
        }
        None
    }
}

/// Mínimo base64 sin dependencias externas.
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };
        out.push(TABLE[b0 >> 2] as char);
        out.push(TABLE[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[b2 & 0x3f] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn git_config(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    let args: Vec<&str> = cmd.args.iter().map(String::as_str).collect();
    let pid = pm.spawn("git", cmd.args.clone());

    // git config [--global] user.name "Nombre"
    let (key, value) = match args.as_slice() {
        [_, "--global", k, v] => (*k, Some(*v)),
        [_, "--global", k] => (*k, None),
        [_, k, v] => (*k, Some(*v)),
        [_, k] => (*k, None),
        _ => {
            pm.exit(pid, 1)?;
            return Ok(err_out("git config: invalid syntax"));
        }
    };

    let mut creds = GitCredentials::load(vfs);

    if let Some(val) = value {
        match key {
            "user.name" => creds.username = Some(val.into()),
            "user.email" => creds.email = Some(val.into()),
            "user.token" | "credential.token" => creds.token = Some(val.into()),
            "user.password" => creds.password = Some(val.into()),
            _ => {
                pm.exit(pid, 0)?;
                return Ok(ok_out(""));
            }
        }
        creds.save(vfs)?;
        pm.exit(pid, 0)?;
        Ok(ok_out(""))
    } else {
        let val = match key {
            "user.name" => creds.username.as_deref().unwrap_or("").to_string(),
            "user.email" => creds.email.as_deref().unwrap_or("").to_string(),
            "user.token" => creds.token.as_deref().unwrap_or("").to_string(),
            _ => String::new(),
        };
        pm.exit(pid, 0)?;
        Ok(ok_out(val))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn http_git_push(
    url: &str,
    branch: &str,
    vfs: &Vfs,
    creds: &GitCredentials,
) -> crate::error::Result<String> {
    // Paso 1: descubrir capacidades del servidor
    let disc_url = format!(
        "{}/info/refs?service=git-receive-pack",
        url.trim_end_matches('/')
    );

    let mut req = reqwest::blocking::Client::new().get(&disc_url);
    if let Some(auth) = creds.auth_header() {
        req = req.header("Authorization", auth);
    }
    let resp = req
        .send()
        .map_err(|e| crate::error::RunboxError::Runtime(format!("push discovery: {e}")))?;

    match resp.status().as_u16() {
        200 | 201 => {}
        401 => {
            return Err(crate::error::RunboxError::Runtime(
                "Authentication failed.\n\
             Set credentials with:\n  git config user.token <your-token>"
                    .into(),
            ));
        }
        403 => {
            return Err(crate::error::RunboxError::Runtime(
                "Permission denied (403). Check that your token has write access.".into(),
            ));
        }
        code => {
            return Err(crate::error::RunboxError::Runtime(format!(
                "push failed: HTTP {code}"
            )));
        }
    }

    // Paso 2: obtener el SHA del HEAD local
    let log = load_log(vfs);
    let local_sha = log.last().map(|c| c.sha.clone()).unwrap_or_default();
    if local_sha.is_empty() {
        return Err(crate::error::RunboxError::Runtime(
            "Nothing to push — no commits in local repository".into(),
        ));
    }

    let remote_ref = format!("refs/heads/{branch}");
    let remote_sha = vfs
        .read(&format!("/.git/{remote_ref}"))
        .map(|b| String::from_utf8_lossy(b).trim().to_string())
        .unwrap_or_else(|_| "0".repeat(40));

    // Paso 3: enviar el pack (simplificado — solo el ref update)
    // Un pack completo requiere serializar todos los objetos git.
    // Esta implementación envía el ref-update pkt-line como demostración.
    let push_url = format!("{}/git-receive-pack", url.trim_end_matches('/'));
    let pkt = format!(
        "{old} {new} {refname}\0 report-status side-band-64k\n",
        old = remote_sha,
        new = local_sha,
        refname = remote_ref,
    );
    let pkt_line = format!("{:04x}{pkt}", pkt.len() + 4);
    let body = format!("{pkt_line}0000").into_bytes();

    let mut req = reqwest::blocking::Client::new()
        .post(&push_url)
        .header("Content-Type", "application/x-git-receive-pack-request")
        .body(body);
    if let Some(auth) = creds.auth_header() {
        req = req.header("Authorization", auth);
    }
    let resp = req
        .send()
        .map_err(|e| crate::error::RunboxError::Runtime(format!("push failed: {e}")))?;

    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        Ok(format!(
            "To {url}\n   {old}..{new}  {branch} -> {branch}\n",
            old = &remote_sha[..7.min(remote_sha.len())],
            new = &local_sha[..7.min(local_sha.len())],
        ))
    } else {
        Err(crate::error::RunboxError::Runtime(format!(
            "push rejected: HTTP {status}"
        )))
    }
}

fn git_remote(cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
    let sub = cmd.args.get(1).map(String::as_str).unwrap_or("");
    let pid = pm.spawn("git", cmd.args.clone());

    match sub {
        "add" => {
            let name = cmd.args.get(2);
            let url = cmd.args.get(3);
            if let (Some(n), Some(u)) = (name, url) {
                save_remote_url(vfs, n, u)?;
                pm.exit(pid, 0)?;
                return Ok(ok_out(""));
            }
            pm.exit(pid, 1)?;
            Ok(err_out("git remote add <name> <url>"))
        }
        "remove" | "rm" => {
            if let Some(name) = cmd.args.get(2) {
                let _ = vfs.remove(&format!("/.git/config.remotes.{name}"));
            }
            pm.exit(pid, 0)?;
            Ok(ok_out(""))
        }
        "-v" | "" => {
            let remotes = list_remotes(vfs);
            pm.exit(pid, 0)?;
            Ok(ok_out(remotes))
        }
        other => {
            pm.exit(pid, 1)?;
            Ok(err_out(format!("git remote: unknown subcommand '{other}'")))
        }
    }
}

// ── HTTP git protocol (nativo) ────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn http_git_ls_refs(url: &str) -> crate::error::Result<Vec<(String, String)>> {
    use crate::network::http_get;
    let discovery_url = format!(
        "{}/info/refs?service=git-upload-pack",
        url.trim_end_matches('/')
    );
    let resp = http_get(&discovery_url)?;

    if resp.status != 200 {
        return Err(crate::error::RunboxError::Runtime(format!(
            "git ls-refs: HTTP {}",
            resp.status
        )));
    }

    parse_pkt_line_refs(resp.body_str())
}

#[cfg(not(target_arch = "wasm32"))]
fn http_git_clone(url: &str, dest: &str, vfs: &mut Vfs) -> crate::error::Result<String> {
    // 1. Descubrir refs
    let refs = http_git_ls_refs(url)?;
    let head_sha = refs
        .iter()
        .find(|(_, r)| r == "HEAD" || r == "refs/heads/main" || r == "refs/heads/master")
        .map(|(s, _)| s.clone())
        .unwrap_or_default();

    // 2. Inicializar repo en el VFS
    vfs.write(
        &format!("/{dest}/.git/HEAD"),
        b"ref: refs/heads/main\n".to_vec(),
    )?;
    for (sha, refname) in &refs {
        vfs.write(
            &format!("/{dest}/.git/{refname}"),
            format!("{sha}\n").into_bytes(),
        )?;
    }

    // 3. Guardar la URL del remote
    save_remote_url_path(vfs, &format!("/{dest}"), "origin", url)?;

    Ok(format!(
        "Cloning into '{dest}'...\nremote: Enumerating objects: {count}\nDone.\nHEAD is at {short}\n",
        count = refs.len(),
        short = &head_sha[..7.min(head_sha.len())],
    ))
}

/// Parsea el formato pkt-line de git smart HTTP.
#[allow(dead_code)]
fn parse_pkt_line_refs(body: &str) -> crate::error::Result<Vec<(String, String)>> {
    let mut refs = vec![];
    let mut lines = body.lines();

    // Saltar el primer pkt-line que es el service announcement
    let _ = lines.next(); // "# service=git-upload-pack"
    let _ = lines.next(); // flush pkt (0000)

    for line in lines {
        // Cada línea: 4 hex chars (longitud) + sha1 + ' ' + refname + flags
        if line.len() < 45 {
            continue;
        }
        let content = &line[4..]; // saltar los 4 bytes de longitud
        let parts: Vec<&str> = content.splitn(2, ' ').collect();
        if parts.len() == 2 {
            let sha = parts[0].trim().to_string();
            let refname = parts[1].split('\0').next().unwrap_or("").trim().to_string();
            if sha.len() == 40 && !refname.is_empty() {
                refs.push((sha, refname));
            }
        }
    }
    Ok(refs)
}

// ── Remote URL helpers ────────────────────────────────────────────────────────

fn save_remote_url(vfs: &mut Vfs, name: &str, url: &str) -> crate::error::Result<()> {
    vfs.write(
        &format!("/.git/config.remotes.{name}"),
        url.as_bytes().to_vec(),
    )
}

#[allow(dead_code)]
fn save_remote_url_path(
    vfs: &mut Vfs,
    prefix: &str,
    name: &str,
    url: &str,
) -> crate::error::Result<()> {
    vfs.write(
        &format!("{prefix}/.git/config.remotes.{name}"),
        url.as_bytes().to_vec(),
    )
}

fn read_remote_url(vfs: &Vfs, name: &str) -> Option<String> {
    vfs.read(&format!("/.git/config.remotes.{name}"))
        .ok()
        .map(|b| String::from_utf8_lossy(b).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn list_remotes(vfs: &Vfs) -> String {
    // Listar todas las entradas /.git/config.remotes.*
    vfs.list("/.git")
        .unwrap_or_default()
        .iter()
        .filter(|e| e.starts_with("config.remotes."))
        .map(|e| {
            let name = e.trim_start_matches("config.remotes.");
            let url = vfs
                .read(&format!("/.git/{e}"))
                .map(|b| String::from_utf8_lossy(b).trim().to_string())
                .unwrap_or_default();
            format!("{name}\t{url} (fetch)\n{name}\t{url} (push)")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn repo_name_from_url(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("repo")
        .trim_end_matches(".git")
        .to_string()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ok_out(text: impl Into<String>) -> ExecOutput {
    ExecOutput {
        stdout: text.into().into_bytes(),
        stderr: vec![],
        exit_code: 0,
    }
}

fn err_out(text: impl Into<String>) -> ExecOutput {
    ExecOutput {
        stdout: vec![],
        stderr: text.into().into_bytes(),
        exit_code: 1,
    }
}

fn default_git_config() -> String {
    "[core]\n\trepositoryformatversion = 0\n\tfilemode = false\n\tbare = false\n".into()
}

fn parse_flag_value(args: &[String], flag: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == flag {
            return iter.next().cloned();
        }
        // --message=value
        if let Some(stripped) = a.strip_prefix(&format!("{flag}=")) {
            return Some(stripped.to_string());
        }
    }
    None
}

fn collect_files(vfs: &Vfs, path: &str) -> Vec<String> {
    let mut result = vec![];
    let entries = match vfs.list(path) {
        Ok(e) => e,
        Err(_) => return result,
    };
    for entry in entries {
        let full = if path == "/" {
            format!("/{entry}")
        } else {
            format!("{path}/{entry}")
        };
        if vfs.read(&full).is_ok() {
            result.push(full);
        } else {
            result.extend(collect_files(vfs, &full));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessManager;

    fn pm() -> ProcessManager {
        ProcessManager::new()
    }
    fn cmd(s: &str) -> Command {
        Command::parse(s).unwrap()
    }

    #[test]
    fn init_and_commit() {
        let mut vfs = Vfs::new();
        let mut pm = pm();
        let rt = GitRuntime;

        let out = rt.exec(&cmd("git init"), &mut vfs, &mut pm).unwrap();
        assert_eq!(out.exit_code, 0);
        assert!(vfs.exists("/.git/HEAD"));

        vfs.write("/src/main.ts", b"console.log('hi')".to_vec())
            .unwrap();
        rt.exec(&cmd("git add ."), &mut vfs, &mut pm).unwrap();

        let out = rt
            .exec(&cmd(r#"git commit -m "initial commit""#), &mut vfs, &mut pm)
            .unwrap();
        assert_eq!(out.exit_code, 0);
        assert!(String::from_utf8_lossy(&out.stdout).contains("initial commit"));
    }

    #[test]
    fn status_shows_untracked() {
        let mut vfs = Vfs::new();
        let mut pm = pm();
        let rt = GitRuntime;

        rt.exec(&cmd("git init"), &mut vfs, &mut pm).unwrap();
        vfs.write("/index.ts", b"code".to_vec()).unwrap();

        let out = rt.exec(&cmd("git status"), &mut vfs, &mut pm).unwrap();
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.contains("Untracked") || s.contains("index.ts"));
    }

    #[test]
    fn branch_create_and_checkout() {
        let mut vfs = Vfs::new();
        let mut pm = pm();
        let rt = GitRuntime;

        rt.exec(&cmd("git init"), &mut vfs, &mut pm).unwrap();
        vfs.write("/file.ts", b"x".to_vec()).unwrap();
        rt.exec(&cmd("git add ."), &mut vfs, &mut pm).unwrap();
        rt.exec(&cmd(r#"git commit -m "first""#), &mut vfs, &mut pm)
            .unwrap();

        rt.exec(&cmd("git branch feature"), &mut vfs, &mut pm)
            .unwrap();
        let out = rt
            .exec(&cmd("git checkout feature"), &mut vfs, &mut pm)
            .unwrap();
        assert_eq!(out.exit_code, 0);
        assert_eq!(current_branch(&vfs).unwrap(), "feature");
    }

    #[test]
    fn merge_fast_forward_updates_head() {
        let mut vfs = Vfs::new();
        let mut pm = pm();
        let rt = GitRuntime;

        rt.exec(&cmd("git init"), &mut vfs, &mut pm).unwrap();
        vfs.write("/file.txt", b"v1".to_vec()).unwrap();
        rt.exec(&cmd("git add ."), &mut vfs, &mut pm).unwrap();
        rt.exec(&cmd(r#"git commit -m "base""#), &mut vfs, &mut pm)
            .unwrap();

        rt.exec(&cmd("git branch feature"), &mut vfs, &mut pm)
            .unwrap();
        rt.exec(&cmd("git checkout feature"), &mut vfs, &mut pm)
            .unwrap();
        vfs.write("/file.txt", b"v2".to_vec()).unwrap();
        rt.exec(&cmd("git add ."), &mut vfs, &mut pm).unwrap();
        rt.exec(&cmd(r#"git commit -m "feature""#), &mut vfs, &mut pm)
            .unwrap();

        rt.exec(&cmd("git checkout main"), &mut vfs, &mut pm)
            .unwrap();
        let out = rt
            .exec(&cmd("git merge feature"), &mut vfs, &mut pm)
            .unwrap();
        assert_eq!(out.exit_code, 0);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("Fast-forward") || stdout.contains("Already up to date"));
    }

    #[test]
    fn push_without_remote_fails() {
        let mut vfs = Vfs::new();
        let mut pm = pm();
        let rt = GitRuntime;

        rt.exec(&cmd("git init"), &mut vfs, &mut pm).unwrap();
        vfs.write("/file.txt", b"v1".to_vec()).unwrap();
        rt.exec(&cmd("git add ."), &mut vfs, &mut pm).unwrap();
        rt.exec(&cmd(r#"git commit -m "base""#), &mut vfs, &mut pm)
            .unwrap();

        let out = rt.exec(&cmd("git push"), &mut vfs, &mut pm).unwrap();
        assert_eq!(out.exit_code, 1);
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("does not appear to be a git repository")
        );
    }
}
