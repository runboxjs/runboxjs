/// Shell emulator — parse and dispatch commands to the correct runtime.
use crate::error::{Result, RunboxError};
use std::collections::HashMap;

// ── ShellState ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ShellState {
    pub cwd: String,
    pub env: HashMap<String, String>,
    pub last_exit: i32,
}

impl Default for ShellState {
    fn default() -> Self {
        let mut env = HashMap::new();
        env.insert("HOME".into(), "/home/user".into());
        env.insert("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into());
        env.insert("SHELL".into(), "/bin/sh".into());
        env.insert("USER".into(), "user".into());
        env.insert("TERM".into(), "xterm-256color".into());
        env.insert("LANG".into(), "en_US.UTF-8".into());
        env.insert("PWD".into(), "/".into());
        Self {
            cwd: "/".into(),
            env,
            last_exit: 0,
        }
    }
}

impl ShellState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve(&self, path: &str) -> String {
        let expanded = if path.starts_with('~') {
            let home = self
                .env
                .get("HOME")
                .map(String::as_str)
                .unwrap_or("/home/user");
            if path == "~" {
                home.to_string()
            } else if let Some(rest) = path.strip_prefix("~/") {
                format!("{}/{}", home, rest)
            } else {
                path.to_string()
            }
        } else {
            path.to_string()
        };

        if expanded.starts_with('/') {
            normalize_path(&expanded)
        } else {
            normalize_path(&format!("{}/{}", self.cwd, expanded))
        }
    }

    pub fn set_cwd(&mut self, path: &str) {
        self.cwd = path.to_string();
        self.env.insert("PWD".into(), path.to_string());
    }

    pub fn export(&mut self, key: &str, value: &str) {
        self.env.insert(key.into(), value.into());
    }
}

pub fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

// ── Redirect ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Redirect {
    Truncate(String),
    Append(String),
    Stderr(String),
    StderrToStdout,
}

// ── Command ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<Vec<u8>>,
    pub redirect: Option<Redirect>,
}

impl Command {
    pub fn parse(line: &str) -> Result<Self> {
        let state = ShellState::default();
        parse_single_command(line, &state)
    }
}

// ── Pipeline & CommandList ────────────────────────────────────────────────────

pub type Pipeline = Vec<Command>;

#[derive(Debug, Clone, PartialEq)]
pub enum ListOp {
    And,
    Or,
    Semi,
}

pub struct CommandList {
    pub first: Pipeline,
    pub rest: Vec<(ListOp, Pipeline)>,
}

// ── Public parse API ──────────────────────────────────────────────────────────

pub fn parse_command_list(line: &str, state: &ShellState) -> Result<CommandList> {
    let segments = split_by_list_ops(line);
    if segments.is_empty() {
        return Err(RunboxError::Shell("empty command".into()));
    }
    let mut iter = segments.into_iter();
    let (_, first_str) = iter.next().unwrap();
    let first_str = first_str.trim();
    if first_str.is_empty() {
        return Err(RunboxError::Shell("empty command".into()));
    }
    let first = parse_pipeline(first_str, state)?;
    let mut rest = vec![];
    for (op, seg) in iter {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        let pl = parse_pipeline(seg, state)?;
        if let Some(op) = op {
            rest.push((op, pl));
        }
    }
    Ok(CommandList { first, rest })
}

// ── Variable expansion ────────────────────────────────────────────────────────

pub fn expand_vars(s: &str, env: &HashMap<String, String>, last_exit: i32) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' {
            result.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            result.push('$');
            break;
        }
        match chars[i] {
            '?' => {
                result.push_str(&last_exit.to_string());
                i += 1;
            }
            '{' => {
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                result.push_str(env.get(&name).map(String::as_str).unwrap_or(""));
                if i < chars.len() {
                    i += 1;
                }
            }
            c if c.is_alphanumeric() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                result.push_str(env.get(&name).map(String::as_str).unwrap_or(""));
            }
            c => {
                result.push('$');
                result.push(c);
                i += 1;
            }
        }
    }
    result
}

// ── Glob expansion ────────────────────────────────────────────────────────────

