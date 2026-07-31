use super::{ExecOutput, Runtime};
use crate::error::{Result, RunboxError};
use crate::process::ProcessManager;
use crate::shell::{Command, ShellState};
use crate::vfs::Vfs;

pub struct ShellBuiltins;

impl Runtime for ShellBuiltins {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn exec(&self, cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput> {
        let mut state = ShellState::default();
        self.exec_with_state(cmd, vfs, pm, &mut state)
    }
}

impl ShellBuiltins {
    pub fn exec_with_state(
        &self,
        cmd: &Command,
        vfs: &mut Vfs,
        pm: &mut ProcessManager,
        state: &mut ShellState,
    ) -> Result<ExecOutput> {
        match cmd.program.as_str() {
            "echo" => self.cmd_echo(cmd),
            "pwd" => Ok(ok(format!("{}\n", state.cwd))),
            "ls" => self.cmd_ls(cmd, vfs, state),
            "cat" => self.cmd_cat(cmd, vfs, state),
            "mkdir" => self.cmd_mkdir(cmd, vfs, state),
            "rm" => self.cmd_rm(cmd, vfs, state),
            "touch" => self.cmd_touch(cmd, vfs, state),
            "cp" => self.cmd_cp(cmd, vfs, state),
            "mv" => self.cmd_mv(cmd, vfs, state),
            "cd" => self.cmd_cd(cmd, state),
            "grep" => self.cmd_grep(cmd, vfs, state),
            "head" => self.cmd_head(cmd, vfs, state),
            "tail" => self.cmd_tail(cmd, vfs, state),
            "wc" => self.cmd_wc(cmd, vfs, state),
            "sort" => self.cmd_sort(cmd, vfs, state),
            "uniq" => self.cmd_uniq(cmd, vfs, state),
            "tee" => self.cmd_tee(cmd, vfs, state),
            "cut" => self.cmd_cut(cmd, vfs, state),
            "tr" => self.cmd_tr(cmd),
            "ps" => self.cmd_ps(pm),
            "kill" => self.cmd_kill(cmd, pm),
            "sleep" => Ok(ok("")),
            "env" => self.cmd_env(state),
            "export" => self.cmd_export(cmd, state),
            "unset" => self.cmd_unset(cmd, state),
            "which" => self.cmd_which(cmd),
            "clear" => Ok(ok("\x1b[2J\x1b[H")),
            "date" => Ok(ok("Thu Jan  1 00:00:00 UTC 1970\n")),
            "basename" => self.cmd_basename(cmd),
            "dirname" => self.cmd_dirname(cmd),
            "printf" => self.cmd_printf(cmd),
            "true" => Ok(ExecOutput {
                stdout: vec![],
                stderr: vec![],
                exit_code: 0,
            }),
            "false" => Ok(ExecOutput {
                stdout: vec![],
                stderr: vec![],
                exit_code: 1,
            }),
            "test" | "[" => self.cmd_test(cmd, vfs, state),
            "chmod" | "chown" => Ok(ok("")),
            "uname" => self.cmd_uname(cmd),
            "find" => self.cmd_find(cmd, vfs, state),
            "stat" => self.cmd_stat(cmd, vfs, state),
            other => Err(RunboxError::Shell(format!("{other}: command not found"))),
        }
    }

    fn cmd_echo(&self, cmd: &Command) -> Result<ExecOutput> {
        let mut no_newline = false;
        let mut interpret_escapes = false;
        let mut args: Vec<&str> = cmd.args.iter().map(String::as_str).collect();
        loop {
            match args.first() {
                Some(&"-n") => {
                    no_newline = true;
                    args.remove(0);
                }
                Some(&"-e") => {
                    interpret_escapes = true;
                    args.remove(0);
                }
                Some(&"-ne") | Some(&"-en") => {
                    no_newline = true;
                    interpret_escapes = true;
                    args.remove(0);
                }
                _ => break,
            }
        }
        let text = args.join(" ");
        let text = if interpret_escapes {
            interpret_escape_sequences(&text)
        } else {
            text
        };
        let output = if no_newline {
            text
        } else {
            format!("{text}\n")
        };
        Ok(ok(output))
    }

