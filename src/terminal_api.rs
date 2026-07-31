/// API de Terminal de alto nivel para ejecutar comandos de forma interactiva
/// Versión FINAL corregida y limpia
use crate::error::{Result, RunboxError};
use crate::process::{Pid, ProcessManager};
use crate::runtime::Runtime;
use crate::shell::{Command, RuntimeTarget};
use crate::terminal::Terminal;
use crate::vfs::Vfs;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

// ══════════════════════════════════════════════════════════════════════════════
// TOKENIZER
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Pipe,
    And,
    Or,
    Semicolon,
    Background,
    RedirectOut,    // >
    RedirectAppend, // >>
    RedirectIn,     // <
    RedirectErr,    // 2>
    RedirectErrOut, // 2>&1
}

/// Sesión de terminal interactiva con estado completo
pub struct TerminalSession {
    vfs: Vfs,
    pm: ProcessManager,
    terminal: Terminal,
    cwd: String,
    env: HashMap<String, String>,
    aliases: HashMap<String, String>,
    history: VecDeque<String>,
    last_exit_code: i32,
    max_history: usize,
}

impl TerminalSession {
    pub fn new() -> Self {
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
        env.insert("HOME".to_string(), "/home/user".to_string());
        env.insert("USER".to_string(), "user".to_string());
        env.insert("SHELL".to_string(), "/bin/bash".to_string());
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("PWD".to_string(), "/".to_string());

        let mut session = Self {
            vfs: Vfs::new(),
            pm: ProcessManager::new(),
            terminal: Terminal::new(8192),
            cwd: "/".to_string(),
            env,
            aliases: HashMap::new(),
            history: VecDeque::new(),
            last_exit_code: 0,
            max_history: 1000,
        };

        session.terminal.write_banner();
        session.terminal.write_prompt(&session.cwd);
        session
    }

    /// Resetea la sesión a su estado inicial limpio
    /// Útil para limpiar estado entre tests o sesiones
    pub fn reset(&mut self) {
        // Limpiar VFS
        self.vfs = Vfs::new();

        // Limpiar ProcessManager
        self.pm = ProcessManager::new();

        // Resetear directorio actual
        self.cwd = "/".to_string();

        // Resetear variables de entorno a valores por defecto
        self.env.clear();
        self.env
            .insert("PATH".to_string(), "/usr/bin:/bin".to_string());
        self.env
            .insert("HOME".to_string(), "/home/user".to_string());
        self.env.insert("USER".to_string(), "user".to_string());
        self.env
            .insert("SHELL".to_string(), "/bin/bash".to_string());
        self.env
            .insert("TERM".to_string(), "xterm-256color".to_string());
        self.env.insert("PWD".to_string(), "/".to_string());

        // Limpiar aliases
        self.aliases.clear();

        // Limpiar historial
        self.history.clear();

        // Resetear exit code
        self.last_exit_code = 0;

        // Crear nuevo terminal limpio
        self.terminal = Terminal::new(8192);
        self.terminal.write_banner();
        self.terminal.write_prompt(&self.cwd);
    }

    pub fn exec(&mut self, command_line: &str) -> Result<CommandResult> {
        let trimmed = command_line.trim();
        if trimmed.is_empty() {
            self.terminal.write_prompt(&self.cwd);
            return Ok(CommandResult::success(0));
        }

        self.history.push_back(trimmed.to_string());
        if self.history.len() > self.max_history {
            self.history.pop_front();
        }
        self.terminal.add_history(trimmed.to_string());

        let expanded = self.expand_variables(trimmed);
        let ast = self.parse_command(&expanded)?;
        let result = self.execute_ast(&ast)?;

        self.last_exit_code = result.exit_code;

        if !result.stdout.is_empty() {
            self.terminal
                .write_stdout(result.pid, String::from_utf8_lossy(&result.stdout));
        }
        if !result.stderr.is_empty() {
            self.terminal
                .write_stderr(result.pid, String::from_utf8_lossy(&result.stderr));
        }

        self.terminal.write_prompt(&self.cwd);
        Ok(result)
    }

    // ==================== TOKENIZER ROBUSTO ====================
    fn tokenize(&self, input: &str) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        let mut chars = input.chars().peekable();
        let mut current = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;

