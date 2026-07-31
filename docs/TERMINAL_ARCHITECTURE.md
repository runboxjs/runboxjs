# Arquitectura de Terminal en RunBox

## Resumen Ejecutivo

RunBox tiene **dos módulos de terminal** con propósitos diferentes y complementarios:

| Módulo | Propósito | Nivel | Analogía |
|---|---|---|---|
| `terminal.rs` | **I/O de bajo nivel** | Infraestructura | El "hardware" del terminal |
| `terminal_api.rs` | **Shell interactivo** | Aplicación | El "sistema operativo" del terminal |

---

## `terminal.rs` - Capa de I/O (Bajo Nivel)

### 📦 Propósito

Gestiona la **comunicación bidireccional** entre RunBox y el frontend (xterm.js). Es la capa de **transporte de datos**.

### 🎯 Responsabilidades

1. **Buffer de salida** - Almacena texto que debe mostrarse en el terminal
2. **Buffer de entrada** - Recibe teclas presionadas por el usuario
3. **Gestión de tamaño** - Maneja resize del terminal (cols/rows)
4. **Colores ANSI** - Aplica códigos de color (stderr en rojo, etc.)
5. **Historial básico** - Guarda comandos ejecutados

### 🔧 Componentes Principales

```rust
pub struct Terminal {
    output_buf: VecDeque<OutputChunk>,  // Cola de salida
    input_buf: VecDeque<InputChunk>,    // Cola de entrada
    pub size: TerminalSize,             // Dimensiones (cols x rows)
    capacity: usize,                    // Límite del buffer
    pub history: Vec<String>,           // Historial de comandos
}

pub struct OutputChunk {
    pub pid: Pid,           // Proceso que generó la salida
    pub data: String,       // Texto con códigos ANSI
    pub stream: Stream,     // Stdout o Stderr
}

pub struct InputChunk {
    pub pid: Option<Pid>,   // Proceso destino
    pub data: String,       // Texto ingresado
}
```

### 📝 API Pública

```rust
// Escritura (RunBox → xterm.js)
terminal.write_stdout(pid, "Hello World");
terminal.write_stderr(pid, "Error message");
terminal.write_prompt("/home/user");
terminal.write_banner();

// Lectura (xterm.js → RunBox)
terminal.input_push("ls -la", None);
let chunk = terminal.input_pop();

// Drenaje (polling desde JS)
let chunks = terminal.output_drain(); // Retorna y limpia el buffer

// Control
terminal.resize(120, 40);
terminal.clear();
terminal.move_cursor(10, 5);
```

### 🔄 Flujo de Datos

```
┌─────────────┐                    ┌──────────────┐
│  xterm.js   │                    │   RunBox     │
│  (Browser)  │                    │   (WASM)     │
└─────────────┘                    └──────────────┘
       │                                   │
       │  1. User types "ls"               │
       │──────────────────────────────────>│
       │     terminal.input_push()         │
       │                                   │
       │                                   │ 2. Process command
       │                                   │    terminal.write_stdout()
       │                                   │
       │  3. Poll for output               │
       │<──────────────────────────────────│
       │     terminal.output_drain()       │
       │                                   │
       │  4. Render in xterm.js            │
       │     xterm.write(chunk.data)       │
       │                                   │
```

### 💡 Ejemplo de Uso

```rust
use runbox::terminal::Terminal;

let mut terminal = Terminal::new(4096);

// Escribir output
terminal.write_stdout(1, "Hello from process 1\n");
terminal.write_stderr(2, "Error in process 2\n");

// Leer input
terminal.input_push("echo test", None);
if let Some(input) = terminal.input_pop() {
    println!("User typed: {}", input.data);
}

// Drenar output para enviar a xterm.js
let chunks = terminal.output_drain();
for chunk in chunks {
    // Enviar a xterm.js via postMessage o similar
    send_to_frontend(chunk);
}
```

---

## `terminal_api.rs` - Shell Interactivo (Alto Nivel)

### 📦 Propósito

Implementa un **shell completo tipo Bash** que interpreta y ejecuta comandos complejos. Es la capa de **lógica de negocio**.

### 🎯 Responsabilidades

1. **Parsing de comandos** - Tokeniza y construye AST
2. **Expansión de variables** - `$VAR`, `${VAR}`, `$HOME`, etc.
3. **Glob expansion** - `*.txt`, `file?.rs`, `[a-z]*`
4. **Pipes** - `cmd1 | cmd2 | cmd3`
5. **Redirección** - `>`, `>>`, `<`, `2>&1`
6. **Condicionales** - `&&`, `||`
7. **Secuencias** - `;`
8. **Background jobs** - `&`
9. **Builtins** - `cd`, `export`, `alias`, `exit`
10. **Gestión de sesión** - cwd, env, aliases, history

### 🔧 Componentes Principales