    fn cmd_ls(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let args = &cmd.args;
        let show_long = args
            .iter()
            .any(|a| a == "-l" || a == "-la" || a == "-al" || a == "-lh" || a == "-lah");
        let show_all = show_long || args.iter().any(|a| a == "-a" || a == "-la" || a == "-al");
        let path_arg = args.iter().find(|a| !a.starts_with('-'));
        let path = path_arg.map(String::as_str).unwrap_or(&state.cwd);
        let path = state.resolve(path);

        let mut entries = vfs.list(&path).unwrap_or_default();
        entries.sort();

        if !show_all {
            entries.retain(|e| !e.starts_with('.'));
        }
        entries.retain(|e| e != ".runbox_dir");

        let mut output = String::new();
        for entry in &entries {
            let full = format!("{}/{}", path.trim_end_matches('/'), entry);
            let is_dir = vfs.is_dir(&full);
            if show_long {
                let size = vfs.stat(&full).map(|m| m.size).unwrap_or(0);
                let perms = if is_dir { "drwxr-xr-x" } else { "-rw-r--r--" };
                let name = if is_dir {
                    format!("{}/", entry)
                } else {
                    entry.clone()
                };
                output.push_str(&format!("{perms} {size:>8} {name}\n"));
            } else {
                let name = if is_dir {
                    format!("{}/", entry)
                } else {
                    entry.clone()
                };
                output.push_str(&format!("{name}\n"));
            }
        }
        Ok(ok(output))
    }

    fn cmd_cat(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let file_args: Vec<&String> = cmd.args.iter().filter(|a| !a.starts_with('-')).collect();
        if file_args.is_empty() {
            let data = cmd.stdin.clone().unwrap_or_default();
            return Ok(ExecOutput {
                stdout: data,
                stderr: vec![],
                exit_code: 0,
            });
        }
        let mut out = vec![];
        for file in file_args {
            let path = state.resolve(file);
            let bytes = vfs.read(&path)?.to_vec();
            out.extend_from_slice(&bytes);
        }
        Ok(ExecOutput {
            stdout: out,
            stderr: vec![],
            exit_code: 0,
        })
    }

    fn cmd_mkdir(&self, cmd: &Command, vfs: &mut Vfs, state: &ShellState) -> Result<ExecOutput> {
        let make_parents = cmd.args.iter().any(|a| a == "-p");
        let paths: Vec<&String> = cmd.args.iter().filter(|a| !a.starts_with('-')).collect();
        if paths.is_empty() {
            return Err(RunboxError::Shell("mkdir: missing operand".into()));
        }
        for p in paths {
            let resolved = state.resolve(p);
            if make_parents {
                let parts: Vec<&str> = resolved.trim_start_matches('/').split('/').collect();
                let mut cur = String::new();
                for part in &parts {
                    cur = format!("{cur}/{part}");
                    let placeholder = format!("{cur}/.runbox_dir");
                    if !vfs.exists(&placeholder) {
                        vfs.write(&placeholder, vec![])?;
                    }
                }
            } else {
                vfs.write(&format!("{resolved}/.runbox_dir"), vec![])?;
            }
        }
        Ok(ok(""))
    }

    fn cmd_rm(&self, cmd: &Command, vfs: &mut Vfs, state: &ShellState) -> Result<ExecOutput> {
        let recursive = cmd
            .args
            .iter()
            .any(|a| a == "-r" || a == "-rf" || a == "-fr");
        let paths: Vec<&String> = cmd.args.iter().filter(|a| !a.starts_with('-')).collect();
        if paths.is_empty() {
            return Err(RunboxError::Shell("rm: missing operand".into()));
        }
        for p in paths {
            let resolved = state.resolve(p);
            if recursive {
                let all = vfs.all_file_paths();
                let prefix = if resolved.ends_with('/') {
                    resolved.clone()
                } else {
                    format!("{resolved}/")
                };
                for path in &all {
                    if path == &resolved || path.starts_with(&prefix) {
                        let _ = vfs.remove(path);
                    }
                }
            } else {
                vfs.remove(&resolved)?;
            }
        }
        Ok(ok(""))
    }