pub fn expand_glob(pattern: &str, all_files: &[String]) -> Vec<String> {
    if !pattern.contains('*') && !pattern.contains('?') {
        return vec![pattern.to_string()];
    }
    let mut matches: Vec<String> = all_files
        .iter()
        .filter(|f| glob_match(pattern, f))
        .cloned()
        .collect();
    matches.sort();
    if matches.is_empty() {
        vec![pattern.to_string()]
    } else {
        matches
    }
}

fn glob_match(pattern: &str, path: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), path.as_bytes())
}

fn glob_match_bytes(pat: &[u8], s: &[u8]) -> bool {
    match (pat.first(), s.first()) {
        (None, None) => true,
        (None, _) => false,
        (Some(b'*'), _) => {
            if pat.len() >= 2 && pat[1] == b'*' {
                let rest_pat = &pat[2..];
                for i in 0..=s.len() {
                    if glob_match_bytes(rest_pat, &s[i..]) {
                        return true;
                    }
                }
                false
            } else {
                let rest_pat = &pat[1..];
                for i in 0..=s.len() {
                    if i > 0 && s[i - 1] == b'/' {
                        break;
                    }
                    if glob_match_bytes(rest_pat, &s[i..]) {
                        return true;
                    }
                }
                false
            }
        }
        (Some(b'?'), Some(c)) if *c != b'/' => glob_match_bytes(&pat[1..], &s[1..]),
        (Some(p), Some(sc)) if p == sc => glob_match_bytes(&pat[1..], &s[1..]),
        _ => false,
    }
}

// ── Runtime target ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeTarget {
    Bun,
    Python,
    Git,
    Curl,
    Npm,
    Pnpm,
    Yarn,
    Shell,
    Unknown,
}

impl RuntimeTarget {
    pub fn detect(cmd: &Command) -> Self {
        match cmd.program.as_str() {
            "bun" | "bunx" | "node" | "nodejs" | "tsx" | "ts-node" => Self::Bun,
            "python" | "python3" | "pip" | "pip3" => Self::Python,
            "git" => Self::Git,
            "curl" | "wget" => Self::Curl,
            "npm" | "npx" => Self::Npm,
            "pnpm" | "pnpx" => Self::Pnpm,
            "yarn" => Self::Yarn,
            "cd" | "ls" | "echo" | "cat" | "pwd" | "mkdir" | "rm" | "cp" | "mv" | "touch"
            | "grep" | "find" | "head" | "tail" | "wc" | "sort" | "uniq" | "cut" | "tr" | "tee"
            | "env" | "export" | "unset" | "which" | "clear" | "date" | "basename" | "dirname"
            | "printf" | "true" | "false" | "test" | "[" | "chmod" | "chown" | "uname" | "ps"
            | "kill" | "sleep" | "stat" => Self::Shell,
            _ => Self::Unknown,
        }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn split_by_list_ops(line: &str) -> Vec<(Option<ListOp>, String)> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut result: Vec<(Option<ListOp>, String)> = vec![];
    let mut segment = String::new();
    let mut i = 0;
    let mut in_quotes = false;
    let mut quote_char = ' ';

    while i < n {
        let ch = chars[i];

        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            }
            segment.push(ch);
            i += 1;
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_quotes = true;
            quote_char = ch;
            segment.push(ch);
            i += 1;
            continue;
        }

        if ch == '\\' {
            segment.push(ch);
            i += 1;
            if i < n {
                segment.push(chars[i]);
                i += 1;
            }
            continue;
        }

        if ch == '&' && i + 1 < n && chars[i + 1] == '&' {
            result.push((None, segment.clone()));
            segment.clear();
            i += 2;
            while i < n && chars[i] == ' ' {
                i += 1;
            }
            let rest_str: String = chars[i..].iter().collect();
            let mut sub = split_by_list_ops_with_initial_op(&rest_str, ListOp::And);
            result.append(&mut sub);
            return result;
        }

        if ch == '|' && i + 1 < n && chars[i + 1] == '|' {
            result.push((None, segment.clone()));
            segment.clear();
            i += 2;
            while i < n && chars[i] == ' ' {
                i += 1;
            }
            let rest_str: String = chars[i..].iter().collect();
            let mut sub = split_by_list_ops_with_initial_op(&rest_str, ListOp::Or);
            result.append(&mut sub);
            return result;
        }

