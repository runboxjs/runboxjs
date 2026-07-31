# RunBox vs WebContainer: Análisis y Mejoras

## Resumen Ejecutivo

RunBox ya tiene una base sólida con VFS, múltiples runtimes y MCP. Para alcanzar y superar a WebContainer, necesita:

1. **Terminal Linux-like nativa** con shell completo (bash/zsh emulado)
2. **API JavaScript ergonómica** para integración fácil
3. **Mejor soporte de procesos** con pipes, redirección y jobs
4. **Networking real** con fetch/WebSocket desde el sandbox
5. **Performance optimizado** para proyectos grandes

---

## Estado Actual de RunBox

### ✅ Fortalezas

| Característica | Estado | Notas |
|---|---|---|
| VFS en memoria | ✅ Completo | B-tree, lazy loading, compresión |
| Múltiples runtimes | ✅ Completo | Bun, Node, Python, Git, npm/pnpm/yarn |
| MCP Server | ✅ Completo | Integración con Claude, Cursor, Zed |
| Hot Reload | ✅ Completo | HMR, CSS injection, preservación de estado |
| TypeScript | ✅ Completo | Stripping y ejecución nativa |
| Package managers | ✅ Completo | npm, pnpm, yarn, bun con lockfiles |
| Git completo | ✅ Completo | Clone, commit, push, pull, merge |
| WASM + Native | ✅ Completo | Dual-target architecture |

### ⚠️ Áreas de Mejora vs WebContainer

| Característica | RunBox | WebContainer | Gap |
|---|---|---|---|
| **Shell interactivo** | Básico (builtins) | Completo (bash-like) | 🔴 Grande |
| **API JavaScript** | WASM directo | Ergonómica | 🟡 Medio |
| **Procesos** | Simple | Completo con pipes | 🔴 Grande |
| **Networking** | Limitado | fetch/WebSocket | 🟡 Medio |
| **Terminal UX** | Funcional | Pulida | 🟡 Medio |
| **Documentación** | Técnica | User-friendly | 🟡 Medio |

---

## Plan de Mejoras: Terminal Linux-like Nativa

### Fase 1: Shell Completo (Prioridad ALTA)

#### 1.1 Emulador de Shell Bash-like

**Archivo nuevo:** `src/runtime/bash_shell.rs`

```rust
pub struct BashShell {
    cwd: String,
    env: HashMap<String, String>,
    aliases: HashMap<String, String>,
    functions: HashMap<String, Vec<String>>,
    history: Vec<String>,
    last_exit_code: i32,
}

impl BashShell {
    // Características esenciales:
    // - Variables de entorno ($VAR, ${VAR})
    // - Expansión de paths (*, ?, ~)
    // - Command substitution $(cmd) y `cmd`
    // - Pipes (|), redirección (>, >>, <, 2>&1)
    // - Background jobs (&)
    // - Condicionales (&&, ||)
    // - Secuencias (;)
    // - Subshells (())
    // - Aliases y funciones
    // - Control de jobs (fg, bg, jobs)
}
```

**Comandos adicionales necesarios:**

```rust
// src/runtime/shell_builtins.rs - EXPANDIR

// Navegación
cd, pushd, popd, dirs

// Archivos
cp, mv, find, grep, head, tail, wc, sort, uniq, cut, sed, awk

// Sistema
env, export, unset, alias, unalias, source, exec, exit

// Procesos
ps, kill, jobs, fg, bg, wait

// Texto
printf, test, [, [[

// Utilidades
date, sleep, true, false, yes, seq, basename, dirname
```

#### 1.2 Parser de Comandos Avanzado

**Archivo:** `src/shell.rs` - REESCRIBIR

```rust
pub enum ShellNode {
    Simple(Command),
    Pipeline(Vec<ShellNode>),
    Sequence(Vec<ShellNode>),
    And(Box<ShellNode>, Box<ShellNode>),
    Or(Box<ShellNode>, Box<ShellNode>),
    Background(Box<ShellNode>),
    Subshell(Box<ShellNode>),
    Redirect {
        node: Box<ShellNode>,
        redirects: Vec<Redirect>,
    },
}

pub struct Redirect {
    pub fd: i32,        // 0=stdin, 1=stdout, 2=stderr
    pub target: RedirectTarget,
}

pub enum RedirectTarget {
    File { path: String, append: bool },
    Fd(i32),
    Pipe,
}
```

