# AGENTS.md — MC Server Launcher (Tauri)

## Quién manda acá
- **Alex orquesta, opencode/coding agents codean.** Nunca debería un agente
  tomar decisiones de producto o arquitectura por su cuenta.
- El **SPEC.md es la fuente de verdad.** Si el código y el spec divergen,
  el agente corrige para cumplir el spec o lo comenta ANTES de irse por otro lado.
- No hay nada intocable (esto NO es un repo con pipeline de releases como PS) —
  pero NO se commitea código roto ni WIP sin avisar.

## Stack
- Tauri v2 (Rust backend + webview nativo). Frontend: **vanilla TypeScript + CSS**
  DECIDIDO (compila a JS vanilla, mismo runtime; tipado leve caza errores de
  contratos en compile-time). Sin framework runtime pesado (React/Vue/Svelte NO
  salvo complejidad justificada y aviso previo).
- Alcanza con lo que trae `npm create tauri-app` — no inventar tooling extra.
- Scaffold: vanilla TS, productName `Digspawn`, identifier `com.digspawn.app`,
  binario `digspawn`.

## Arquitectura: el backend no se contamina con la UI (REGLA DURA)
- **La lógica de negocio NO vive en la interfaz.** El frontend solo muestra y
  delega: pedir datos al backend, pintar, devolver la acción del usuario. Nada de
  decisiones de dominio (qué Java hace falta, qué versiones son válidas, cómo se
  arma un comando de arranque) resueltas en TypeScript. Eso vive en Rust.
- **Todo el acoplamiento a Tauri en el frontend pasa por `src/api.ts`.** Ningún
  otro archivo importa de `@tauri-apps/*`: nada de `invoke`, `listen`, `open`,
  `openPath`, `openUrl`, `getCurrentWebview` sueltos por las vistas. Se envuelven
  acá (`api.onLogLine(cb)`, `api.elegirArchivo()`, `api.abrirEnNavegador(url)`) y
  las vistas consumen la API tipada.
- **Por qué:** si la UI queda desacoplada del runtime, cambiar de stack de interfaz
  (ej. Tauri → Slint) es reescribir `api.ts` + el markup, y los módulos de lógica
  (`java.rs`, `modrinth.rs`, `paper_api.rs`, `mojang_api.rs`, `properties.rs`,
  `errors.rs` — hoy con CERO dependencia de Tauri) se reutilizan intactos. Si la
  lógica se hardcodea en la vista, la migración pasa de horas a reescritura
  completa. **La regla no cuesta nada hoy y es lo que mantiene esa puerta abierta.**
- Estado actual a respetar: los 51 `invoke` ya están todos en `api.ts`. Los
  imports de plugins aún se reparten en las vistas (deuda a saldar cuando se toque
  cada archivo, sin apuro y sin romper nada).

## Reglas de trabajo
1. Leer SPEC.md completo antes de tocar código.
2. Arrancar por el esqueleto: `npm create tauri-app` → app mínima que corre,
   y que compila en Arch Linux (el dev machine de Alex) aunque el target sea
   Windows. Prioridad: que Alex PUEBE la UI en su Arch. Código solo-diaglo
   Windows al final.
3. Paciencia con el flux de Tauri: cambiar código Rust requiere rebuild,
   el frontend se recarga en vivo. Avisar cuando hace falta recompilar backend.
4. Commits: mensajes claros, un `git init` ya existe (user 'alex').
   Commitear por feature.
5. NO subir GitHub ni nada a remoto sin pedirlo. Todo local hasta que Alex diga.
6. Si algo del spec no funciona como está escrito (ej: Paper API migró de
   v2 a v3), investigar brevemente y usar lo que funcione, avisando el cambio
   en el commit message.

## Entorno / buildeo
- Build Rust: `cargo build`. Frontend: `npm run dev` (Tauri dev server).
- Correr en Arch: `npm run tauri dev` (o el equivalente de la plantilla).
- Para el exe Windows final: `tauri build --target x86_64-pc-windows-msvc`
  (requiere toolchain + webview2 en la máquina build; JUEGO POSTERIOR, no del arranque).

## Definiciones pendientes (decidir CON Alex, código assumir quieto)
- Nada crítico pendiente de código. Nombre **Digspawn** / binario `digspawn`,
  dataDir `%APPDATA%/digspawn/`, frontend vanilla TS, update-check meta-file GitHub
  tipo Phone Stories — todo DECIDIDO y en SPEC.md.

## Cómo comprobar progreso (playbook del agente)
- `cargo build` exitoso + `npm run tauri dev` levanta la UI → esqueleto ok.
- La biblioteca lista servers en disco (carpeta vacía → estado limpio).
- Crear server baja un jar de Paper y genera eula+props → corazón del wizard ok.
- Poder arrancar un server local (niñito) y castear log en la consola → MVP cerrado.
- Cada hito: commit con mensaje tipo "feat: skeleton compila" / "feat: wizard crea server".