    fn cmd_touch(&self, cmd: &Command, vfs: &mut Vfs, state: &ShellState) -> Result<ExecOutput> {
        let paths: Vec<&String> = cmd.args.iter().filter(|a| !a.starts_with('-')).collect();
        if paths.is_empty() {
            return Err(RunboxError::Shell("touch: missing operand".into()));
        }
        for p in paths {
            let resolved = state.resolve(p);
            if !vfs.exists(&resolved) {
                vfs.write(&resolved, vec![])?;
            }
        }
        Ok(ok(""))
    }

    fn cmd_cp(&self, cmd: &Command, vfs: &mut Vfs, state: &ShellState) -> Result<ExecOutput> {
        let args: Vec<&String> = cmd.args.iter().filter(|a| !a.starts_with('-')).collect();
        if args.len() < 2 {
            return Err(RunboxError::Shell("cp: missing operand".into()));
        }
        let src = state.resolve(args[0]);
        let dst = state.resolve(args[1]);
        if vfs.is_dir(&src) {
            let all = vfs.all_file_paths();
            let prefix = format!("{src}/");
            for path in all {
                if path.starts_with(&prefix) {
                    let rel = &path[src.len()..];
                    let dst_path = format!("{dst}{rel}");
                    let data = vfs.read(&path)?.to_vec();
                    vfs.write(&dst_path, data)?;
                }
            }
        } else {
            let data = vfs.read(&src)?.to_vec();
            vfs.write(&dst, data)?;
        }
        Ok(ok(""))
    }

    fn cmd_mv(&self, cmd: &Command, vfs: &mut Vfs, state: &ShellState) -> Result<ExecOutput> {
        self.cmd_cp(cmd, vfs, state)?;
        let args: Vec<&String> = cmd.args.iter().filter(|a| !a.starts_with('-')).collect();
        if !args.is_empty() {
            let src = state.resolve(args[0]);
            let rm_cmd = Command {
                program: "rm".to_string(),
                args: vec!["-r".to_string(), src],
                env: vec![],
                stdin: None,
                redirect: None,
            };
            self.cmd_rm(&rm_cmd, vfs, state)?;
        }
        Ok(ok(""))
    }

    fn cmd_cd(&self, cmd: &Command, state: &mut ShellState) -> Result<ExecOutput> {
        let path = cmd.args.first().map(String::as_str).unwrap_or("~");
        let resolved = state.resolve(path);
        state.set_cwd(&resolved);
        Ok(ok(""))
    }

    fn cmd_grep(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let case_insensitive = cmd.args.iter().any(|a| a == "-i");
        let show_line_nums = cmd.args.iter().any(|a| a == "-n");
        let invert = cmd.args.iter().any(|a| a == "-v");
        let count_only = cmd.args.iter().any(|a| a == "-c");
        let non_flag_args: Vec<&String> = cmd.args.iter().filter(|a| !a.starts_with('-')).collect();
        if non_flag_args.is_empty() {
            return Err(RunboxError::Shell("grep: missing pattern".into()));
        }
        let pattern = non_flag_args[0].as_str();
        let input = if non_flag_args.len() > 1 {
            let path = state.resolve(non_flag_args[1]);
            vfs.read(&path)?.to_vec()
        } else {
            cmd.stdin.clone().unwrap_or_default()
        };
        let text = String::from_utf8_lossy(&input);
        let mut count = 0usize;
        let mut output = String::new();
        for (idx, line) in text.lines().enumerate() {
            let line_to_match = if case_insensitive {
                line.to_lowercase()
            } else {
                line.to_string()
            };
            let pat_to_match = if case_insensitive {
                pattern.to_lowercase()
            } else {
                pattern.to_string()
            };
            let matches = line_to_match.contains(&pat_to_match);
            let include = if invert { !matches } else { matches };
            if include {
                count += 1;
                if !count_only {
                    if show_line_nums {
                        output.push_str(&format!("{}:{}\n", idx + 1, line));
                    } else {
                        output.push_str(line);
                        output.push('\n');
                    }
                }
            }
        }
        if count_only {
            output = format!("{count}\n");
        }
        let exit_code = if count == 0 { 1 } else { 0 };
        Ok(ExecOutput {
            stdout: output.into_bytes(),
            stderr: vec![],
            exit_code,
        })
    }