**Ejemplo de parsing:**

```bash
# Input
ls -la | grep "\.rs$" | wc -l > count.txt 2>&1

# AST
Redirect {
    node: Pipeline([
        Simple(Command { program: "ls", args: ["-la"] }),
        Simple(Command { program: "grep", args: ["\\.rs$"] }),
        Simple(Command { program: "wc", args: ["-l"] }),
    ]),
    redirects: [
        Redirect { fd: 1, target: File { path: "count.txt", append: false } },
        Redirect { fd: 2, target: Fd(1) },
    ],
}
```

#### 1.3 Sistema de Pipes

**Archivo nuevo:** `src/pipe.rs`

```rust
pub struct PipeManager {
    pipes: HashMap<PipeId, Pipe>,
}

pub struct Pipe {
    buffer: VecDeque<u8>,
    capacity: usize,
    closed_write: bool,
    closed_read: bool,
}

impl Pipe {
    pub fn write(&mut self, data: &[u8]) -> Result<usize>;
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    pub fn close_write(&mut self);
    pub fn close_read(&mut self);
}
```

**Ejecución de pipeline:**

```rust
// ls | grep foo | wc -l
// 1. Crear 2 pipes: pipe1 (ls→grep), pipe2 (grep→wc)
// 2. Ejecutar ls con stdout=pipe1.write
// 3. Ejecutar grep con stdin=pipe1.read, stdout=pipe2.write
// 4. Ejecutar wc con stdin=pipe2.read, stdout=terminal
```

#### 1.4 Variables de Entorno y Expansión

```rust
impl BashShell {
    fn expand_variables(&self, input: &str) -> String {
        // $VAR, ${VAR}, ${VAR:-default}, ${VAR:=default}
        // $?, $!, $$, $0, $1, $@, $*
    }
    
    fn expand_globs(&self, pattern: &str, vfs: &Vfs) -> Vec<String> {
        // *.rs, **/*.ts, file?.txt, [abc].txt
    }
    
    fn expand_tilde(&self, path: &str) -> String {
        // ~/file → /home/user/file
    }
    
    fn expand_command_substitution(&mut self, cmd: &str) -> Result<String> {
        // $(ls) o `ls`
    }
}
```

### Fase 2: API JavaScript Ergonómica (Prioridad ALTA)

#### 2.1 API Estilo WebContainer

**Archivo nuevo:** `src/wasm_api.rs`

```typescript
// API objetivo (TypeScript)
interface RunboxAPI {
  // Filesystem
  fs: {
    readFile(path: string, encoding?: 'utf-8' | 'buffer'): Promise<string | Uint8Array>;
    writeFile(path: string, content: string | Uint8Array): Promise<void>;
    readdir(path: string): Promise<string[]>;
    mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
    rm(path: string, options?: { recursive?: boolean }): Promise<void>;
    watch(path: string, callback: (event: FileChangeEvent) => void): Watcher;
  };
  
  // Shell
  spawn(command: string, args?: string[], options?: SpawnOptions): Process;
  
  // Terminal
  terminal: {
    attach(element: HTMLElement): void;
    write(data: string): void;
    onData(callback: (data: string) => void): void;
    resize(cols: number, rows: number): void;
  };
  
  // Networking
  fetch(url: string, init?: RequestInit): Promise<Response>;
  
  // Hot reload
  on(event: 'reload' | 'error', callback: (data: any) => void): void;
}

// Uso
const runbox = await Runbox.create();

// Escribir archivo
await runbox.fs.writeFile('/index.js', 'console.log("Hello")');

// Ejecutar comando con streaming
const proc = runbox.spawn('npm', ['install']);
proc.stdout.on('data', chunk => console.log(chunk));
await proc.exit;

// Terminal interactiva
runbox.terminal.attach(document.getElementById('term'));
```

