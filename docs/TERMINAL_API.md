# Terminal API - Guía Completa

## Introducción

La Terminal API de RunBox proporciona una interfaz de línea de comandos completa similar a Linux/Bash, ejecutándose completamente en el navegador o en modo nativo. Soporta características avanzadas como pipes, redirección, variables de entorno, glob expansion y más.

---

## Inicio Rápido

### Uso en JavaScript/TypeScript (WASM)

```typescript
import init, { TerminalSession } from 'runboxjs';

// Inicializar WASM
await init();

// Crear sesión de terminal
const terminal = new TerminalSession();

// Ejecutar comandos
const result = await terminal.exec('ls -la');
console.log(result.stdout);

// Comandos con pipes
await terminal.exec('ls | grep .rs | wc -l');

// Redirección
await terminal.exec('echo "Hello World" > /greeting.txt');
await terminal.exec('cat /greeting.txt');

// Variables de entorno
await terminal.exec('export MY_VAR=hello');
await terminal.exec('echo $MY_VAR');

// Condicionales
await terminal.exec('test -f /file.txt && echo "exists" || echo "not found"');
```

### Uso en Rust (Nativo)

```rust
use runbox::terminal_api::TerminalSession;

fn main() {
    let mut terminal = TerminalSession::new();
    
    // Ejecutar comandos
    let result = terminal.exec("ls -la").unwrap();
    println!("{}", String::from_utf8_lossy(&result.stdout));
    
    // Pipes
    terminal.exec("ls | grep .rs").unwrap();
    
    // Redirección
    terminal.exec("echo 'test' > /output.txt").unwrap();
}
```

---

## Características

### 1. Comandos Básicos (Shell Builtins)

```bash
# Navegación
cd /path/to/dir
pwd

# Archivos
ls /
ls -la /src
cat /file.txt
touch /newfile.txt
mkdir /mydir
rm /file.txt
cp /source.txt /dest.txt
mv /old.txt /new.txt

# Texto
echo "Hello World"
printf "Formatted: %s\n" "text"

# Sistema
env                    # Listar variables de entorno
export VAR=value       # Definir variable
alias ll='ls -la'      # Crear alias
exit                   # Salir
```

### 2. Pipes (|)

Conecta la salida de un comando con la entrada del siguiente:

```bash
# Contar archivos .rs
ls | grep .rs | wc -l

# Buscar en archivos
cat /src/main.rs | grep "fn main"

# Ordenar y filtrar
ls -la | sort | head -n 10
```

### 3. Redirección de I/O

```bash
# Redirigir stdout a archivo (sobrescribir)
echo "Hello" > /output.txt

# Redirigir stdout a archivo (append)
echo "World" >> /output.txt

# Redirigir stdin desde archivo
cat < /input.txt

# Redirigir stderr a stdout
command 2>&1

# Redirigir stderr a archivo
command 2> /errors.txt

# Redirigir todo a archivo
command > /output.txt 2>&1
```

### 4. Variables de Entorno

```bash
# Definir variable
export MY_VAR="hello"

# Usar variable
echo $MY_VAR
echo ${MY_VAR}

# Variables especiales
echo $?        # Exit code del último comando
echo $$        # PID del proceso actual
echo $PWD      # Directorio actual
echo $HOME     # Directorio home

# Variable en comando
MY_VAR=value command args
```

### 5. Glob Expansion

```bash
# Wildcard *
ls *.txt           # Todos los archivos .txt
ls src/*.rs        # Todos los .rs en src/

# Single char ?
ls file?.txt       # file1.txt, file2.txt, etc.

# Character class []
ls file[123].txt   # file1.txt, file2.txt, file3.txt
ls [a-z]*.txt      # Archivos que empiezan con letra minúscula
```

### 6. Condicionales

```bash
# AND (&&) - ejecuta el segundo solo si el primero tiene éxito
test -f /file.txt && echo "File exists"

# OR (||) - ejecuta el segundo solo si el primero falla
test -f /file.txt || echo "File not found"

# Combinados
mkdir /mydir && cd /mydir && touch file.txt
```

### 7. Secuencias (;)

```bash
# Ejecutar múltiples comandos en secuencia
cd /src; ls; pwd

# Independiente del exit code
false; echo "This runs anyway"
```

### 8. Background Jobs (&)

```bash
# Ejecutar en background (no bloquea el terminal)
npm install &

# Múltiples jobs
npm run build & npm run test &
```

### 9. Expansión de Tilde (~)