    fn cmd_head(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let n = parse_n_flag(&cmd.args, 10);
        let input = get_text_input(cmd, vfs, state)?;
        let text = String::from_utf8_lossy(&input);
        let output: String = text.lines().take(n).map(|l| format!("{l}\n")).collect();
        Ok(ok(output))
    }

    fn cmd_tail(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let n = parse_n_flag(&cmd.args, 10);
        let input = get_text_input(cmd, vfs, state)?;
        let text = String::from_utf8_lossy(&input);
        let lines: Vec<&str> = text.lines().collect();
        let start = lines.len().saturating_sub(n);
        let output: String = lines[start..].iter().map(|l| format!("{l}\n")).collect();
        Ok(ok(output))
    }

    fn cmd_wc(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let lines_only = cmd.args.iter().any(|a| a == "-l");
        let words_only = cmd.args.iter().any(|a| a == "-w");
        let bytes_only = cmd.args.iter().any(|a| a == "-c");
        let input = get_text_input(cmd, vfs, state)?;
        let text = String::from_utf8_lossy(&input);
        let line_count = text.lines().count();
        let word_count = text.split_whitespace().count();
        let byte_count = input.len();
        let output = if lines_only {
            format!("{line_count}\n")
        } else if words_only {
            format!("{word_count}\n")
        } else if bytes_only {
            format!("{byte_count}\n")
        } else {
            format!("{line_count} {word_count} {byte_count}\n")
        };
        Ok(ok(output))
    }

    fn cmd_sort(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let reverse = cmd.args.iter().any(|a| a == "-r");
        let input = get_text_input(cmd, vfs, state)?;
        let text = String::from_utf8_lossy(&input);
        let mut lines: Vec<&str> = text.lines().collect();
        lines.sort();
        if reverse {
            lines.reverse();
        }
        let output: String = lines.iter().map(|l| format!("{l}\n")).collect();
        Ok(ok(output))
    }

    fn cmd_uniq(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let input = get_text_input(cmd, vfs, state)?;
        let text = String::from_utf8_lossy(&input);
        let mut output = String::new();
        let mut prev: Option<String> = None;
        for line in text.lines() {
            if prev.as_deref() != Some(line) {
                output.push_str(line);
                output.push('\n');
                prev = Some(line.to_string());
            }
        }
        Ok(ok(output))
    }

    fn cmd_tee(&self, cmd: &Command, vfs: &mut Vfs, state: &ShellState) -> Result<ExecOutput> {
        let input = cmd.stdin.clone().unwrap_or_default();
        if let Some(file) = cmd.args.iter().find(|a| !a.starts_with('-')) {
            let path = state.resolve(file);
            vfs.write(&path, input.clone())?;
        }
        Ok(ExecOutput {
            stdout: input,
            stderr: vec![],
            exit_code: 0,
        })
    }

    fn cmd_cut(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let args = &cmd.args;
        let mut delim = "\t".to_string();
        let mut field: usize = 1;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-d" => {
                    i += 1;
                    if i < args.len() {
                        delim = args[i].clone();
                    }
                }
                "-f" => {
                    i += 1;
                    if i < args.len() {
                        field = args[i].parse().unwrap_or(1);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let input = get_text_input(cmd, vfs, state)?;
        let text = String::from_utf8_lossy(&input);
        let mut output = String::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split(delim.as_str()).collect();
            if field > 0 && field <= parts.len() {
                output.push_str(parts[field - 1]);
            }
            output.push('\n');
        }
        Ok(ok(output))
    }

