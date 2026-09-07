# AGENTS.md — MC Server Launcher (Tauri)

## Quién manda acá
- **Alex orquesta, opencode/coding agents codean.** Nunca debería un agente
  tomar decisiones de producto o arquitectura por su cuenta.
- El **SPEC.md es la fuente de verdad.** Si el código y el spec divergen,
  el agente corrige para cumplir el spec o lo comenta ANTES de irse por otro lado.
- No hay nada intocable (esto NO es un repo con pipeline de releases como PS) —
  pero NO se commitea código roto ni WIP sin avisar.

## Stack
- Tauri v2 (Rust backend + webview nativo). Frontend: vanilla JS + CSS (sin
  framework runtime pesado; Svelte solo si el estado se vuelve complejo).
- Alcanza con lo que trae `npm create tauri-app` — no inventar tooling extra.

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
- Nombre de la app / binario.
- Framework frontend confirmado: vanilla JS+CSS salvo contraorden.
- (DataDir YA decidido: `%APPDATA%/mc-launcher/` fijo. Update check = meta-file de
  GitHub tipo Phone Stories, Opción A. Ver SPEC.md → Bundling & Updates.)

## Cómo comprobar progreso (playbook del agente)
- `cargo build` exitoso + `npm run tauri dev` levanta la UI → esqueleto ok.
- La biblioteca lista servers en disco (carpeta vacía → estado limpio).
- Crear server baja un jar de Paper y genera eula+props → corazón del wizard ok.
- Poder arrancar un server local (niñito) y castear log en la consola → MVP cerrado.
- Cada hito: commit con mensaje tipo "feat: skeleton compila" / "feat: wizard crea server".