```bash
# ~ se expande a $HOME
cd ~
ls ~/documents
cat ~/.bashrc
```

---

## Runtimes Soportados

### JavaScript/TypeScript

```bash
# Bun runtime
bun run index.ts
bun install
bun add express
bun test

# Node.js
node index.js
node --version

# TypeScript directo
tsx script.ts
ts-node app.ts
```

### Package Managers

```bash
# npm
npm init -y
npm install express
npm run dev
npm list
npm update

# pnpm
pnpm install
pnpm add react
pnpm run build

# yarn
yarn install
yarn add vue
yarn run start
```

### Git

```bash
# Inicializar repo
git init
git config user.name "Your Name"
git config user.email "you@example.com"

# Staging y commits
git add .
git commit -m "Initial commit"
git status
git log

# Branches
git branch feature
git checkout feature
git merge main

# Remote
git remote add origin https://github.com/user/repo
git push origin main
git pull origin main
git clone https://github.com/user/repo
```

### Python

```bash
# Ejecutar scripts
python script.py
python3 app.py

# pip
pip install requests
pip list
pip show requests
pip freeze > requirements.txt
```

---

## API de Sesión

### TerminalSession

```typescript
interface TerminalSession {
  // Ejecutar comando
  exec(command: string): Promise<CommandResult>;
  
  // Obtener output del terminal
  drainOutput(): string;
  
  // Obtener estado de la sesión
  getState(): SessionState;
  
  // Propiedades
  cwd: string;                          // Directorio actual
  env: Map<string, string>;             // Variables de entorno
  history: string[];                    // Historial de comandos
  lastExitCode: number;                 // Exit code del último comando
}

interface CommandResult {
  pid: number;
  stdout: Uint8Array;
  stderr: Uint8Array;
  exitCode: number;
}

interface SessionState {
  cwd: string;
  env: Record<string, string>;
  lastExitCode: number;
  history: string[];
}
```

### Ejemplo Avanzado

```typescript
import { TerminalSession } from 'runboxjs';

const terminal = new TerminalSession();

// Configurar entorno
terminal.exec('export NODE_ENV=production');
terminal.exec('export API_URL=https://api.example.com');

// Crear estructura de proyecto
await terminal.exec('mkdir -p /src/components');
await terminal.exec('mkdir -p /src/utils');

// Crear archivos
await terminal.exec('echo "export const API_URL = process.env.API_URL" > /src/config.ts');

// Instalar dependencias
await terminal.exec('npm init -y');
await terminal.exec('npm install react react-dom');

// Ejecutar build
const buildResult = await terminal.exec('npm run build');
if (buildResult.exitCode !== 0) {
  console.error('Build failed:', buildResult.stderr);
}

// Verificar output
const files = await terminal.exec('ls -la /dist');
console.log(files.stdout);

// Obtener estado
const state = terminal.getState();
console.log('Current directory:', state.cwd);
console.log('Environment:', state.env);
console.log('Command history:', state.history);
```

---

## Integración con xterm.js