    fn cmd_tr(&self, cmd: &Command) -> Result<ExecOutput> {
        let input = cmd.stdin.clone().unwrap_or_default();
        let text = String::from_utf8_lossy(&input).to_string();
        let from = cmd.args.first().map(String::as_str).unwrap_or("");
        let to = cmd.args.get(1).map(String::as_str).unwrap_or("");
        let from_chars: Vec<char> = from.chars().collect();
        let to_chars: Vec<char> = to.chars().collect();
        let output: String = text
            .chars()
            .map(|c| {
                if let Some(pos) = from_chars.iter().position(|&fc| fc == c) {
                    to_chars.get(pos).copied().unwrap_or(c)
                } else {
                    c
                }
            })
            .collect();
        Ok(ok(output))
    }

    fn cmd_ps(&self, pm: &ProcessManager) -> Result<ExecOutput> {
        let mut output = String::from("PID   CMD      STATUS\n");
        for p in pm.running() {
            output.push_str(&format!("{:<6} {:<8} Running\n", p.pid, p.command));
        }
        Ok(ok(output))
    }

    fn cmd_kill(&self, cmd: &Command, pm: &mut ProcessManager) -> Result<ExecOutput> {
        let pid_str = cmd
            .args
            .iter()
            .find(|a| !a.starts_with('-'))
            .ok_or_else(|| RunboxError::Shell("kill: missing pid".into()))?;
        let pid: u32 = pid_str
            .parse()
            .map_err(|_| RunboxError::Shell(format!("kill: invalid pid: {pid_str}")))?;
        pm.kill(pid)?;
        Ok(ok(""))
    }

    fn cmd_env(&self, state: &ShellState) -> Result<ExecOutput> {
        let mut output = String::new();
        let mut pairs: Vec<(&String, &String)> = state.env.iter().collect();
        pairs.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in pairs {
            output.push_str(&format!("{k}={v}\n"));
        }
        Ok(ok(output))
    }

    fn cmd_export(&self, cmd: &Command, state: &mut ShellState) -> Result<ExecOutput> {
        for arg in &cmd.args {
            if let Some((k, v)) = arg.split_once('=') {
                state.export(k, v);
            }
        }
        Ok(ok(""))
    }

    fn cmd_unset(&self, cmd: &Command, state: &mut ShellState) -> Result<ExecOutput> {
        for key in &cmd.args {
            state.env.remove(key);
        }
        Ok(ok(""))
    }

    fn cmd_which(&self, cmd: &Command) -> Result<ExecOutput> {
        let name = cmd
            .args
            .first()
            .ok_or_else(|| RunboxError::Shell("which: missing argument".into()))?;
        Ok(ok(format!("/usr/bin/{name}\n")))
    }

    fn cmd_basename(&self, cmd: &Command) -> Result<ExecOutput> {
        let path = cmd
            .args
            .first()
            .ok_or_else(|| RunboxError::Shell("basename: missing operand".into()))?;
        let base = path.rsplit('/').next().unwrap_or(path);
        let base = if let Some(suffix) = cmd.args.get(1) {
            base.strip_suffix(suffix.as_str()).unwrap_or(base)
        } else {
            base
        };
        Ok(ok(format!("{base}\n")))
    }

    fn cmd_dirname(&self, cmd: &Command) -> Result<ExecOutput> {
        let path = cmd
            .args
            .first()
            .ok_or_else(|| RunboxError::Shell("dirname: missing operand".into()))?;
        let dir = if let Some(pos) = path.rfind('/') {
            if pos == 0 { "/" } else { &path[..pos] }
        } else {
            "."
        };
        Ok(ok(format!("{dir}\n")))
    }