        while let Some(c) = chars.next() {
            match c {
                '\'' if !in_double_quote => {
                    in_single_quote = !in_single_quote;
                    if !in_single_quote && !current.is_empty() {
                        tokens.push(Token::Word(current.clone()));
                        current.clear();
                    }
                }
                '"' if !in_single_quote => {
                    in_double_quote = !in_double_quote;
                    if !in_double_quote && !current.is_empty() {
                        tokens.push(Token::Word(current.clone()));
                        current.clear();
                    }
                }
                '\\' if !in_single_quote => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                ' ' | '\t' if !in_single_quote && !in_double_quote => {
                    if !current.is_empty() {
                        tokens.push(Token::Word(current.clone()));
                        current.clear();
                    }
                }
                '|' if !in_single_quote && !in_double_quote => {
                    if !current.is_empty() {
                        tokens.push(Token::Word(current.clone()));
                        current.clear();
                    }
                    if chars.peek() == Some(&'|') {
                        chars.next();
                        tokens.push(Token::Or);
                    } else {
                        tokens.push(Token::Pipe);
                    }
                }
                '&' if !in_single_quote && !in_double_quote => {
                    if !current.is_empty() {
                        tokens.push(Token::Word(current.clone()));
                        current.clear();
                    }
                    if chars.peek() == Some(&'&') {
                        chars.next();
                        tokens.push(Token::And);
                    } else {
                        tokens.push(Token::Background);
                    }
                }
                ';' if !in_single_quote && !in_double_quote => {
                    if !current.is_empty() {
                        tokens.push(Token::Word(current.clone()));
                        current.clear();
                    }
                    tokens.push(Token::Semicolon);
                }
                '>' if !in_single_quote && !in_double_quote => {
                    if !current.is_empty() {
                        tokens.push(Token::Word(current.clone()));
                        current.clear();
                    }
                    if chars.peek() == Some(&'>') {
                        chars.next();
                        tokens.push(Token::RedirectAppend);
                    } else {
                        tokens.push(Token::RedirectOut);
                    }
                }
                '<' if !in_single_quote && !in_double_quote => {
                    if !current.is_empty() {
                        tokens.push(Token::Word(current.clone()));
                        current.clear();
                    }
                    tokens.push(Token::RedirectIn);
                }
                '2' if !in_single_quote && !in_double_quote => {
                    if chars.peek() == Some(&'>') {
                        chars.next();
                        if chars.peek() == Some(&'&') {
                            chars.next();
                            if chars.peek() == Some(&'1') {
                                chars.next();
                                tokens.push(Token::RedirectErrOut);
                            } else {
                                tokens.push(Token::RedirectErr);
                            }
                        } else {
                            tokens.push(Token::RedirectErr);
                        }
                    } else {
                        current.push('2');
                    }
                }
                _ => current.push(c),
            }
        }

        if !current.is_empty() {
            tokens.push(Token::Word(current));
        }

        if in_single_quote || in_double_quote {
            return Err(RunboxError::Shell("unterminated quote".into()));
        }