```rust
pub struct TerminalSession {
    pub vfs: Vfs,                       // Sistema de archivos virtual
    pub pm: ProcessManager,             // Gestor de procesos
    pub terminal: Terminal,             // ← USA terminal.rs
    pub cwd: String,                    // Directorio actual
    pub env: HashMap<String, String>,   // Variables de entorno
    pub aliases: HashMap<String, String>, // Aliases de comandos
    pub history: VecDeque<String>,      // Historial completo
    pub last_exit_code: i32,            // Exit code del último comando
}

enum CommandAst {
    Simple(Command),                    // ls -la
    Pipeline(Vec<CommandAst>),          // cmd1 | cmd2
    Sequence(Vec<CommandAst>),          // cmd1; cmd2
    And(Box<CommandAst>, Box<CommandAst>), // cmd1 && cmd2
    Or(Box<CommandAst>, Box<CommandAst>),  // cmd1 || cmd2
    Background(Box<CommandAst>),        // cmd &
    Redirect {                          // cmd > file
        node: Box<CommandAst>,
        redirects: Vec<Redirect>,
    },
}
```

### 📝 API Pública

```rust
// Crear sesión
let mut session = TerminalSession::new();

// Ejecutar comandos
session.exec("ls -la")?;
session.exec("echo $HOME")?;
session.exec("ls | grep .rs | wc -l")?;
session.exec("echo 'test' > /file.txt")?;
session.exec("cd /src && npm install")?;

// Obtener estado
let state = session.get_state();
println!("CWD: {}", state.cwd);
println!("Last exit code: {}", state.last_exit_code);

// Drenar output para xterm.js
let output_json = session.drain_output();
```

### 🔄 Flujo de Ejecución

```
User Input: "ls | grep .rs > output.txt"
       │
       ▼
┌──────────────────────────────────────────┐
│  1. TOKENIZE                             │
│     ["ls", "|", "grep", ".rs", ">",     │
│      "output.txt"]                       │
└──────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────┐
│  2. PARSE → AST                          │
│     Redirect {                           │
│       node: Pipeline([                   │
│         Simple("ls"),                    │
│         Simple("grep .rs")               │
│       ]),                                │
│       redirects: [                       │
│         { fd: 1, target: "output.txt" }  │
│       ]                                  │
│     }                                    │
└──────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────┐
│  3. EXPAND VARIABLES                     │
│     (ninguna en este caso)               │
└──────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────┐
│  4. EXECUTE AST                          │
│     a) Ejecutar "ls"                     │
│     b) Pasar stdout a "grep"             │
│     c) Redirigir stdout a archivo        │
└──────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────┐
│  5. WRITE TO TERMINAL                    │
│     terminal.write_stdout(...)           │
│     terminal.write_prompt(...)           │
└──────────────────────────────────────────┘
```

### 💡 Ejemplo de Uso

```rust
use runbox::terminal_api::TerminalSession;

let mut session = TerminalSession::new();

// Comandos simples
session.exec("mkdir /project")?;
session.exec("cd /project")?;
session.exec("touch README.md")?;

// Variables de entorno
session.exec("export NODE_ENV=production")?;
session.exec("echo $NODE_ENV")?;

// Pipes
session.exec("ls | grep .txt")?;

// Redirección
session.exec("echo 'Hello World' > /greeting.txt")?;
session.exec("cat /greeting.txt")?;

// Condicionales
session.exec("test -f /file.txt && echo 'exists' || echo 'not found'")?;

// Obtener estado
let state = session.get_state();
println!("Current directory: {}", state.cwd);
println!("Environment variables: {:?}", state.env);
```

---

## Relación Entre Ambos Módulos

### 📊 Diagrama de Capas

```
┌─────────────────────────────────────────────────────────────┐
│                        xterm.js                             │
│                     (Frontend UI)                           │
└─────────────────────────────────────────────────────────────┘
                            ▲ │
                            │ │ postMessage / WebSocket
                            │ ▼
┌─────────────────────────────────────────────────────────────┐
│                     terminal.rs                             │
│                  (I/O Transport Layer)                      │
│  • Buffers (input/output)                                   │
│  • ANSI colors                                              │
│  • Resize handling                                          │
└─────────────────────────────────────────────────────────────┘
                            ▲ │
                            │ │ write_stdout() / input_pop()
                            │ ▼
┌─────────────────────────────────────────────────────────────┐
│                   terminal_api.rs                           │
│                (Shell Logic Layer)                          │
│  • Command parsing                                          │
│  • Variable expansion                                       │
│  • Pipes & redirection                                      │
│  • Builtins (cd, export, alias)                             │
└─────────────────────────────────────────────────────────────┘
                            ▲ │
                            │ │ exec() / Runtime trait
                            │ ▼
┌─────────────────────────────────────────────────────────────┐
│                    Runtime Layer                            │
│  • BunRuntime (JS/TS)                                       │
│  • GitRuntime                                               │
│  • PythonRuntime                                            │
│  • PackageManagerRuntime (npm/pnpm/yarn)                    │
│  • ShellBuiltins (ls, cat, echo, etc.)                      │
└─────────────────────────────────────────────────────────────┘
                            ▲ │
                            │ │ read() / write()
                            │ ▼
┌─────────────────────────────────────────────────────────────┐
│                         VFS                                 │
│                  (Virtual File System)                      │
└─────────────────────────────────────────────────────────────┘
```

