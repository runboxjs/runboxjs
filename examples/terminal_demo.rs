/// Ejemplo de uso de la Terminal API de RunBox
///
/// Este ejemplo demuestra las capacidades avanzadas de la terminal:
/// - Pipes
/// - Redirección
/// - Variables de entorno
/// - Glob expansion
/// - Condicionales
/// - Background jobs
use runbox::terminal_api::TerminalSession;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 RunBox Terminal Demo\n");

    let mut terminal = TerminalSession::new();

    // ═══════════════════════════════════════════════════════════════════════
    // 1. Comandos Básicos
    // ═══════════════════════════════════════════════════════════════════════

    println!("📁 1. Comandos Básicos");
    println!("─────────────────────");

    // Crear estructura de directorios
    terminal.exec("mkdir -p /project/src")?;
    terminal.exec("mkdir -p /project/tests")?;

    // Crear archivos
    terminal.exec("touch /project/README.md")?;
    terminal.exec("touch /project/src/main.rs")?;
    terminal.exec("touch /project/src/lib.rs")?;
    terminal.exec("touch /project/tests/integration.rs")?;

    // Listar archivos
    let result = terminal.exec("ls /project")?;
    println!("Archivos en /project:");
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // ═══════════════════════════════════════════════════════════════════════
    // 2. Redirección de I/O
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n📝 2. Redirección de I/O");
    println!("─────────────────────");

    // Escribir contenido a archivo
    terminal.exec("echo Hello World > /project/src/main.rs")?;
    terminal.exec("echo This is RunBox >> /project/src/main.rs")?;
    terminal.exec("echo Terminal Demo >> /project/src/main.rs")?;

    // Leer archivo
    let result = terminal.exec("cat /project/src/main.rs")?;
    println!("Contenido de main.rs:");
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // ═══════════════════════════════════════════════════════════════════════
    // 3. Variables de Entorno
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n🔧 3. Variables de Entorno");
    println!("─────────────────────────");

    // Definir variables
    terminal.exec("export PROJECT_NAME=RunBox")?;
    terminal.exec("export VERSION=1.1.0")?;
    terminal.exec("export AUTHOR=RunBoxTeam")?;

    // Usar variables
    let result = terminal.exec("echo Project: $PROJECT_NAME")?;
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // Variable especial $PWD
    let result = terminal.exec("echo Current directory: $PWD")?;
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // ═══════════════════════════════════════════════════════════════════════
    // 4. Pipes
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n🔗 4. Pipes");
    println!("──────────");

    // Crear más archivos para demostrar pipes
    terminal.exec("touch /project/file1.txt")?;
    terminal.exec("touch /project/file2.rs")?;
    terminal.exec("touch /project/file3.rs")?;
    terminal.exec("touch /project/file4.md")?;

    // Contar archivos .rs
    let result = terminal.exec("ls /project | grep .rs")?;
    println!("Archivos .rs encontrados:");
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // ═══════════════════════════════════════════════════════════════════════
    // 5. Glob Expansion
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n🌟 5. Glob Expansion");
    println!("───────────────────");

    // Listar todos los .rs
    let result = terminal.exec("ls /project/*.rs")?;
    println!("Archivos *.rs:");
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // Listar todos los archivos en src/
    let result = terminal.exec("ls /project/src/*")?;
    println!("\nArchivos en src/:");
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // ═══════════════════════════════════════════════════════════════════════
    // 6. Condicionales
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n⚡ 6. Condicionales");
    println!("─────────────────");

    // AND (&&) - ejecuta el segundo solo si el primero tiene éxito
    let result = terminal.exec("ls /project/README.md && echo README exists!")?;
    println!("Test con &&:");
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // OR (||) - ejecuta el segundo solo si el primero falla
    let result = terminal.exec("ls /project/nonexistent.txt || echo File not found")?;
    println!("\nTest con ||:");
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // ═══════════════════════════════════════════════════════════════════════
    // 7. Secuencias
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n📋 7. Secuencias de Comandos");
    println!("───────────────────────────");

    // Ejecutar múltiples comandos en secuencia
    terminal.exec("cd /project; pwd; ls")?;
    let state = terminal.get_state();
    println!("Directorio actual después de cd: {}", state.cwd);

    // ═══════════════════════════════════════════════════════════════════════
    // 8. Navegación con cd
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n🧭 8. Navegación con cd");
    println!("──────────────────────");

    terminal.exec("cd /project/src")?;
    let state = terminal.get_state();
    println!("Cambiado a: {}", state.cwd);

    terminal.exec("cd ..")?;
    let state = terminal.get_state();
    println!("Volviendo atrás: {}", state.cwd);

    terminal.exec("cd ~")?;
    let state = terminal.get_state();
    println!("Home directory: {}", state.cwd);

    // ═══════════════════════════════════════════════════════════════════════
    // 9. Aliases
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n🔖 9. Aliases");
    println!("────────────");

    terminal.exec("alias ll='ls -la'")?;
    terminal.exec("alias gs='git status'")?;

    let result = terminal.exec("alias")?;
    println!("Aliases definidos:");
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // ═══════════════════════════════════════════════════════════════════════
    // 10. Historial de Comandos
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n📜 10. Historial de Comandos");
    println!("───────────────────────────");

    let state = terminal.get_state();
    println!("Últimos 5 comandos ejecutados:");
    for (i, cmd) in state.history.iter().rev().take(5).enumerate() {
        println!("  {}. {}", i + 1, cmd);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 11. Workflow Completo: Proyecto Node.js
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n🚀 11. Workflow Completo: Proyecto Node.js");
    println!("──────────────────────────────────────────");

    // Crear proyecto
    terminal.exec("mkdir /my-app && cd /my-app")?;

    // Inicializar package.json
    terminal.exec("npm init -y")?;
    println!("✅ Proyecto inicializado");

    // Crear archivo principal
    terminal.exec("echo console.log(Hello from RunBox) > /my-app/index.js")?;
    println!("✅ Archivo index.js creado");

    // Ejecutar
    let result = terminal.exec("node /my-app/index.js")?;
    println!("📤 Output:");
    println!("{}", String::from_utf8_lossy(&result.stdout));

    // ═══════════════════════════════════════════════════════════════════════
    // 12. Estado Final
    // ═══════════════════════════════════════════════════════════════════════

    println!("\n📊 12. Estado Final de la Sesión");
    println!("───────────────────────────────");

    let state = terminal.get_state();
    println!("Directorio actual: {}", state.cwd);
    println!("Variables de entorno: {} definidas", state.env.len());
    println!("Comandos ejecutados: {}", state.history.len());
    println!("Último exit code: {}", state.last_exit_code);

    println!("\n✨ Demo completado exitosamente!");

    Ok(())
}