        Ok(tokens)
    }

    // ==================== PARSER ====================
    fn parse_command(&self, input: &str) -> Result<CommandAst> {
        let tokens = self.tokenize(input)?;
        self.parse_tokens(&tokens)
    }

    fn parse_tokens(&self, tokens: &[Token]) -> Result<CommandAst> {
        if tokens.is_empty() {
            return Err(RunboxError::Shell("empty command".into()));
        }

        if let Some(pos) = tokens.iter().position(|t| matches!(t, Token::Pipe)) {
            let left = self.parse_tokens(&tokens[..pos])?;
            let right = self.parse_tokens(&tokens[pos + 1..])?;
            return Ok(CommandAst::Pipeline(vec![left, right]));
        }

        if let Some(pos) = tokens.iter().position(|t| matches!(t, Token::And)) {
            let left = self.parse_tokens(&tokens[..pos])?;
            let right = self.parse_tokens(&tokens[pos + 1..])?;
            return Ok(CommandAst::And(Box::new(left), Box::new(right)));
        }
        if let Some(pos) = tokens.iter().position(|t| matches!(t, Token::Or)) {
            let left = self.parse_tokens(&tokens[..pos])?;
            let right = self.parse_tokens(&tokens[pos + 1..])?;
            return Ok(CommandAst::Or(Box::new(left), Box::new(right)));
        }

        if tokens.iter().any(|t| matches!(t, Token::Semicolon)) {
            let mut nodes = Vec::new();
            let mut start = 0;
            for (i, t) in tokens.iter().enumerate() {
                if matches!(t, Token::Semicolon) {
                    if i > start {
                        nodes.push(self.parse_tokens(&tokens[start..i])?);
                    }
                    start = i + 1;
                }
            }
            if start < tokens.len() {
                nodes.push(self.parse_tokens(&tokens[start..])?);
            }
            return Ok(CommandAst::Sequence(nodes));
        }

        if let Some(last) = tokens.last()
            && matches!(last, Token::Background)
        {
            let node = self.parse_tokens(&tokens[..tokens.len() - 1])?;
            return Ok(CommandAst::Background(Box::new(node)));
        }

        if tokens.iter().any(|t| {
            matches!(
                t,
                Token::RedirectOut
                    | Token::RedirectAppend
                    | Token::RedirectIn
                    | Token::RedirectErr
                    | Token::RedirectErrOut
            )
        }) {
            return self.parse_redirect_from_tokens(tokens);
        }

        self.parse_simple_from_tokens(tokens)
    }

    fn parse_simple_from_tokens(&self, tokens: &[Token]) -> Result<CommandAst> {
        let words: Vec<String> = tokens
            .iter()
            .filter_map(|t| {
                if let Token::Word(w) = t {
                    Some(w.clone())
                } else {
                    None
                }
            })
            .collect();

        if words.is_empty() {
            return Err(RunboxError::Shell("no command".into()));
        }

        let program = words[0].clone();
        let args = words[1..].to_vec();

        let mut expanded_args = Vec::new();
        for arg in args {
            if arg.contains('*') || arg.contains('?') || arg.contains('[') {
                let matches = self.expand_glob(&arg);
                if matches.is_empty() {
                    expanded_args.push(arg);
                } else {
                    expanded_args.extend(matches);
                }
            } else {
                expanded_args.push(arg);
            }
        }

        Ok(CommandAst::Simple(Command {
            program,
            args: expanded_args,
            env: vec![], // ← CORREGIDO: Vec<(String,String)>
            stdin: None,
            redirect: None,
        }))
    }

    fn parse_redirect_from_tokens(&self, tokens: &[Token]) -> Result<CommandAst> {
        let mut cmd_tokens = Vec::new();
        let mut redirects = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            match &tokens[i] {
                Token::RedirectOut
                | Token::RedirectAppend
                | Token::RedirectIn
                | Token::RedirectErr => {
                    let append = matches!(tokens[i], Token::RedirectAppend);
                    let fd = match &tokens[i] {
                        Token::RedirectErr => 2,
                        Token::RedirectIn => 0,
                        _ => 1, // RedirectOut, RedirectAppend
                    };
                    i += 1;
                    if i < tokens.len()
                        && let Token::Word(path) = &tokens[i]
                    {
                        redirects.push(Redirect {
                            fd,
                            target: RedirectTarget::File {
                                path: path.clone(),
                                append,
                            },
                        });
                    }
                }
                Token::RedirectErrOut => {
                    redirects.push(Redirect {
                        fd: 2,
                        target: RedirectTarget::Fd(1),
                    });
                }
                Token::Word(w) => cmd_tokens.push(Token::Word(w.clone())),
                _ => {}
            }
            i += 1;
        }

        let node = self.parse_simple_from_tokens(&cmd_tokens)?;
        Ok(CommandAst::Redirect {
            node: Box::new(node),
            redirects,
        })
    }

    // ==================== EXPANSIÓN ====================
    fn expand_variables(&self, input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '$' {
                if let Some('{') = chars.peek() {
                    chars.next();
                    let mut var = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch == '}' {
                            chars.next();
                            break;
                        }
                        var.push(ch);
                        chars.next();
                    }
                    if let Some(val) = self.env.get(&var) {
                        result.push_str(val);
                    }
                } else {
                    let mut var = String::new();
                    while let Some(&ch) = chars.peek() {
                        if !ch.is_alphanumeric() && ch != '_' {
                            break;
                        }
                        var.push(ch);
                        chars.next();
                    }
                    match var.as_str() {
                        "?" => result.push_str(&self.last_exit_code.to_string()),
                        "$" => result.push('1'),
                        "PWD" => result.push_str(&self.cwd),
                        "HOME" => result.push_str(self.env.get("HOME").unwrap_or(&"/".to_string())),
                        _ => {
                            if let Some(val) = self.env.get(&var) {
                                result.push_str(val);
                            } else {
                                result.push('$');
                                result.push_str(&var);
                            }
                        }
                    }
                }
            } else if c == '~' && result.is_empty() {
                result.push_str(self.env.get("HOME").unwrap_or(&"/".to_string()));
            } else {
                result.push(c);
            }
        }
        result
    }

    fn expand_glob(&self, pattern: &str) -> Vec<String> {
        let mut matches = Vec::new();
        let (dir, file_pattern) = if let Some(pos) = pattern.rfind('/') {
            (&pattern[..pos], &pattern[pos + 1..])
        } else {
            (&self.cwd[..], pattern)
        };

        if let Ok(entries) = self.vfs.list(dir) {
            for entry in entries {
                if self.glob_match_iterative(file_pattern, &entry) {
                    let full = if dir == "/" {
                        format!("/{}", entry)
                    } else {
                        format!("{}/{}", dir, entry)
                    };
                    matches.push(full);
                }
            }
        }
        matches
    }

    fn glob_match_iterative(&self, pattern: &str, name: &str) -> bool {
        let p: Vec<char> = pattern.chars().collect();
        let n: Vec<char> = name.chars().collect();
        let mut p_idx = 0;
        let mut n_idx = 0;
        let mut star_idx = None;
        let mut match_idx = 0;

        while n_idx < n.len() {
            if p_idx < p.len() && (p[p_idx] == '?' || p[p_idx] == n[n_idx]) {
                p_idx += 1;
                n_idx += 1;
            } else if p_idx < p.len() && p[p_idx] == '*' {
                star_idx = Some(p_idx);
                match_idx = n_idx;
                p_idx += 1;
            } else if let Some(s_idx) = star_idx {
                p_idx = s_idx + 1;
                match_idx += 1;
                n_idx = match_idx;
            } else {
                return false;
            }
        }

        while p_idx < p.len() && p[p_idx] == '*' {
            p_idx += 1;
        }
        p_idx >= p.len()
    }

    // ==================== EJECUCIÓN ====================
    fn execute_ast(&mut self, ast: &CommandAst) -> Result<CommandResult> {
        self.execute_ast_with_input(ast, None)
    }

    fn execute_ast_with_input(
        &mut self,
        ast: &CommandAst,
        stdin: Option<&[u8]>,
    ) -> Result<CommandResult> {
        match ast {
            CommandAst::Simple(cmd) => self.execute_simple(cmd, stdin),
            CommandAst::Pipeline(nodes) => self.execute_pipeline(nodes),
            CommandAst::Sequence(nodes) => {
                let mut last = CommandResult::default();
                for node in nodes {
                    last = self.execute_ast(node)?;
                }
                Ok(last)
            }
            CommandAst::And(left, right) => {
                let l = self.execute_ast(left)?;
                if l.exit_code == 0 {
                    let r = self.execute_ast(right)?;
                    Ok(combine_results(l, r))
                } else {
                    Ok(l)
                }
            }
            CommandAst::Or(left, right) => {
                let l = self.execute_ast(left)?;
                if l.exit_code != 0 {
                    let r = self.execute_ast(right)?;
                    Ok(combine_results(l, r))
                } else {
                    Ok(l)
                }
            }
            CommandAst::Background(node) => self.execute_ast(node),
            CommandAst::Redirect { node, redirects } => self.execute_with_redirect(node, redirects),
        }
    }

    fn execute_simple(&mut self, cmd: &Command, stdin: Option<&[u8]>) -> Result<CommandResult> {
        match cmd.program.as_str() {
            "cd" => return self.builtin_cd(cmd),
            "export" => return self.builtin_export(cmd),
            "alias" => return self.builtin_alias(cmd),
            "pwd" => return self.builtin_pwd(),
            "history" => return self.builtin_history(cmd),
            "echo" => return self.builtin_echo(cmd),
            "cat" => return self.builtin_cat(cmd, stdin),
            "ls" => return self.builtin_ls(cmd),
            "touch" => return self.builtin_touch(cmd),
            "mkdir" => return self.builtin_mkdir(cmd),
            "grep" => return self.builtin_grep(cmd, stdin),
            "exit" => return Ok(CommandResult::success(0)),
            _ => {}
        }

        if cmd.program == "npm" && cmd.args.first().map(String::as_str) == Some("init") {
            return self.builtin_npm_init(cmd);
        }

        let target = RuntimeTarget::detect(cmd);
        let runtime: Box<dyn Runtime> = match target {
            RuntimeTarget::Bun => Box::new(crate::runtime::bun::BunRuntime),
            RuntimeTarget::Python => Box::new(crate::runtime::python::PythonRuntime),
            RuntimeTarget::Git => Box::new(crate::runtime::git::GitRuntime),
            RuntimeTarget::Npm => Box::new(crate::runtime::npm::PackageManagerRuntime::npm()),
            RuntimeTarget::Pnpm => Box::new(crate::runtime::npm::PackageManagerRuntime::pnpm()),
            RuntimeTarget::Yarn => Box::new(crate::runtime::npm::PackageManagerRuntime::yarn()),
            RuntimeTarget::Shell => Box::new(crate::runtime::shell_builtins::ShellBuiltins),
            _ => {
                return Err(RunboxError::Shell(format!(
                    "{}: command not found",
                    cmd.program
                )));
            }
        };

        let mut runtime_cmd = cmd.clone();
        runtime_cmd.env = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        runtime_cmd.env.retain(|(k, _)| k != "PWD");
        runtime_cmd.env.push(("PWD".to_string(), self.cwd.clone()));

        let pid = self
            .pm
            .spawn(&runtime_cmd.program, runtime_cmd.args.clone());
        let output = runtime.exec(&runtime_cmd, &mut self.vfs, &mut self.pm)?;
        self.pm.exit(pid, output.exit_code)?;

        Ok(CommandResult {
            pid,
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.exit_code,
        })
    }

    // ==================== BUILTINS BÁSICOS ====================
    fn builtin_echo(&self, cmd: &Command) -> Result<CommandResult> {
        let output = cmd.args.join(" ") + "\n";
        Ok(CommandResult {
            pid: 0,
            stdout: output.into_bytes(),
            stderr: vec![],
            exit_code: 0,
        })
    }

    fn builtin_cat(&self, cmd: &Command, stdin: Option<&[u8]>) -> Result<CommandResult> {
        if cmd.args.is_empty() {
            if let Some(input) = stdin {
                return Ok(CommandResult {
                    pid: 0,
                    stdout: input.to_vec(),
                    stderr: vec![],
                    exit_code: 0,
                });
            }
            return Ok(CommandResult::error(
                0,
                "cat: missing file operand\n".to_string(),
            ));
        }
        let mut stdout = Vec::new();
        for file in &cmd.args {
            let resolved = self.resolve_path(file);
            match self.vfs.read(&resolved) {
                Ok(content) => stdout.extend_from_slice(content),
                Err(_) => {
                    return Ok(CommandResult::error(
                        0,
                        format!("cat: {}: No such file or directory\n", file),
                    ));
                }
            }
        }
        Ok(CommandResult {
            pid: 0,
            stdout,
            stderr: vec![],
            exit_code: 0,
        })
    }

    fn builtin_ls(&self, cmd: &Command) -> Result<CommandResult> {
        let mut show_all = false;
        let mut targets: Vec<&str> = Vec::new();

        for arg in &cmd.args {
            if arg.starts_with('-') {
                if arg.contains('a') {
                    show_all = true;
                }
                continue;
            }
            targets.push(arg.as_str());
        }

        if targets.is_empty() {
            targets.push(&self.cwd);
        }

        let multiple = targets.len() > 1;
        let mut chunks = Vec::new();

        for target in targets {
            let resolved = self.resolve_path(target);
            if let Ok(entries) = self.vfs.list(&resolved) {
                let mut visible: Vec<String> = entries
                    .into_iter()
                    .filter(|entry| show_all || !entry.starts_with('.'))
                    .collect();
                visible.sort();
                let listing = visible.join("  ");
                if multiple {
                    chunks.push(format!("{target}:\n{listing}"));
                } else {
                    chunks.push(listing);
                }
            } else if self.vfs.read(&resolved).is_ok() {
                chunks.push(self.display_name(&resolved));
            } else {
                return Ok(CommandResult::error(
                    0,
                    format!(
                        "ls: cannot access '{}': No such file or directory\n",
                        target
                    ),
                ));
            }
        }

        Ok(CommandResult {
            pid: 0,
            stdout: format!("{}\n", chunks.join("\n")).into_bytes(),
            stderr: vec![],
            exit_code: 0,
        })
    }

    fn builtin_touch(&mut self, cmd: &Command) -> Result<CommandResult> {
        // ← CORREGIDO: &mut self
        for file in &cmd.args {
            let resolved = self.resolve_path(file);
            self.vfs.write(&resolved, vec![])?;
        }
        Ok(CommandResult::success(0))
    }

    fn builtin_mkdir(&mut self, cmd: &Command) -> Result<CommandResult> {
        let mut created = 0usize;
        for dir in &cmd.args {
            if dir.starts_with('-') {
                continue;
            }
            let resolved = self.resolve_path(dir);

            // Usar el método mkdir nativo del VFS
            self.vfs.mkdir(&resolved)?;

            created += 1;
        }
        if created == 0 {
            return Ok(CommandResult::error(
                0,
                "mkdir: missing operand\n".to_string(),
            ));
        }
        Ok(CommandResult::success(0))
    }

    fn builtin_grep(&self, cmd: &Command, stdin: Option<&[u8]>) -> Result<CommandResult> {
        if cmd.args.is_empty() {
            return Ok(CommandResult::error(
                0,
                "grep: missing pattern\n".to_string(),
            ));
        }
        let pattern = &cmd.args[0];
        let files = if cmd.args.len() > 1 {
            &cmd.args[1..]
        } else {
            &[]
        };
        let mut stdout = Vec::new();

        if files.is_empty() {
            if let Some(input) = stdin {
                let text = String::from_utf8_lossy(input);
                for line in text.lines() {
                    if line.contains(pattern) {
                        stdout.extend_from_slice(line.as_bytes());
                        stdout.push(b'\n');
                    }
                }
            }
            return Ok(CommandResult {
                pid: 0,
                stdout,
                stderr: vec![],
                exit_code: 0,
            });
        }

        for file in files {
            let resolved = self.resolve_path(file);
            if let Ok(content) = self.vfs.read(&resolved) {
                let text = String::from_utf8_lossy(content);
                for line in text.lines() {
                    if line.contains(pattern) {
                        stdout.extend_from_slice(line.as_bytes());
                        stdout.push(b'\n');
                    }
                }
            }
        }
        Ok(CommandResult {
            pid: 0,
            stdout,
            stderr: vec![],
            exit_code: 0,
        })
    }

    // ==================== BUILTINS YA EXISTENTES ====================
    fn builtin_cd(&mut self, cmd: &Command) -> Result<CommandResult> {
        let path = cmd.args.first().map(String::as_str).unwrap_or("~");
        let normalized = self.resolve_path(path);

        if self.is_directory(&normalized) {
            self.cwd = normalized;
            self.env.insert("PWD".to_string(), self.cwd.clone());
            Ok(CommandResult::success(0))
        } else {
            Ok(CommandResult::error(
                0,
                format!("cd: {}: No such file or directory\n", path),
            ))
        }
    }

    fn builtin_pwd(&self) -> Result<CommandResult> {
        Ok(CommandResult {
            pid: 0,
            stdout: format!("{}\n", self.cwd).into_bytes(),
            stderr: vec![],
            exit_code: 0,
        })
    }

    fn builtin_history(&mut self, cmd: &Command) -> Result<CommandResult> {
        if cmd.args.first().map(|s| s.as_str()) == Some("-c") {
            self.history.clear();
            return Ok(CommandResult::success(0));
        }
        let mut output = String::new();
        for (i, entry) in self.history.iter().enumerate() {
            output.push_str(&format!("{:5}  {}\n", i + 1, entry));
        }
        Ok(CommandResult {
            pid: 0,
            stdout: output.into_bytes(),
            stderr: vec![],
            exit_code: 0,
        })
    }

    fn builtin_export(&mut self, cmd: &Command) -> Result<CommandResult> {
        for arg in &cmd.args {
            if let Some((key, value)) = arg.split_once('=') {
                self.env.insert(key.to_string(), value.to_string());
            }
        }
        Ok(CommandResult::success(0))
    }

    fn builtin_alias(&mut self, cmd: &Command) -> Result<CommandResult> {
        if cmd.args.is_empty() {
            let mut output = String::new();
            for (name, value) in &self.aliases {
                output.push_str(&format!("alias {}='{}'\n", name, value));
            }
            Ok(CommandResult {
                pid: 0,
                stdout: output.into_bytes(),
                stderr: vec![],
                exit_code: 0,
            })
        } else {
            for arg in &cmd.args {
                if let Some((name, value)) = arg.split_once('=') {
                    self.aliases.insert(name.to_string(), value.to_string());
                }
            }
            Ok(CommandResult::success(0))
        }
    }

    fn builtin_npm_init(&mut self, cmd: &Command) -> Result<CommandResult> {
        let name = self
            .cwd
            .trim_matches('/')
            .split('/')
            .next_back()
            .filter(|segment| !segment.is_empty())
            .unwrap_or("my-project");
        let pkg_path = if self.cwd == "/" {
            "/package.json".to_string()
        } else {
            format!("{}/package.json", self.cwd)
        };
        let pkg = serde_json::json!({
            "name": name,
            "version": "0.1.0",
            "description": "",
            "main": "index.js",
            "scripts": {
                "dev": "bun run src/index.ts",
                "build": "bun build src/index.ts --outdir dist",
                "test": "bun test"
            }
        });
        let bytes = serde_json::to_string_pretty(&pkg)
            .map_err(|e| RunboxError::Runtime(format!("package.json serialization failed: {e}")))?
            .into_bytes();
        self.vfs.write(&pkg_path, bytes)?;

        let output = if cmd.args.iter().any(|a| a == "-y" || a == "--yes") {
            format!("Wrote to {pkg_path}")
        } else {
            serde_json::to_string_pretty(&pkg).unwrap_or_default()
        };

        Ok(CommandResult {
            pid: 0,
            stdout: output.into_bytes(),
            stderr: vec![],
            exit_code: 0,
        })
    }

    fn execute_pipeline(&mut self, nodes: &[CommandAst]) -> Result<CommandResult> {
        if nodes.is_empty() {
            return Err(RunboxError::Shell("empty pipeline".into()));
        }

        let mut flattened = Vec::new();
        for node in nodes {
            collect_pipeline_nodes(node, &mut flattened);
        }

        let mut last_stdout = Vec::new();
        let mut final_result = CommandResult::default();

        for node in flattened {
            let result = self.execute_ast_with_input(
                node,
                (!last_stdout.is_empty()).then_some(last_stdout.as_slice()),
            )?;
            last_stdout = result.stdout.clone();
            final_result = result;
        }
        Ok(final_result)
    }

    fn execute_with_redirect(
        &mut self,
        node: &CommandAst,
        redirects: &[Redirect],
    ) -> Result<CommandResult> {
        let mut result = self.execute_ast(node)?;

        for redirect in redirects {
            match &redirect.target {
                RedirectTarget::File { path, append } => {
                    let data = match redirect.fd {
                        1 => &result.stdout,
                        2 => &result.stderr,
                        _ => continue,
                    };
                    let resolved = self.resolve_path(path);
                    if *append {
                        let mut existing = self.vfs.read(&resolved).unwrap_or_default().to_vec();
                        existing.extend_from_slice(data);
                        self.vfs.write(&resolved, existing)?;
                    } else {
                        self.vfs.write(&resolved, data.to_vec())?;
                    }
                    if redirect.fd == 1 {
                        result.stdout.clear();
                    } else if redirect.fd == 2 {
                        result.stderr.clear();
                    }
                }
                RedirectTarget::Fd(target_fd) if redirect.fd == 2 && *target_fd == 1 => {
                    result.stdout.extend_from_slice(&result.stderr);
                    result.stderr.clear();
                }
                _ => {}
            }
        }
        Ok(result)
    }

    pub fn drain_output(&mut self) -> String {
        self.terminal.output_drain_json()
    }

    pub fn get_state(&self) -> SessionState {
        // 🔧 CORRECCIÓN: Crear el estado de forma más segura para evitar memory access issues
        let history_vec: Vec<String> = self.history.iter().cloned().collect();

        SessionState {
            cwd: self.cwd.clone(),
            env: self.env.clone(),
            last_exit_code: self.last_exit_code,
            history: history_vec,
        }
    }

    // ==================== VFS WRAPPER METHODS ====================
    // Estos métodos encapsulan el acceso al VFS para evitar problemas de borrowing en WASM

    pub fn vfs_write(&mut self, path: &str, content: Vec<u8>) -> Result<()> {
        self.vfs.write(path, content)
    }

    pub fn vfs_read(&self, path: &str) -> Result<Vec<u8>> {
        self.vfs.read(path).map(|b| b.to_vec())
    }

    pub fn vfs_list(&self, path: &str) -> Result<Vec<String>> {
        self.vfs.list(path)
    }

    pub fn vfs_exists(&self, path: &str) -> bool {
        self.vfs.exists(path)
    }

    pub fn vfs_remove(&mut self, path: &str) -> Result<()> {
        self.vfs.remove(path)
    }

    fn resolve_path(&self, path: &str) -> String {
        let expanded = self.expand_variables(path);
        if expanded.is_empty() {
            return self.cwd.clone();
        }
        let raw = if expanded == "~" {
            self.env
                .get("HOME")
                .cloned()
                .unwrap_or_else(|| "/".to_string())
        } else if expanded.starts_with('/') {
            expanded
        } else {
            format!("{}/{}", self.cwd.trim_end_matches('/'), expanded)
        };
        normalize_path(&raw)
    }

    fn is_directory(&self, path: &str) -> bool {
        self.vfs.list(path).is_ok()
    }

    fn display_name(&self, path: &str) -> String {
        path.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .to_string()
    }
}