#### 2.2 Clase Process con Streams

```rust
// src/process.rs - EXPANDIR

pub struct Process {
    pub pid: Pid,
    pub stdin: ProcessStream,
    pub stdout: ProcessStream,
    pub stderr: ProcessStream,
    pub exit_code: Option<i32>,
}

pub struct ProcessStream {
    buffer: Arc<Mutex<VecDeque<u8>>>,
    callbacks: Vec<Box<dyn Fn(&[u8])>>,
}

impl ProcessStream {
    pub fn write(&mut self, data: &[u8]);
    pub fn read(&mut self, buf: &mut [u8]) -> usize;
    pub fn on_data(&mut self, callback: impl Fn(&[u8]) + 'static);
}
```

### Fase 3: Mejoras de Terminal UX (Prioridad MEDIA)

#### 3.1 Autocompletado Inteligente

```rust
// src/terminal_autocomplete.rs

pub struct Autocomplete {
    vfs: Arc<Vfs>,
    commands: HashSet<String>,
    history: Vec<String>,
}

impl Autocomplete {
    pub fn suggest(&self, input: &str, cursor: usize) -> Vec<Suggestion> {
        // 1. Detectar contexto (comando, path, variable)
        // 2. Generar sugerencias relevantes
        // 3. Ordenar por relevancia
    }
}

pub struct Suggestion {
    pub text: String,
    pub description: Option<String>,
    pub icon: SuggestionIcon,
}
```

**Tipos de autocompletado:**

- Comandos disponibles (builtins + PATH)
- Paths del VFS (con fuzzy matching)
- Variables de entorno
- Opciones de comandos (--help parsing)
- Historial de comandos (Ctrl+R)

#### 3.2 Prompt Personalizable

```rust
// src/terminal_prompt.rs

pub struct PromptConfig {
    pub format: String,  // "user@host:cwd$ "
    pub colors: PromptColors,
    pub git_status: bool,
    pub exit_code_indicator: bool,
}

impl PromptConfig {
    pub fn render(&self, shell: &BashShell, vfs: &Vfs) -> String {
        // Interpreta variables: {user}, {host}, {cwd}, {git_branch}
        // Aplica colores ANSI
        // Muestra indicador de exit code si != 0
    }
}
```

#### 3.3 Keybindings Avanzados

```typescript
// Implementar en el lado JS (xterm.js addons)

terminal.attachCustomKeyEventHandler((event) => {
  // Ctrl+C - SIGINT
  if (event.ctrlKey && event.key === 'c') {
    runbox.kill_process(currentPid, 'SIGINT');
    return false;
  }
  
  // Ctrl+D - EOF
  if (event.ctrlKey && event.key === 'd') {
    runbox.terminal_input('\x04', currentPid);
    return false;
  }
  
  // Ctrl+Z - SIGTSTP (suspend)
  if (event.ctrlKey && event.key === 'z') {
    runbox.suspend_process(currentPid);
    return false;
  }
  
  // Ctrl+R - Reverse search
  if (event.ctrlKey && event.key === 'r') {
    enterReverseSearchMode();
    return false;
  }
  
  // Tab - Autocomplete
  if (event.key === 'Tab') {
    const suggestions = runbox.autocomplete(currentInput, cursorPos);
    showSuggestions(suggestions);
    return false;
  }
  
  return true;
});
```

### Fase 4: Networking Real (Prioridad MEDIA)

#### 4.1 Fetch desde el Sandbox

```rust
// src/network.rs - EXPANDIR

impl RunboxInstance {
    pub fn sandbox_fetch(&self, url: &str, init: FetchInit) -> Result<FetchResponse> {
        // En WASM: usar web_sys::fetch con CORS
        // En Native: usar reqwest
        
        // Aplicar políticas de seguridad:
        // - Whitelist de dominios
        // - Rate limiting
        // - Timeout
    }
}
```

**API JavaScript:**

```typescript
// Dentro del sandbox
const response = await fetch('https://api.github.com/users/octocat');
const data = await response.json();
console.log(data);
```