        if ch == ';' {
            result.push((None, segment.clone()));
            segment.clear();
            i += 1;
            while i < n && chars[i] == ' ' {
                i += 1;
            }
            let rest_str: String = chars[i..].iter().collect();
            let mut sub = split_by_list_ops_with_initial_op(&rest_str, ListOp::Semi);
            result.append(&mut sub);
            return result;
        }

        segment.push(ch);
        i += 1;
    }

    if !segment.trim().is_empty() || result.is_empty() {
        result.push((None, segment));
    }
    result
}

fn split_by_list_ops_with_initial_op(
    line: &str,
    initial_op: ListOp,
) -> Vec<(Option<ListOp>, String)> {
    let mut sub = split_by_list_ops(line);
    if let Some(first) = sub.first_mut() {
        first.0 = Some(initial_op);
    }
    sub
}

fn parse_pipeline(line: &str, state: &ShellState) -> Result<Pipeline> {
    let stages = split_by_pipe(line);
    if stages.is_empty() {
        return Err(RunboxError::Shell("empty pipeline".into()));
    }
    stages
        .iter()
        .map(|s| parse_single_command(s.trim(), state))
        .collect()
}

fn split_by_pipe(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut result = vec![];
    let mut current = String::new();
    let mut i = 0;
    let mut in_quotes = false;
    let mut quote_char = ' ';

    while i < n {
        let ch = chars[i];
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            }
            current.push(ch);
            i += 1;
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_quotes = true;
            quote_char = ch;
            current.push(ch);
            i += 1;
            continue;
        }
        if ch == '\\' {
            current.push(ch);
            i += 1;
            if i < n {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if ch == '|' {
            if i + 1 < n && chars[i + 1] == '|' {
                current.push(ch);
                i += 1;
                continue;
            }
            result.push(current.clone());
            current.clear();
            i += 1;
            continue;
        }
        current.push(ch);
        i += 1;
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

fn parse_single_command(line: &str, state: &ShellState) -> Result<Command> {
    let expanded = expand_vars(line, &state.env, state.last_exit);
    let raw_tokens = tokenize(&expanded);
    if raw_tokens.is_empty() {
        return Err(RunboxError::Shell("empty command".into()));
    }

    let mut env: Vec<(String, String)> = vec![];
    let mut args: Vec<String> = vec![];
    let mut redirect: Option<Redirect> = None;
    let mut program: Option<String> = None;

    let mut i = 0;
    while i < raw_tokens.len() {
        let tok = &raw_tokens[i];

        if tok == ">>" {
            i += 1;
            let file = raw_tokens.get(i).cloned().unwrap_or_default();
            redirect = Some(Redirect::Append(state.resolve(&file)));
            i += 1;
            continue;
        }
        if tok == ">" {
            i += 1;
            let file = raw_tokens.get(i).cloned().unwrap_or_default();
            redirect = Some(Redirect::Truncate(state.resolve(&file)));
            i += 1;
            continue;
        }
        if tok == "2>&1" {
            redirect = Some(Redirect::StderrToStdout);
            i += 1;
            continue;
        }
        if tok == "2>" {
            i += 1;
            let file = raw_tokens.get(i).cloned().unwrap_or_default();
            redirect = Some(Redirect::Stderr(state.resolve(&file)));
            i += 1;
            continue;
        }

        if program.is_none() {
            if let Some((k, v)) = tok.split_once('=')
                && k.chars().all(|c| c.is_alphanumeric() || c == '_')
                && !k.is_empty()
            {
                env.push((k.to_string(), v.to_string()));
                i += 1;
                continue;
            }
            program = Some(tok.clone());
        } else {
            args.push(tok.clone());
        }
        i += 1;
    }

    let program = program.ok_or_else(|| RunboxError::Shell("no command after env vars".into()))?;
    Ok(Command {
        program,
        args,
        env,
        stdin: None,
        redirect,
    })
}

fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = vec![];
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';
    let mut escape_next = false;
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let ch = chars[i];

        if escape_next {
            current.push(ch);
            escape_next = false;
            i += 1;
            continue;
        }
        if ch == '\\' && !in_quotes {
            escape_next = true;
            i += 1;
            continue;
        }

        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            } else {
                current.push(ch);
            }
            i += 1;
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_quotes = true;
            quote_char = ch;
            i += 1;
            continue;
        }

        if ch == '>' {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            if i + 1 < n && chars[i + 1] == '>' {
                tokens.push(">>".to_string());
                i += 2;
            } else {
                tokens.push(">".to_string());
                i += 1;
            }
            continue;
        }

        // Check for 2> or 2>&1 only when current is empty (standalone token)
        if ch == '2' && current.is_empty() && i + 1 < n && chars[i + 1] == '>' {
            if i + 3 < n && chars[i + 2] == '&' && chars[i + 3] == '1' {
                tokens.push("2>&1".to_string());
                i += 4;
            } else {
                tokens.push("2>".to_string());
                i += 2;
            }
            continue;
        }

        if ch == ' ' || ch == '\t' {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            i += 1;
            continue;
        }

        current.push(ch);
        i += 1;
    }
    if escape_next {
        current.push('\\');
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let cmd = Command::parse("bun run index.ts").unwrap();
        assert_eq!(cmd.program, "bun");
        assert_eq!(cmd.args, vec!["run", "index.ts"]);
    }

    #[test]
    fn parse_with_env() {
        let cmd = Command::parse("NODE_ENV=production bun run build.ts").unwrap();
        assert_eq!(
            cmd.env,
            vec![("NODE_ENV".to_string(), "production".to_string())]
        );
        assert_eq!(cmd.program, "bun");
    }

    #[test]
    fn detect_runtime() {
        let cmd = Command::parse("python3 main.py").unwrap();
        assert_eq!(RuntimeTarget::detect(&cmd), RuntimeTarget::Python);

        let cmd = Command::parse("git clone https://github.com/foo/bar").unwrap();
        assert_eq!(RuntimeTarget::detect(&cmd), RuntimeTarget::Git);

        let cmd = Command::parse("node index.js").unwrap();
        assert_eq!(RuntimeTarget::detect(&cmd), RuntimeTarget::Bun);
    }

    #[test]
    fn test_parse_simple() {
        let cmd = Command::parse("echo hello world").unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec!["hello", "world"]);
    }

    #[test]
    fn test_parse_quoted() {
        let cmd = Command::parse("echo 'hello world'").unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec!["hello world"]);
    }

    #[test]
    fn test_parse_env_vars() {
        let cmd = Command::parse("FOO=bar echo test").unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.env, vec![("FOO".to_string(), "bar".to_string())]);
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("/foo/bar/../baz"), "/foo/baz");
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path("/foo/./bar"), "/foo/bar");
    }

    #[test]
    fn test_resolve_relative() {
        let mut state = ShellState::new();
        state.set_cwd("/home/user");
        assert_eq!(state.resolve("foo"), "/home/user/foo");
        assert_eq!(state.resolve("/abs"), "/abs");
        assert_eq!(state.resolve("~"), "/home/user");
        assert_eq!(state.resolve("~/foo"), "/home/user/foo");
    }

    #[test]
    fn test_expand_vars() {
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        assert_eq!(expand_vars("$FOO", &env, 0), "bar");
        assert_eq!(expand_vars("${FOO}", &env, 0), "bar");
        assert_eq!(expand_vars("$?", &env, 42), "42");
        assert_eq!(expand_vars("$MISSING", &env, 0), "");
    }

    #[test]
    fn test_pipeline_split() {
        let state = ShellState::new();
        let cl = parse_command_list("echo foo | cat", &state).unwrap();
        assert_eq!(cl.first.len(), 2);
    }

    #[test]
    fn test_command_list_and() {
        let state = ShellState::new();
        let cl = parse_command_list("true && echo yes", &state).unwrap();
        assert_eq!(cl.rest.len(), 1);
        assert_eq!(cl.rest[0].0, ListOp::And);
    }

    #[test]
    fn test_redirect_parse() {
        let cmd = Command::parse("echo hi > /out.txt").unwrap();
        assert!(matches!(cmd.redirect, Some(Redirect::Truncate(_))));
    }
}