impl Default for TerminalSession {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== TIPOS ====================
#[derive(Debug, Clone)]
pub enum CommandAst {
    Simple(Command),
    Pipeline(Vec<CommandAst>),
    Sequence(Vec<CommandAst>),
    And(Box<CommandAst>, Box<CommandAst>),
    Or(Box<CommandAst>, Box<CommandAst>),
    Background(Box<CommandAst>),
    Redirect {
        node: Box<CommandAst>,
        redirects: Vec<Redirect>,
    },
}

#[derive(Debug, Clone)]
pub struct Redirect {
    pub fd: i32,
    pub target: RedirectTarget,
}

#[derive(Debug, Clone)]
pub enum RedirectTarget {
    File { path: String, append: bool },
    Fd(i32),
    Pipe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub pid: Pid,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

impl CommandResult {
    pub fn success(pid: Pid) -> Self {
        Self {
            pid,
            stdout: vec![],
            stderr: vec![],
            exit_code: 0,
        }
    }
    pub fn error(pid: Pid, message: String) -> Self {
        Self {
            pid,
            stdout: vec![],
            stderr: message.into_bytes(),
            exit_code: 1,
        }
    }
}

impl Default for CommandResult {
    fn default() -> Self {
        Self::success(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub cwd: String,
    pub env: HashMap<String, String>,
    pub last_exit_code: i32,
    pub history: Vec<String>,
}

// ==================== UTILIDADES ====================
fn normalize_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn combine_results(mut left: CommandResult, right: CommandResult) -> CommandResult {
    left.stdout.extend_from_slice(&right.stdout);
    left.stderr.extend_from_slice(&right.stderr);
    CommandResult {
        pid: right.pid,
        stdout: left.stdout,
        stderr: left.stderr,
        exit_code: right.exit_code,
    }
}

fn collect_pipeline_nodes<'a>(node: &'a CommandAst, out: &mut Vec<&'a CommandAst>) {
    match node {
        CommandAst::Pipeline(nodes) => {
            for child in nodes {
                collect_pipeline_nodes(child, out);
            }
        }
        _ => out.push(node),
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalSession;

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn demo_shell_features_work_end_to_end() {
        let mut session = TerminalSession::new();

        assert_eq!(session.exec("mkdir -p /project/src").unwrap().exit_code, 0);
        assert_eq!(
            session
                .exec("touch /project/src/index.js")
                .unwrap()
                .exit_code,
            0
        );

        let ls_all = session.exec("ls -la /project").unwrap();
        assert_eq!(ls_all.exit_code, 0);
        let ls_all_text = text(&ls_all.stdout);
        assert!(ls_all_text.contains("src"));

        let ls_src = session.exec("ls /project/src").unwrap();
        assert_eq!(ls_src.exit_code, 0);
        let ls_src_text = text(&ls_src.stdout);
        assert!(ls_src_text.contains("index.js"));
        assert!(!ls_src_text.contains(".keep"));

        session.exec("echo Hello World > /output.txt").unwrap();
        session.exec("echo Line 2 >> /output.txt").unwrap();
        let cat = session.exec("cat /output.txt").unwrap();
        assert_eq!(text(&cat.stdout), "Hello World\nLine 2\n");

        let piped = session.exec("echo Test | cat").unwrap();
        assert_eq!(piped.exit_code, 0);
        assert_eq!(text(&piped.stdout), "Test\n");

        session.exec("export MY_VAR=hello").unwrap();
        let echo_var = session.exec("echo $MY_VAR").unwrap();
        assert_eq!(text(&echo_var.stdout), "hello\n");

        let and_result = session.exec("echo success && echo chained").unwrap();
        assert_eq!(and_result.exit_code, 0);
        assert_eq!(text(&and_result.stdout), "success\nchained\n");

        let or_result = session
            .exec("ls /nonexistent.txt || echo File not found")
            .unwrap();
        assert_eq!(or_result.exit_code, 0);
        assert!(text(&or_result.stdout).contains("File not found"));
        assert!(text(&or_result.stderr).contains("/nonexistent.txt"));
    }

    #[test]
    fn demo_globs_and_file_listing_work() {
        let mut session = TerminalSession::new();

        session.exec("touch /file1.txt").unwrap();
        session.exec("touch /file2.txt").unwrap();
        session.exec("touch /file3.rs").unwrap();

        let glob_txt = session.exec("ls /*.txt").unwrap();
        assert_eq!(glob_txt.exit_code, 0);
        let stdout = text(&glob_txt.stdout);
        assert!(stdout.contains("file1.txt"));
        assert!(stdout.contains("file2.txt"));
        assert!(!stdout.contains("file3.rs"));

        let glob_q = session.exec("ls /file?.txt").unwrap();
        assert_eq!(glob_q.exit_code, 0);
        let stdout = text(&glob_q.stdout);
        assert!(stdout.contains("file1.txt"));
        assert!(stdout.contains("file2.txt"));
    }

    #[test]
    fn npm_init_uses_current_directory() {
        let mut session = TerminalSession::new();

        session.exec("mkdir /myapp").unwrap();
        session.exec("cd /myapp").unwrap();

        let init = session.exec("npm init -y").unwrap();
        assert_eq!(init.exit_code, 0);
        assert!(text(&init.stdout).contains("/myapp/package.json"));

        let cat = session.exec("cat /myapp/package.json").unwrap();
        assert_eq!(cat.exit_code, 0);
        let stdout = text(&cat.stdout);
        assert!(stdout.contains("\"name\": \"myapp\""));
    }

    #[test]
    fn git_flow_works_in_interactive_session() {
        let mut session = TerminalSession::new();

        assert_eq!(session.exec("git init").unwrap().exit_code, 0);
        assert_eq!(
            session
                .exec("git config user.name \"Test User\"")
                .unwrap()
                .exit_code,
            0
        );
        assert_eq!(
            session
                .exec("git config user.email \"test@example.com\"")
                .unwrap()
                .exit_code,
            0
        );
        session.exec("touch /README.md").unwrap();
        session.exec("git add .").unwrap();

        let commit = session.exec("git commit -m \"Initial commit\"").unwrap();
        assert_eq!(commit.exit_code, 0);
        assert!(text(&commit.stdout).contains("Initial commit"));

        let log = session.exec("git log").unwrap();
        assert_eq!(log.exit_code, 0);
        let stdout = text(&log.stdout);
        assert!(stdout.contains("Initial commit"));
        assert!(stdout.contains("Test User") || stdout.contains("RunBox User"));
    }
}