#### 4.2 WebSocket desde el Sandbox

```rust
// src/websocket.rs - EXPANDIR

pub struct SandboxWebSocket {
    url: String,
    state: WebSocketState,
    message_queue: VecDeque<WebSocketMessage>,
}

impl SandboxWebSocket {
    pub fn connect(url: &str) -> Result<Self>;
    pub fn send(&mut self, data: WebSocketMessage) -> Result<()>;
    pub fn recv(&mut self) -> Option<WebSocketMessage>;
    pub fn close(&mut self) -> Result<()>;
}
```

### Fase 5: Performance (Prioridad BAJA)

#### 5.1 Worker Pool para Comandos

```rust
// src/worker_pool.rs

pub struct WorkerPool {
    workers: Vec<Worker>,
    queue: VecDeque<Job>,
}

// Ejecutar comandos pesados en workers separados
// para no bloquear el thread principal
```

#### 5.2 Caché de Comandos Compilados

```rust
// src/command_cache.rs

pub struct CommandCache {
    cache: HashMap<String, CompiledCommand>,
}

// Cachear el AST parseado de comandos frecuentes
// para evitar re-parsing
```

---

## Comparación Final: RunBox Mejorado vs WebContainer

| Característica | WebContainer | RunBox Actual | RunBox Mejorado |
|---|---|---|---|
| Shell interactivo | ✅ Bash-like | ⚠️ Básico | ✅ Bash completo |
| Pipes y redirección | ✅ | ❌ | ✅ |
| Variables de entorno | ✅ | ⚠️ Parcial | ✅ |
| Glob expansion | ✅ | ❌ | ✅ |
| Job control | ✅ | ❌ | ✅ |
| Autocompletado | ✅ | ❌ | ✅ |
| API ergonómica | ✅ | ⚠️ WASM directo | ✅ |
| Networking | ✅ | ⚠️ Limitado | ✅ |
| Git completo | ❌ Básico | ✅ | ✅ |
| Python runtime | ❌ | ✅ | ✅ |
| MCP Server | ❌ | ✅ | ✅ |
| AI Integration | ❌ | ✅ | ✅ |
| Dual target (WASM+Native) | ❌ | ✅ | ✅ |
| Open Source | ❌ | ✅ | ✅ |

---

## Prioridades de Implementación

### Sprint 1 (2 semanas) - Terminal Funcional
1. ✅ Parser de comandos con pipes básicos
2. ✅ Redirección de I/O (>, >>, <, 2>&1)
3. ✅ Variables de entorno ($VAR)
4. ✅ Comandos adicionales (cp, mv, grep, find)

### Sprint 2 (2 semanas) - Shell Completo
1. ✅ Glob expansion (*, ?, [])
2. ✅ Command substitution $(cmd)
3. ✅ Condicionales (&&, ||)
4. ✅ Background jobs (&)
5. ✅ Job control (fg, bg, jobs)

### Sprint 3 (1 semana) - API JavaScript
1. ✅ Wrapper ergonómico sobre WASM
2. ✅ Clase Process con streams
3. ✅ API de filesystem estilo Node.js
4. ✅ Documentación y ejemplos

### Sprint 4 (1 semana) - UX Terminal
1. ✅ Autocompletado básico
2. ✅ Historial con Ctrl+R
3. ✅ Prompt personalizable
4. ✅ Keybindings avanzados

### Sprint 5 (1 semana) - Networking
1. ✅ fetch() desde sandbox
2. ✅ WebSocket support
3. ✅ Políticas de seguridad

---

## Conclusión

RunBox tiene una base técnica **superior** a WebContainer en muchos aspectos (Git, Python, MCP, AI). Las mejoras propuestas lo convertirían en:

1. **Más completo** - Shell bash-like con todas las características
2. **Más fácil de usar** - API JavaScript ergonómica
3. **Más potente** - Networking real, mejor performance
4. **Más abierto** - Open source, extensible, dual-target

**Ventaja competitiva clave:** RunBox puede ejecutarse tanto en browser (WASM) como en servidor/CLI (Native), algo que WebContainer no puede hacer.