```typescript
import { Terminal as XTerm } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { TerminalSession } from 'runboxjs';

// Crear terminal visual
const xterm = new XTerm({
  cursorBlink: true,
  theme: {
    background: '#1a1b1e',
    foreground: '#d4d4d4',
  },
});

const fitAddon = new FitAddon();
xterm.loadAddon(fitAddon);
xterm.open(document.getElementById('terminal'));
fitAddon.fit();

// Crear sesión de RunBox
const session = new TerminalSession();

// Conectar input del usuario
let currentLine = '';
xterm.onData(async (data) => {
  if (data === '\r') {
    // Enter - ejecutar comando
    xterm.write('\r\n');
    
    const result = await session.exec(currentLine);
    
    // Mostrar output
    if (result.stdout.length > 0) {
      xterm.write(new TextDecoder().decode(result.stdout));
    }
    if (result.stderr.length > 0) {
      xterm.write('\x1b[31m' + new TextDecoder().decode(result.stderr) + '\x1b[0m');
    }
    
    // Nuevo prompt
    const state = session.getState();
    xterm.write(`\x1b[32muser@runbox\x1b[0m:\x1b[34m${state.cwd}\x1b[0m$ `);
    
    currentLine = '';
  } else if (data === '\x7f') {
    // Backspace
    if (currentLine.length > 0) {
      currentLine = currentLine.slice(0, -1);
      xterm.write('\b \b');
    }
  } else {
    // Carácter normal
    currentLine += data;
    xterm.write(data);
  }
});

// Prompt inicial
xterm.write('\x1b[1;35m  RunBox Terminal\x1b[0m\r\n');
xterm.write('  Type a command to get started.\r\n\r\n');
xterm.write('\x1b[32muser@runbox\x1b[0m:\x1b[34m/\x1b[0m$ ');

// Resize handler
new ResizeObserver(() => {
  fitAddon.fit();
}).observe(document.getElementById('terminal'));
```

---

## Casos de Uso

### 1. IDE en el Navegador

```typescript
// Crear proyecto React
await terminal.exec('mkdir /my-app && cd /my-app');
await terminal.exec('npm init -y');
await terminal.exec('npm install react react-dom');

// Crear archivos
await terminal.exec('echo "import React from \'react\'" > /my-app/src/App.tsx');

// Build y preview
await terminal.exec('npm run build');
```

### 2. Tutorial Interactivo

```typescript
// Guiar al usuario paso a paso
const steps = [
  'mkdir /tutorial',
  'cd /tutorial',
  'touch index.js',
  'echo "console.log(\'Hello\')" > index.js',
  'node index.js',
];

for (const step of steps) {
  console.log(`Ejecutando: ${step}`);
  const result = await terminal.exec(step);
  console.log(result.stdout);
}
```

### 3. CI/CD en el Navegador

```typescript
// Pipeline de CI
const pipeline = async () => {
  // Checkout
  await terminal.exec('git clone https://github.com/user/repo');
  await terminal.exec('cd /repo');
  
  // Install
  const install = await terminal.exec('npm install');
  if (install.exitCode !== 0) throw new Error('Install failed');
  
  // Lint
  const lint = await terminal.exec('npm run lint');
  if (lint.exitCode !== 0) throw new Error('Lint failed');
  
  // Test
  const test = await terminal.exec('npm test');
  if (test.exitCode !== 0) throw new Error('Tests failed');
  
  // Build
  const build = await terminal.exec('npm run build');
  if (build.exitCode !== 0) throw new Error('Build failed');
  
  console.log('✅ Pipeline completed successfully');
};
```

### 4. Playground de Código

```typescript
// Ejecutar código del usuario de forma segura
const userCode = `
console.log('Hello from user code');
const result = 2 + 2;
console.log('Result:', result);
`;

await terminal.exec(`echo "${userCode}" > /user-script.js`);
const result = await terminal.exec('node /user-script.js');
console.log(result.stdout);
```

---

## Comparación con WebContainer

| Característica | RunBox Terminal | WebContainer |
|---|---|---|
| Shell interactivo | ✅ Bash-like completo | ✅ |
| Pipes | ✅ | ✅ |
| Redirección | ✅ | ✅ |
| Variables de entorno | ✅ | ✅ |
| Glob expansion | ✅ | ✅ |
| Job control | ✅ (background) | ✅ |
| Git completo | ✅ | ⚠️ Básico |
| Python runtime | ✅ | ❌ |
| Dual target (WASM+Native) | ✅ | ❌ |
| Open source | ✅ | ❌ |
| MCP integration | ✅ | ❌ |
| AI tools | ✅ | ❌ |

---

## Próximas Características

### En Desarrollo

- [ ] Autocompletado inteligente (Tab)
- [ ] Historial con búsqueda (Ctrl+R)
- [ ] Señales (SIGINT, SIGTERM, SIGTSTP)
- [ ] Job control completo (fg, bg, jobs)
- [ ] Subshells (())
- [ ] Command substitution avanzado
- [ ] Funciones de shell
- [ ] Scripts .sh ejecutables

### Roadmap

- [ ] Prompt personalizable (PS1)
- [ ] Colores y temas
- [ ] Plugins de terminal
- [ ] Integración con LSP
- [ ] Debugging integrado

---

## Troubleshooting

### Comando no encontrado

```typescript
// Verificar que el comando existe
const result = await terminal.exec('nonexistent');
// result.exitCode === 1
// result.stderr === "nonexistent: command not found"
```

### Path no existe

```typescript
// Verificar antes de cd
await terminal.exec('test -d /mydir && cd /mydir || echo "Directory not found"');
```

### Variable no definida

```typescript
// Usar valor por defecto
await terminal.exec('echo ${MY_VAR:-default_value}');
```

---

## Contribuir

¿Quieres agregar más comandos o características? Consulta [DEVELOPMENT.md](./DEVELOPMENT.md) para guías de contribución.

---

## Licencia

MIT
