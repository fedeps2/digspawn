TE ARRANCO EL LAUNCHER DE SERVERS DE MINECRAFT — Tauri v2, Windows, pa' que "el mas burro" lo use.

## CONTEXTO
Esto es un launcher tipo Prism pero para HOSTEAR servers de Minecraft (Paper/Vanilla),
apuntado a un usuario no-técnico que solo quiere jugar con los compañeros del laburo.
MVP primero, facha después. Orquestación: Alex decide producto/arquitectura, vos codeas.
NO tomes decisiones de diseño por tu cuenta.

## FUENTES DE VERDAD (leelas ANTES de tocar codigo)
- SPEC.md — el spec completo. Es TU Biblia. Si algo del codigo y el spec divergen,
  cambias el codigo para cumplir el spec.
- AGENTS.md — reglas de trabajo. Seguilas.

## QUÉ HACER EN ESTE HITO (paso 1 SOLAMENTE)
Montar el esqueleto Tauri v2 MINIMO y que corra en mi Arch (el dev machine).

1. `npm create tauri-app` decente (Tauri v2, vanilla JS/TS frontend, sin framework pesado).
2. Que `npm run tauri dev` levante la UI vacia en Arch Linux SIN errores.
3. Estructura de backend Rust lista: modulos vacios/placeholder para ServerManager,
   PaperAPIClient/MojangAPI, PropertiesParser, JavaDetector (no implementes logica aun).
4. `cargo build` exitoso + `.gitignore` para target/, node_modules/, dist/.
5. Commit con mensaje tipo "feat: skeleton tauri v2 compila y corre".

## RESTRICCIONES / LIMITES
- SOLO el esqueleto. NO implementes cargar servers, bajar jars, consola, ni nada del
  feature-set del MVP todavia. Ese es el siguiente hito. No te adelantes.
- Frontend: vanilla JS+CSS, salvo que el estado se vuelva complejo (ahi evalua Svelte
  compilado, pero avisa antes de cambiar).
- Compilación target principal: que corra en Arch. El exe Windows final se resuelve
  DESPUES. No lo persigas ahora.
- Tiramselo portable-with-files NO — dataDir fijo en APPDATA.
- Si algo del spec no aplica a un esqueleto (ej: Paper API v2 vs v3), anotalo en un
  TODO y seguí; no lo resuelvas a menos que bloquee.
- No subas a GitHub aun. Repo local.

## DEFINIDO YA (no lo reabras)
DataDir=%APPDATA%/mc-launcher/, update-check=meta-file GitHub tipo Phone Stories,
Java=autoinstall Adoptium portable + JDK21 + Aikar's flags. Todo en el spec.

## PLAYBOOK PARA CONFIRMAR QUE QUEDO BIEN
- `cargo build` exitoso.
- `npm run tauri dev` levanta la app y ves una ventana (vacia).
- `git log --oneline` muestra un commit de esqueleto.
Reporta SI corriste eso y que viste, no me digas "quedo listo" sin verificarlo.