    fn cmd_printf(&self, cmd: &Command) -> Result<ExecOutput> {
        if cmd.args.is_empty() {
            return Ok(ok(""));
        }
        let fmt = &cmd.args[0];
        let args = &cmd.args[1..];
        let mut output = String::new();
        let mut arg_idx = 0;
        let chars: Vec<char> = fmt.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' {
                i += 1;
                if i < chars.len() {
                    match chars[i] {
                        'n' => output.push('\n'),
                        't' => output.push('\t'),
                        'r' => output.push('\r'),
                        '\\' => output.push('\\'),
                        c => {
                            output.push('\\');
                            output.push(c);
                        }
                    }
                }
                i += 1;
                continue;
            }
            if chars[i] == '%' {
                i += 1;
                if i < chars.len() {
                    match chars[i] {
                        's' => {
                            let val = args.get(arg_idx).map(String::as_str).unwrap_or("");
                            output.push_str(val);
                            arg_idx += 1;
                        }
                        'd' => {
                            let val = args.get(arg_idx).map(String::as_str).unwrap_or("0");
                            let n: i64 = val.parse().unwrap_or(0);
                            output.push_str(&n.to_string());
                            arg_idx += 1;
                        }
                        '%' => output.push('%'),
                        c => {
                            output.push('%');
                            output.push(c);
                        }
                    }
                }
                i += 1;
                continue;
            }
            output.push(chars[i]);
            i += 1;
        }
        Ok(ok(output))
    }

    fn cmd_test(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let args: Vec<&str> = cmd
            .args
            .iter()
            .map(String::as_str)
            .filter(|&a| a != "]")
            .collect();
        let result = eval_test(&args, vfs, state);
        Ok(ExecOutput {
            stdout: vec![],
            stderr: vec![],
            exit_code: if result { 0 } else { 1 },
        })
    }

    fn cmd_uname(&self, _cmd: &Command) -> Result<ExecOutput> {
        Ok(ok("Linux runbox 5.15.0 #1 SMP WASM GNU/Linux\n"))
    }

    fn cmd_find(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let args = &cmd.args;
        let path_arg = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .map(String::as_str)
            .unwrap_or(".");
        let search_path = state.resolve(path_arg);
        let name_pattern = find_flag_value(args, "-name");
        let type_filter = find_flag_value(args, "-type");
        let max_depth: Option<usize> =
            find_flag_value(args, "-maxdepth").and_then(|s| s.parse().ok());

        let all_files = vfs.all_file_paths();
        let prefix = if search_path == "/" {
            String::new()
        } else {
            search_path.clone()
        };
        let mut output = String::new();

        for path in &all_files {
            if !prefix.is_empty() && !path.starts_with(&prefix) {
                continue;
            }
            if path.contains("/.runbox_dir") {
                continue;
            }

            if let Some(md) = max_depth {
                let rel = path.strip_prefix(&search_path).unwrap_or(path);
                let depth = rel.matches('/').count();
                if depth > md {
                    continue;
                }
            }

            if let Some(t) = type_filter {
                let is_dir = vfs.is_dir(path);
                match t {
                    "f" if is_dir => continue,
                    "d" if !is_dir => continue,
                    _ => {}
                }
            }

            if let Some(pat) = name_pattern {
                let filename = path.rsplit('/').next().unwrap_or(path);
                if !glob_match_simple(pat, filename) {
                    continue;
                }
            }

            output.push_str(path);
            output.push('\n');
        }
        Ok(ok(output))
    }

    fn cmd_stat(&self, cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<ExecOutput> {
        let path = cmd
            .args
            .iter()
            .find(|a| !a.starts_with('-'))
            .ok_or_else(|| RunboxError::Shell("stat: missing operand".into()))?;
        let resolved = state.resolve(path);
        if !vfs.exists(&resolved) {
            return Err(RunboxError::Shell(format!(
                "stat: {}: No such file or directory",
                path
            )));
        }
        let size = vfs.stat(&resolved).map(|m| m.size).unwrap_or(0);
        let is_dir = vfs.is_dir(&resolved);
        let file_type = if is_dir { "directory" } else { "regular file" };
        Ok(ok(format!(
            "  File: {resolved}\n  Size: {size}\nFile type: {file_type}\n"
        )))
    }
}