### 🔗 Integración

`TerminalSession` **contiene** una instancia de `Terminal`:

```rust
pub struct TerminalSession {
    pub terminal: Terminal,  // ← Composición
    // ... otros campos
}

impl TerminalSession {
    pub fn exec(&mut self, command: &str) -> Result<CommandResult> {
        // 1. Parsear comando
        let ast = self.parse_command(command)?;
        
        // 2. Ejecutar
        let result = self.execute_ast(&ast)?;
        
        // 3. Escribir output al terminal de bajo nivel
        self.terminal.write_stdout(result.pid, &result.stdout);
        self.terminal.write_stderr(result.pid, &result.stderr);
        self.terminal.write_prompt(&self.cwd);
        
        Ok(result)
    }
}
```

---

## Comparación Lado a Lado

| Aspecto | `terminal.rs` | `terminal_api.rs` |
|---|---|---|
| **Nivel** | Bajo nivel (I/O) | Alto nivel (Shell) |
| **Propósito** | Transporte de datos | Lógica de comandos |
| **Complejidad** | Simple (~200 líneas) | Complejo (~800 líneas) |
| **Estado** | Buffers, tamaño | VFS, env, cwd, aliases |
| **Parsing** | No | Sí (tokenizer + AST) |
| **Ejecución** | No | Sí (runtimes) |
| **Variables** | No | Sí ($VAR, $HOME, etc.) |
| **Pipes** | No | Sí |
| **Redirección** | No | Sí |
| **Builtins** | No | Sí (cd, export, alias) |
| **Dependencias** | Ninguna | VFS, ProcessManager, Runtimes |
| **Uso directo** | Raro | Común |

---

## Casos de Uso

### Cuándo Usar `terminal.rs`

✅ **Integración con xterm.js**
```typescript
// Polling de output
setInterval(() => {
  const chunks = runbox.terminal_drain();
  for (const chunk of chunks) {
    xterm.write(chunk.data);
  }
}, 16); // 60 FPS
```

✅ **Control de bajo nivel**
```rust
terminal.resize(120, 40);
terminal.clear();
terminal.write_stdout(1, "\x1b[32mGreen text\x1b[0m");
```

### Cuándo Usar `terminal_api.rs`

✅ **Ejecutar comandos del usuario**
```rust
let mut session = TerminalSession::new();
session.exec(user_input)?;
```

✅ **Scripts automatizados**
```rust
session.exec("mkdir /project")?;
session.exec("cd /project")?;
session.exec("npm init -y")?;
session.exec("npm install express")?;
```

✅ **IDE o playground**
```rust
// Usuario escribe en el editor
session.exec("node /app.js")?;

// Ver output
let output = session.drain_output();
```

---

## Analogía con Linux

Para entender mejor la relación:

| Componente RunBox | Equivalente Linux |
|---|---|
| `terminal.rs` | `/dev/tty` (dispositivo de terminal) |
| `terminal_api.rs` | `/bin/bash` (shell) |
| `Runtime` trait | Comandos ejecutables (`/bin/ls`, `/usr/bin/git`) |
| `VFS` | Sistema de archivos (`/`, `/home`, `/etc`) |

En Linux:
- El **dispositivo TTY** maneja I/O de bajo nivel (caracteres, control codes)
- El **shell (bash)** interpreta comandos, expande variables, maneja pipes

En RunBox:
- `terminal.rs` = TTY (I/O)
- `terminal_api.rs` = Bash (shell)

---

## Resumen

### `terminal.rs` 🔌
- **Qué es:** Capa de transporte I/O
- **Para qué:** Comunicación con xterm.js
- **Cuándo usarlo:** Integración frontend, control de bajo nivel

### `terminal_api.rs` 🐚
- **Qué es:** Shell interactivo completo
- **Para qué:** Ejecutar comandos complejos
- **Cuándo usarlo:** Siempre que el usuario escriba comandos

### Relación
`terminal_api.rs` **usa** `terminal.rs` internamente para escribir output.

---

## Ejemplo Completo de Integración

```rust
use runbox::terminal_api::TerminalSession;

// Crear sesión (incluye terminal.rs internamente)
let mut session = TerminalSession::new();

// Usuario escribe comando
let user_input = "ls | grep .rs > /output.txt";

// Ejecutar (terminal_api.rs parsea y ejecuta)
session.exec(user_input)?;

// Obtener output para xterm.js (terminal.rs)
let output_json = session.drain_output();

// Enviar a frontend
send_to_xterm(output_json);
```

---

## Conclusión

- **`terminal.rs`** = Infraestructura (cables y enchufes)
- **`terminal_api.rs`** = Aplicación (el shell que usas)

Ambos son necesarios y complementarios. `terminal_api.rs` no podría funcionar sin `terminal.rs`, y `terminal.rs` solo es útil cuando `terminal_api.rs` (u otro componente) escribe en él.
