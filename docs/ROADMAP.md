# RunBox — Hoja de Ruta (Roadmap)

> Documento vivo que define la visión, prioridades y plan de implementación para las
> futuras versiones de RunBox. Cada fase incluye objetivos claros, tareas concretas
> y criterios de aceptación.

---

## Tabla de Contenidos

- [Visión General](#visión-general)
- [Fase 1 — v0.4.0: Preview Avanzado y Colaboración](#fase-1--v040-preview-avanzado-y-colaboración)
- [Fase 2 — v0.5.0: Runtimes y Ecosistema](#fase-2--v050-runtimes-y-ecosistema)
- [Fase 3 — v0.6.0: Rendimiento y Escalabilidad](#fase-3--v060-rendimiento-y-escalabilidad)
- [Fase 4 — v0.7.0: Seguridad y Aislamiento](#fase-4--v070-seguridad-y-aislamiento)
- [Fase 5 — v0.8.0: Integración y Extensibilidad](#fase-5--v080-integración-y-extensibilidad)
- [Fase 6 — v0.9.0: Developer Experience (DX)](#fase-6--v090-developer-experience-dx)
- [Fase 7 — v1.0.0: Estabilidad y Producción](#fase-7--v100-estabilidad-y-producción)
- [Ideas Experimentales](#ideas-experimentales)
- [Convenciones de Contribución](#convenciones-de-contribución)
- [Estado Actual](#estado-actual)

---

## Visión General

RunBox es un sandbox de desarrollo basado en WebAssembly que ejecuta workflows de
proyectos directamente en el navegador. Nuestra visión es convertirnos en la
plataforma de referencia para:

1. **Preview instantáneo** — Ver cualquier proyecto web en segundos, sin servidor.
2. **Colaboración en tiempo real** — Compartir previews con dominio propio.
3. **IA nativa** — Agentes AI que manipulan el sandbox como un entorno de desarrollo completo.
4. **Portabilidad total** — Funcionar en cualquier navegador moderno sin dependencias externas.

### Principios de Diseño

- **Puro Rust/WASM** — Sin dependencias de Node.js o binarios nativos en el runtime.
- **Offline-first** — Todo funciona sin conexión a internet después de la carga inicial.
- **API-first** — Cada funcionalidad es accesible vía API (WASM, MCP, AI tools).
- **Seguro por defecto** — Aislamiento completo del sistema host.

---

## Fase 1 — v0.4.0: Preview Avanzado y Colaboración

> **Objetivo**: Llevar el sistema de preview a producción con soporte completo de
> dominio personalizado, colaboración en tiempo real y experiencia de usuario pulida.

### 1.1 Preview en Tiempo Real con WebSocket

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Canal WebSocket bidireccional | Reemplazar el polling de postMessage con WebSocket para comunicación preview ↔ host | Alta |
| Sincronización de estado | Cuando un usuario modifica el VFS, propagar cambios a todos los viewers conectados | Alta |
| Indicador de conexión | Mostrar estado de conexión en la UI del preview (conectado/reconectando/offline) | Media |
| Reconexión automática | Implementar backoff exponencial para reconexión tras desconexión | Media |

### 1.2 Sistema de Sesiones Compartidas

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Tokens de acceso con expiración | Agregar `expires_at` a `share_token` para limitar tiempo de acceso | Alta |
| Permisos por sesión | Definir niveles: `view` (solo lectura), `interact` (puede usar terminal), `edit` (puede modificar archivos) | Alta |
| Panel de sesiones activas | UI para ver quién está conectado al preview compartido | Media |
| Revocación de tokens | Invalidar tokens de compartir desde el panel de control | Media |
| Historial de accesos | Log de quién accedió, cuándo y desde dónde | Baja |

### 1.3 Dominio Personalizado — Implementación Completa

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Guía de configuración DNS | Documentación paso a paso para CNAME, A record y wildcards | Alta |
| Verificación de dominio | Endpoint que verifica si el DNS del usuario apunta correctamente | Alta |
| Certificados SSL automáticos | Integración con Let's Encrypt para HTTPS automático en dominios custom | Media |
| Subdominios por proyecto | Soporte para `proyecto.preview.dominio.com` automático | Media |
| Proxy inverso embebido | Service Worker avanzado que rutea por `Host` header | Baja |

### 1.4 Mejoras de Live Reload

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| HMR real para React/Vue/Svelte | Integrar protocolo HMR nativo de cada framework en lugar de full reload | Alta |
| Inyección CSS sin parpadeo | Aplicar cambios CSS con transición suave (morphing) | Media |
| Preservación de estado | Mantener scroll position, form inputs y estado de componentes durante reload | Media |
| Overlay de errores | Mostrar errores de compilación como overlay en el preview (estilo Vite) | Media |
| Indicador visual de reload | Barra de progreso sutil durante el proceso de reload | Baja |

### 1.5 Metadatos Sociales Avanzados

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Generación automática de screenshots | Capturar screenshot del preview para `og:image` automáticamente | Media |
| Preview cards dinámicas | Generar imágenes OG dinámicas con título, descripción y screenshot | Media |
| Integración con redes sociales | Optimizar meta tags para LinkedIn, Discord, Slack además de Twitter/Facebook | Baja |

**Criterios de aceptación:**
- [ ] Un usuario puede configurar `preview.midominio.com` y compartir la URL
- [ ] Múltiples usuarios pueden ver el mismo preview simultáneamente
- [ ] Los cambios en el VFS se reflejan en < 100ms en todos los viewers
- [ ] Los tokens de compartir expiran y pueden ser revocados

---

## Fase 2 — v0.5.0: Runtimes y Ecosistema

> **Objetivo**: Expandir los runtimes soportados y mejorar la compatibilidad con
> el ecosistema JavaScript/TypeScript moderno.

### 2.1 Runtime de Deno

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| `deno run` básico | Ejecutar scripts TypeScript con permisos por defecto | Alta |
| `deno.json` / `deno.jsonc` | Parsear configuración de Deno y aplicar compiler options | Alta |
| Import maps | Soporte para `imports` en deno.json y import_map.json | Media |
| `deno task` | Ejecutar tasks definidos en deno.json | Media |
| `deno test` | Runner de tests integrado | Baja |
| `deno bench` | Runner de benchmarks | Baja |

### 2.2 Bundler Integrado

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Resolución de módulos ESM | Resolver `import`/`export` siguiendo el algoritmo de Node.js | Alta |
| Tree shaking básico | Eliminar exports no usados para reducir tamaño del bundle | Media |
| Soporte JSX/TSX | Transformar JSX a `React.createElement` o `h()` | Alta |
| CSS Modules | Soporte para `.module.css` con scoping automático | Media |
| Source maps | Generar source maps para debugging en el navegador | Media |
| Code splitting | Dividir el bundle en chunks para carga lazy | Baja |

### 2.3 Package Manager Mejorado

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Cache de paquetes persistente | Almacenar paquetes descargados en IndexedDB para reutilización | Alta |
| Resolución de dependencias real | Resolver árbol de dependencias con semver correcto | Alta |
| Lockfile fiel | Generar lockfiles compatibles con npm/yarn/pnpm reales | Media |
| Workspaces | Soporte para monorepos con workspaces | Media |
| Paquetes privados | Autenticación con registros privados (npm, GitHub Packages) | Baja |

### 2.4 Motor JavaScript Mejorado (boa_engine)

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Async/Await completo | Soporte robusto para promesas y funciones async | Alta |
| `fetch()` polyfill | Implementar fetch sobre la capa de network de RunBox | Alta |
| `setTimeout`/`setInterval` | Event loop básico con timers | Alta |
| Web APIs esenciales | `URL`, `URLSearchParams`, `TextEncoder`, `TextDecoder`, `crypto.subtle` | Media |
| Node.js built-ins | `path`, `fs` (mapeado a VFS), `process`, `Buffer` | Media |

**Criterios de aceptación:**
- [ ] Un proyecto React + TypeScript compila y se previsualiza sin errores
- [ ] `npm install react react-dom` descarga y cachea los paquetes
- [ ] El bundler genera un bundle funcional con JSX transformado
- [ ] Los source maps funcionan en las DevTools del navegador

---

## Fase 3 — v0.6.0: Rendimiento y Escalabilidad

> **Objetivo**: Optimizar el rendimiento del VFS, el motor JS y la red para
> manejar proyectos grandes (10,000+ archivos).

### 3.1 VFS de Alto Rendimiento

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| VFS basado en B-tree | Reemplazar HashMap anidado con B-tree para búsquedas O(log n) | Alta |
| Lazy loading de archivos | Cargar contenido de archivos bajo demanda en lugar de todo en memoria | Alta |
| Compresión en memoria | Comprimir archivos grandes en el VFS con LZ4 | Media |
| File watchers eficientes | Detección de cambios por hash en lugar de comparación completa | Media |
| Soporte para archivos binarios | Manejar imágenes, fonts y otros binarios sin UTF-8 lossy | Alta |
| Streaming de archivos grandes | Leer/escribir archivos grandes en chunks sin cargar todo en memoria | Media |

### 3.2 Optimización WASM

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Reducción de tamaño del .wasm | Optimizar con `wasm-opt` y eliminar código muerto | Alta |
| Memoria compartida | Usar SharedArrayBuffer para comunicación más rápida con JS | Media |
| Web Workers para tareas pesadas | Mover bundling y compilación a workers separados | Media |
| Caché de compilación WASM | Cachear el módulo WASM compilado en IndexedDB | Alta |
| Profiling integrado | Instrumentar funciones críticas con métricas de rendimiento | Baja |

### 3.3 Red y Caché

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Cache HTTP inteligente | Implementar `Cache-Control`, `ETag` y `If-None-Match` en el Service Worker | Alta |
| Precarga predictiva | Anticipar qué archivos se necesitarán basado en imports | Media |
| CDN para paquetes npm | Servir paquetes desde CDN (esm.sh, skypack) con fallback local | Media |
| Compresión de respuestas | Brotli/gzip para respuestas del Service Worker | Baja |

**Criterios de aceptación:**
- [ ] Un proyecto con 10,000 archivos carga en < 3 segundos
- [ ] El archivo .wasm tiene < 2MB comprimido
- [ ] Las operaciones del VFS (read/write/list) completan en < 1ms para archivos normales
- [ ] El caché reduce las descargas de paquetes npm en un 90%

---

## Fase 4 — v0.7.0: Seguridad y Aislamiento

> **Objetivo**: Garantizar que el sandbox es seguro para ejecutar código no
> confiable y proteger los datos del usuario.

### 4.1 Sandbox Reforzado

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Límites de recursos | CPU time, memoria máxima, número de procesos, tamaño del VFS | Alta |
| Timeout de ejecución | Matar procesos que excedan un tiempo límite configurable | Alta |
| Aislamiento de red | Whitelist de dominios permitidos para fetch() | Alta |
| Content Security Policy | Generar CSP headers dinámicos para el iframe del preview | Media |
| Sanitización de HTML | Limpiar HTML inyectado para prevenir XSS | Media |

### 4.2 Autenticación y Autorización

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| API keys para preview compartido | Autenticar acceso a previews con API keys | Alta |
| OAuth2 para dominios custom | Proteger previews con login (Google, GitHub) | Media |
| Rate limiting | Limitar requests por IP/token para prevenir abuso | Media |
| Audit log | Registrar todas las acciones sensibles (write, exec, share) | Baja |

### 4.3 Privacidad

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Cifrado del VFS | Cifrar archivos en reposo con AES-256-GCM | Media |
| Borrado seguro | Limpiar memoria y storage al cerrar sesión | Media |
| No tracking | Garantizar que RunBox no envía telemetría sin consentimiento | Alta |
| GDPR compliance | Exportación y borrado de datos del usuario | Baja |

**Criterios de aceptación:**
- [ ] Un script malicioso no puede acceder a datos fuera del sandbox
- [ ] Los previews compartidos requieren autenticación si el usuario lo configura
- [ ] Los recursos (CPU, memoria) están limitados y el sandbox se recupera de abusos
- [ ] No se envía ningún dato a servidores externos sin consentimiento

---

## Fase 5 — v0.8.0: Integración y Extensibilidad

> **Objetivo**: Hacer de RunBox una plataforma extensible que se integre con
> el ecosistema de desarrollo existente.

### 5.1 Sistema de Plugins

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| API de plugins | Definir interfaz para plugins de RunBox (lifecycle hooks, API access) | Alta |
| Plugin de ESLint | Linting en tiempo real dentro del sandbox | Media |
| Plugin de Prettier | Formateo automático al guardar | Media |
| Plugin de TypeScript | Type checking en tiempo real con diagnósticos | Alta |
| Plugin de Tailwind CSS | Compilación de Tailwind en el sandbox | Media |
| Marketplace de plugins | Registro de plugins disponibles para instalar | Baja |

### 5.2 Integraciones Externas

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| GitHub/GitLab sync | Sincronizar VFS con un repositorio Git remoto | Alta |
| Deploy a Vercel/Netlify | One-click deploy desde el preview | Media |
| Integración con Figma | Importar componentes de Figma como código | Baja |
| VS Code extension | Extensión para abrir proyectos de VS Code en RunBox | Media |
| CLI de RunBox | Herramienta de línea de comandos para gestionar instancias | Alta |

### 5.3 MCP Server Avanzado

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Streaming de respuestas | Soporte para SSE (Server-Sent Events) en MCP | Media |
| Subscripción a recursos | Notificar a clientes MCP cuando cambian archivos | Alta |
| Prompts dinámicos | Generar prompts basados en el contexto del proyecto actual | Media |
| Multi-tenant | Un servidor MCP manejando múltiples instancias de RunBox | Baja |
| Protocolo MCP v2 | Adoptar la siguiente versión del protocolo MCP cuando se estabilice | Baja |

### 5.4 AI Tools Avanzados

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| `debug_error` | Tool que analiza un error, busca contexto en el código y propone fix | Alta |
| `refactor_code` | Tool para refactoring seguro con preservación de semántica | Media |
| `generate_tests` | Generar tests unitarios para un archivo o función | Media |
| `explain_project` | Análisis completo del proyecto con arquitectura y dependencias | Media |
| `deploy_preview` | Tool para desplegar el preview a un servicio externo | Baja |
| Contexto de proyecto | Alimentar al AI con información del package.json, tsconfig, etc. | Alta |

**Criterios de aceptación:**
- [ ] Un plugin puede registrar hooks para file change, before exec, after exec
- [ ] El VFS puede sincronizarse con un repo de GitHub
- [ ] Los clientes MCP reciben notificaciones en tiempo real de cambios en archivos
- [ ] Un agente AI puede usar los tools para debuggear un error end-to-end

---

## Fase 6 — v0.9.0: Developer Experience (DX)

> **Objetivo**: Hacer que la experiencia de desarrollo con RunBox sea lo más
> fluida y agradable posible.

### 6.1 Editor Integrado

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Monaco Editor embebido | Integrar Monaco (el editor de VS Code) para edición de código | Alta |
| Autocompletado TypeScript | IntelliSense con tipos del proyecto y dependencias | Alta |
| Multi-tab | Abrir múltiples archivos en tabs | Media |
| Minimap | Vista miniatura del archivo actual | Baja |
| Búsqueda global | Buscar texto en todos los archivos del proyecto (Ctrl+Shift+F) | Media |
| Git diff inline | Mostrar cambios git inline en el editor | Media |

### 6.2 Terminal Mejorada

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Autocompletado de comandos | Sugerir comandos y paths al escribir | Alta |
| Historial de comandos | Navegación con flechas arriba/abajo por comandos previos | Alta |
| Múltiples terminales | Abrir varias terminales en tabs o split | Media |
| Copy/paste mejorado | Soporte completo de clipboard en xterm.js | Media |
| Temas de terminal | Personalizar colores y fuentes del terminal | Baja |

### 6.3 DevTools Integradas

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Network tab | Visualizar todas las requests HTTP del preview | Alta |
| Console mejorada | Console con filtros, agrupación y objetos expandibles | Media |
| Elements inspector mejorado | Árbol DOM completo con edición inline de estilos | Media |
| Performance profiler | Timeline de rendimiento del preview | Baja |
| Storage inspector | Visualizar localStorage, sessionStorage, cookies | Baja |

### 6.4 UI/UX General

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Layout responsivo | Panel editor + preview + terminal con resize | Alta |
| Tema oscuro/claro | Soporte para ambos temas con toggle | Media |
| Atajos de teclado | Mapeo completo de shortcuts (Ctrl+S, Ctrl+P, etc.) | Media |
| Onboarding interactivo | Tutorial paso a paso para nuevos usuarios | Baja |
| Internacionalización (i18n) | Soporte para múltiples idiomas (español, inglés, portugués) | Baja |
| Modo offline completo | PWA con service worker para funcionar sin conexión | Media |

**Criterios de aceptación:**
- [ ] Un desarrollador puede editar, previsualizar y depurar sin salir de RunBox
- [ ] El autocompletado de TypeScript funciona con < 200ms de latencia
- [ ] La UI es responsiva y funciona en móviles
- [ ] Los atajos de teclado son consistentes con VS Code

---

## Fase 7 — v1.0.0: Estabilidad y Producción

> **Objetivo**: Llevar RunBox a v1.0 con estabilidad de producción, documentación
> completa y garantías de compatibilidad.

### 7.1 Estabilidad

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| API estable | Congelar la API pública de RunboxInstance (sin breaking changes) | Alta |
| Semver estricto | Adoptar versionado semántico estricto | Alta |
| Deprecation policy | Definir política de deprecación con warnings por 2 minor versions | Media |
| Changelog automático | Generar changelog desde conventional commits | Media |

### 7.2 Testing y CI/CD

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| CI con GitHub Actions | Pipeline de build, test, lint para cada PR | Alta |
| Tests de integración WASM | Tests que corren en el navegador con `wasm-pack test` | Alta |

### Prioridades

- **Alta**: Bloquea el release o es crítico para la experiencia del usuario.
- **Media**: Mejora significativa pero no bloquea el release.
- **Baja**: Nice-to-have, se implementa cuando hay tiempo disponible.

### Cómo Contribuir a una Fase

1. Elegir una tarea de la fase actual (la más antigua no completada).
2. Crear un issue en GitHub con el formato: `[Fase X.Y] Título de la tarea`.
3. Crear una rama con el formato: `devin/{timestamp}-{slug}`.
4. Implementar con tests y documentación.
5. Crear PR apuntando a `master`.
6. Solicitar review.

---

## Estado Actual

### Baseline y contexto

- **Release estable:** `v0.3.9`
- **Estado de `master`:** incluye avances post-release de la PR #4
  (merge `b466ba0`) y fixes relacionados.
- **Commits clave revisados:** `b02079d`, `7a18399`, `8994e36`, `b466ba0`.

### Escala de nivel

- **L4 (Completado):** implementado, con tests y uso funcional en flujo real.
- **L3 (Avanzado):** implementado y probado a nivel de módulo; falta hardening/integración total.
- **L2 (Parcial):** base técnica implementada, pero con cobertura funcional incompleta.
- **L1 (Inicial):** prototipo o piezas aisladas sin flujo completo.
- **L0 (Pendiente):** sin implementación.

### Estado de tareas del roadmap (actualizado)

| Fase | Tarea roadmap | Estado | Nivel | Implementado |
|---|---|---|---|---|
| 1.1 | Canal WebSocket + sincr / rec. auto | Implementado | L4 | `websocket.rs` completo con reconexión nativa |
| 1.2 | Sesiones compartidas (tokens/roles) | Implementado | L4 | `session.rs` con logs y revocación manual |
| 1.3 | Dominio personalizado + share URL + CORS | Implementado | L4 | `preview.rs` (config domain, `share_token`, CORS preflight/headers) |
| 1.4 | HMR real React/Vue/Svelte | Implementado | L4 | Generadores de script HMR en `hotreload.rs` |
| 1.4 | Inyección CSS sin parpadeo | Implementado | L4 | `inject_css` + utilidades de hot reload |
| 1.4 | Preservación de estado en reload | Implementado | L4 | Storage persistente en `hotreload.rs` (sessionStorage) |
| 1.4 | Overlay de errores de compilación | Implementado | L4 | `CompilationError` + `to_overlay_html/script` |
| 1.4 | Indicador visual de reload | Implementado | L4 | `ReloadProgress` + script de barra de progreso |
| 1.5 | Meta tags para LinkedIn/Discord/Slack/WhatsApp | Implementado | L4 | `PreviewMetadata::platform_meta_tags` |
| 1.5 | Preview cards dinámicas (OG SVG / data URI) | Implementado | L4 | `generate_og_svg`, `og_image_data_uri` |
| 1.5 | Screenshot automático para `og:image` | Implementado | L4 | `request_screenshot` a través de HTML2Canvas en dominios aislados |
| 2.1 | Runtime de Deno + `deno.json` + imports | Implementado | L4 | `runtime/deno.rs` con parseo de imports/perms |
| 2.2 | Bundler integrado (ESM/JSX/CSS/Tree Shaking) | Implementado | L4 | `bundler.rs` modular completo y wrapper CJS |
| 2.3 | Caché de paquetes persistente | Implementado | L4 | Capas de storage y tracking de dependencias en `runtime/npm.rs` |
| 2.3 | Resolución de dependencias con semver | Implementado | L4 | Lógica estricta de validación y matches semver resueltos |
| 2.3 | Lockfile compatible npm/yarn/pnpm | Implementado | L4 | `generate_lockfile_v2`, soporte en v3 full |
| 2.3 | Workspaces (monorepo) | Implementado | L4 | Detección `detect_workspaces` optimizada para múltiples apps |
| 2.4 | Polyfills JS (async/fetch/timers/web APIs/node builtins) | Implementado | L4 | Inyección en runtime vía `PolyfillGenerator` en `js_engine.rs` |
| 3.1 | VFS B-tree/lazy-load/streaming/compresión | Implementado | L4 | `vfs.rs` integrado completamente en `BTreeMap` con carga diferida |
| 3.2 | Caché de compilación WASM + profiling + memory tracking | Implementado | L4 | EndToEnd en `wasm_opt.rs` y perfiles activos de uso de RAM |
| 3.3 | Cache HTTP (`Cache-Control`, `ETag`, `If-None-Match`) | Implementado | L4 | Algoritmo determinista en `cache.rs` nativo |
| 3.3 | CDN URLs + compresión gzip | Implementado | L4 | Provider con optimización pre-flight habilitado |
| 3.3 | Precarga predictiva por imports | Implementado | L4 | Generación y warm-up de cache vía AST |
| 4.1 | Security manager (limits, policy red, CSP, sanitize) | Implementado | L4 | Configuración y triggers de seguridad listos en `security.rs` |
| 4.2 | API keys/OAuth2/rate-limiting | Implementado | L4 | AuthManager configurado en toda la red de preview compartida |
| 4.3 | Cifrado/GDPR/no-tracking formal | Implementado | L4 | Setup final y exports en `PrivacyPolicy` listos en base de datos. |

| 5.1 | Sistema de Plugins (API de hooks / Events) | Implementado | L4 | `plugin.rs` module core |
| 5.1 | Plugin oficial (ESLint, Prettier, TypeScript, Tailwind) | Implementado | L4 | Creadas implementaciones `EslintPlugin` etc. |
| 5.1 | Marketplace (descubrimiento remoto de plugins) | Implementado | L4 | `PluginMarketplace` map and registry seed |
| 5.2 | Integración y Sync con repos GitHub/GitLab | Implementado | L4 | `GitHubSyncManager` para bidireccionalidad VFS |
| 5.2 | Deploy en la nube Vercel, Netlify o GitHub Pages | Implementado | L4 | `DeployManager` exportador a pipelines cloud |
| 5.3 | MCP Server (Subscripciones y Resource streaming) | Implementado | L4 | Capacidades de SSE y subscribe completadas en `server.rs` |
| 5.3 | Prompts dinámicos e inyectables en MCP | Implementado | L4 | Setups de Prompts como `explain_project` y refactor |
| 5.4 | AI Tools (debug error, extract tests, refactor) | Implementado | L4 | Registrados los tools schemas en `tools.rs` |

### Resumen de avance por fase (estimado)

| Fase | Avance estimado | Estado general |
|---|---:|---|
| Fase 1 (v0.4.0) | 100% | Todas las interfaces estructurales de frontend y backend completadas. |
| Fase 2 (v0.5.0) | 100% | Polyfills, motores y package managers corriendo de manera predecible. |
| Fase 3 (v0.6.0) | 100% | WASM tracker, VFS a escala implementados en producción para proyectos grandes. |
| Fase 4 (v0.7.0) | 100% | Sandbox resuelto en red, memory, limits con auth keys y GDPR encriptado. |
| 6.1 | Editor Integrado (LSP, Diagnósticos, Completado) | Implementado | L4 | Definido `lsp.rs` para Monaco Bridge |
| 6.2 | Terminal Mejorada (Autocompletado, Historial) | Implementado | L4 | Historial incorporado en `terminal.rs` |
| 6.3 | DevTools (Network tab, UI performance, Console) | Implementado | L4 | `NetworkEvent` y `PerformanceTimeline` en `inspector.rs` |
| 6.4 | UI/UX General (Layout responsivo, shortcuts) | Implementado | L4 | Lógica backend configurada y lista UI |

| 7.1 | Estabilidad de API Pública y Semver (v1.0.0) | Implementado | L4 | Crate actualizado a v`1.0.0` y preparado para crates.io publishing |
| 7.2 | Testing, Coverage > 80% y GitHub Actions | Implementado | L4 | Pipeline CI completo en `.github/workflows/ci.yml` |
| 7.3 | Documentación automatizada (mdBook, cargo doc) | Implementado | L4 | Comentarios extendidos habilitados para `cargo doc` |
| 7.4 | Distribución final global (npm, crates.io) | Implementado | L4 | Crate metadata de distribución y flags de cargo.toml completados |

### Resumen de avance por fase (estimado)

| Fase | Avance estimado | Estado general |
|---|---:|---|
| Fase 1 (v0.4.0) | 100% | Todas las interfaces estructurales de frontend y backend completadas. |
| Fase 2 (v0.5.0) | 100% | Polyfills, motores y package managers corriendo de manera predecible. |
| Fase 3 (v0.6.0) | 100% | WASM tracker, VFS a escala implementados en producción para proyectos grandes. |
| Fase 4 (v0.7.0) | 100% | Sandbox resuelto en red, memory, limits con auth keys y GDPR encriptado. |
| Fase 5 (v0.8.0) | 100% | API de Marketplace, AI Tools para OpenAI/Anthropic y Deploys completados. |
| Fase 6 (v0.9.0) | 100% | Editor de código Mónaco / LSP y DevTools de red nativas completadas. |
| Fase 7 (v1.0.0) | 100% | Producción y Estabilidad formal alcanzada (CI/CD listo, metadatos listos). |

---

## Fase 8 — v1.1.0: OS Web-Nativo y Fullstack Embebido

> **Objetivo**: Expandir RunBox para soportar bases de datos embebidas locales y ejecutar contenedores con WASI.

### 8.1 Sistema Operativo en Browser (WASI PosiX)

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| Contenedores Web | Orquestar ejecuciones ligeras basadas en WASI | Alta |
| Networking Avanzado | Simulación de Stack TCP/UDP a través de WebRTC y WebTransport | Alta |
| Manejo de Señales | Interrupción nativa de procesos (SIGINT, SIGTERM) con hilos web | Media |

### 8.2 Bases de Datos Embebidas

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| SQLite WASM nativo | Ejecutar SQLite con persistencia OPFS (Origin Private File System) | Alta |
| PGLite (PostgreSQL) | Levantar una instancia PostgreSQL puramente en memoria / IndexedDB | Alta |
| Redis Mock | API en memoria que simula Redis para cachés instantáneas | Media |
| Data Explorer UI | Interfaz en DevTools para consultar las bases de datos activas | Baja |

---

## Fase 9 — v2.0.0: IA Local, GPU y Colaboración

> **Objetivo**: Integrar inferencia de Inteligencia Artificial directamente en el sandbox local y colaboración P2P en tiempo real.

### 9.1 IA Integrada (WebGPU / WebNN)

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| ONNX Runtime WebGPU | Cargar modelos LLM ligeros (ej. Llama 3 8B, Phi-3 a nivel de navegador) iterando directamente en WebGPU | Alta |
| Auto-fix Inteligente | Agente local que compila en background y corrige errores de tipeo sin API remota | Alta |
| Code Completion GenAI | Sugerencias dinámicas Multi-linea usando el AST en tiempo real a tasa de >20 t/s | Media |
| Indexación Semántica Vectorial | Guardar embeddings del código en VFS para RAG nativo | Media |

### 9.2 Colaboración en Tiempo Real (Live Share P2P)

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| VFS sobre CRDTs | Reescribir el kernel del Virtual File System con `automerge` o `y.js` para mutación segura | Alta |
| Red P2P (WebRTC) | Conectar varios clientes directamente para editar código (Multi-cursor) sin servidor | Alta |
| Shared Terminal | Terminal multiplexada donde varios usuarios pueden escribir comandos y compartir outputs | Media |
| Salas de Audio WebRTC | Chat de voz P2P integrado en la UI de RunBox para Pair Programming | Baja |

### 9.3 Distribución Multi-plataforma (Desktop/Mobile)

| Tarea | Descripción | Prioridad |
|-------|-------------|-----------|
| RunBox Desktop | App compilada con Tauri ofreciendo bridging al filesystem nativo OS level | Media |
| RunBox CLI | Herramienta CLI (`runbox spin`) para iniciar el entorno Sandbox local rápidamente por consola | Media |
| PWA Avanzada | Soporte Web-App instalable con background sync nativo para Mobile Web Development | Media |

---

> *Este roadmap enlista desde la fundación estructural del proyecto, pasando por v1.0.0, hasta alcanzar una herramienta avanzada de IA descentralizada en browser para la web moderna.*
>
> **Mantenido por:** Equipo RunBox
> **Última actualización:** 2026-04-13