fn ok(s: impl AsRef<[u8]>) -> ExecOutput {
    ExecOutput {
        stdout: s.as_ref().to_vec(),
        stderr: vec![],
        exit_code: 0,
    }
}

fn get_text_input(cmd: &Command, vfs: &Vfs, state: &ShellState) -> Result<Vec<u8>> {
    let file_arg = find_non_flag_after_flags(&cmd.args);
    if let Some(file) = file_arg {
        let path = state.resolve(file);
        vfs.read(&path).map(|b| b.to_vec())
    } else if let Some(stdin) = &cmd.stdin {
        Ok(stdin.clone())
    } else {
        Ok(vec![])
    }
}

fn find_non_flag_after_flags(args: &[String]) -> Option<&str> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-n" || arg == "-f" || arg == "-d" {
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(arg.as_str());
    }
    None
}

fn parse_n_flag(args: &[String], default: usize) -> usize {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-n"
            && let Some(val) = args.get(i + 1)
        {
            return val.parse().unwrap_or(default);
        }
        i += 1;
    }
    default
}

fn find_flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            return args.get(i + 1).map(String::as_str);
        }
        i += 1;
    }
    None
}

fn interpret_escape_sequences(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(c) => {
                    out.push('\\');
                    out.push(c);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn eval_test(args: &[&str], vfs: &Vfs, state: &ShellState) -> bool {
    match args {
        ["-f", path] => {
            let p = state.resolve(path);
            vfs.exists(&p) && !vfs.is_dir(&p)
        }
        ["-d", path] => {
            let p = state.resolve(path);
            vfs.exists(&p) && vfs.is_dir(&p)
        }
        ["-e", path] => {
            let p = state.resolve(path);
            vfs.exists(&p)
        }
        ["-z", s] => s.is_empty(),
        ["-n", s] => !s.is_empty(),
        [a, "=", b] => a == b,
        [a, "!=", b] => a != b,
        [a, "-eq", b] => a.parse::<i64>().ok() == b.parse::<i64>().ok(),
        [a, "-ne", b] => a.parse::<i64>().ok() != b.parse::<i64>().ok(),
        [a, "-lt", b] => {
            matches!((a.parse::<i64>(), b.parse::<i64>()), (Ok(x), Ok(y)) if x < y)
        }
        [a, "-gt", b] => {
            matches!((a.parse::<i64>(), b.parse::<i64>()), (Ok(x), Ok(y)) if x > y)
        }
        _ => false,
    }
}

fn glob_match_simple(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == name;
    }
    glob_match_bytes_simple(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_bytes_simple(pat: &[u8], s: &[u8]) -> bool {
    match (pat.first(), s.first()) {
        (None, None) => true,
        (None, _) => false,
        (Some(b'*'), _) => {
            for i in 0..=s.len() {
                if glob_match_bytes_simple(&pat[1..], &s[i..]) {
                    return true;
                }
            }
            false
        }
        (Some(b'?'), Some(_)) => glob_match_bytes_simple(&pat[1..], &s[1..]),
        (Some(p), Some(sc)) if p == sc => glob_match_bytes_simple(&pat[1..], &s[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::Command;
    use crate::vfs::Vfs;

    fn make_cmd(program: &str, args: &[&str]) -> Command {
        Command {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env: vec![],
            stdin: None,
            redirect: None,
        }
    }

    #[test]
    fn test_echo() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let runtime = ShellBuiltins;
        let cmd = make_cmd("echo", &["hello", "world"]);
        let out = runtime.exec(&cmd, &mut vfs, &mut pm).unwrap();
        assert_eq!(out.stdout, b"hello world\n");
    }

    #[test]
    fn test_pwd() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let mut state = ShellState::default();
        let runtime = ShellBuiltins;
        let cmd = make_cmd("pwd", &[]);
        let out = runtime
            .exec_with_state(&cmd, &mut vfs, &mut pm, &mut state)
            .unwrap();
        assert_eq!(out.stdout, b"/\n");
    }

    #[test]
    fn test_touch_and_ls() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let mut state = ShellState::default();
        let runtime = ShellBuiltins;
        let cmd_touch = make_cmd("touch", &["/test.txt"]);
        runtime
            .exec_with_state(&cmd_touch, &mut vfs, &mut pm, &mut state)
            .unwrap();
        let cmd_ls = make_cmd("ls", &["/"]);
        let out_ls = runtime
            .exec_with_state(&cmd_ls, &mut vfs, &mut pm, &mut state)
            .unwrap();
        assert_eq!(out_ls.stdout, b"test.txt\n");
    }

    #[test]
    fn test_cat() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let runtime = ShellBuiltins;
        vfs.write("/hello.txt", b"world".to_vec()).unwrap();
        let cmd = make_cmd("cat", &["/hello.txt"]);
        let out = runtime.exec(&cmd, &mut vfs, &mut pm).unwrap();
        assert_eq!(out.stdout, b"world");
    }

    #[test]
    fn test_mkdir() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let runtime = ShellBuiltins;
        let cmd = make_cmd("mkdir", &["/mydir"]);
        runtime.exec(&cmd, &mut vfs, &mut pm).unwrap();
        assert!(vfs.exists("/mydir/.runbox_dir"));
    }

    #[test]
    fn test_rm() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let runtime = ShellBuiltins;
        vfs.write("/delete_me.txt", vec![]).unwrap();
        let cmd = make_cmd("rm", &["/delete_me.txt"]);
        runtime.exec(&cmd, &mut vfs, &mut pm).unwrap();
        assert!(!vfs.exists("/delete_me.txt"));
    }

    #[test]
    fn test_cd_changes_state() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let mut state = ShellState::default();
        let runtime = ShellBuiltins;
        vfs.write("/mydir/.runbox_dir", vec![]).unwrap();
        let cmd = make_cmd("cd", &["/mydir"]);
        runtime
            .exec_with_state(&cmd, &mut vfs, &mut pm, &mut state)
            .unwrap();
        assert_eq!(state.cwd, "/mydir");
    }

    #[test]
    fn test_grep() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let mut state = ShellState::default();
        let runtime = ShellBuiltins;
        vfs.write("/test.txt", b"hello\nworld\nhello again\n".to_vec())
            .unwrap();
        let cmd = make_cmd("grep", &["hello", "/test.txt"]);
        let out = runtime
            .exec_with_state(&cmd, &mut vfs, &mut pm, &mut state)
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("hello"));
        assert!(!text.contains("world"));
    }

    #[test]
    fn test_wc() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let mut state = ShellState::default();
        let runtime = ShellBuiltins;
        let mut cmd = make_cmd("wc", &["-l"]);
        cmd.stdin = Some(b"line1\nline2\nline3\n".to_vec());
        let out = runtime
            .exec_with_state(&cmd, &mut vfs, &mut pm, &mut state)
            .unwrap();
        assert_eq!(out.stdout, b"3\n");
    }

    #[test]
    fn test_echo_n_flag() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let runtime = ShellBuiltins;
        let cmd = make_cmd("echo", &["-n", "no newline"]);
        let out = runtime.exec(&cmd, &mut vfs, &mut pm).unwrap();
        assert_eq!(out.stdout, b"no newline");
    }

    #[test]
    fn test_sort() {
        let mut vfs = Vfs::new();
        let mut pm = ProcessManager::new();
        let mut state = ShellState::default();
        let runtime = ShellBuiltins;
        let mut cmd = make_cmd("sort", &[]);
        cmd.stdin = Some(b"banana\napple\ncherry\n".to_vec());
        let out = runtime
            .exec_with_state(&cmd, &mut vfs, &mut pm, &mut state)
            .unwrap();
        assert_eq!(out.stdout, b"apple\nbanana\ncherry\n");
    }
}
