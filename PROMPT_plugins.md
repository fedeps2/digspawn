HITO EXPANSIVO: INSTALADOR IN-APP DE PLUGINS (Modrinth) — pa' que "el mas burro" le ponga plugins al server desde la app.

## CONTEXTO
El launcher ya crea servers Paper que arrancan y corren (con consola, jugadores,
comandos). Lo que falta: que el usuario no-tecnico pueda agregar PLUGINS sin bajar
un jar a mano ni tocar una carpeta. Este hito agrega la pestana "Plugins" dentro del
Modo servidor de un server **Paper** (los servers Vanilla NO llevan plugins — no
mostrar/avisar ahi).
Orquestacion: Alex decide producto/arquitectura, vos codeas. NO tomes decisiones
de diseno por tu cuenta; si algo del spec no aplica, anota la divergencia ANTES de
irte por otro lado (regla AGENTS 6).

## FUENTES DE VERDAD (leelas ANTES de tocar codigo)
- SPEC.md — la Biblia. Modo servidor / pestanas ya definido (Consola, Jugadores,
  Comandos utiles, Ajustes) en ~seccion "3. Modo servidor". Suma una pestana nueva.
- AGENTS.md — reglas de trabajo. Seguilas.

## QUÉ HACER EN ESTE HITO
Cerrar en algo que el pana entenderia: **"eleji un plugin, se instalo, reinicie el
server y quedo andando"**. Tres piernas:

### 1. Backend (Rust, Tauri v2) — solo Modrinth API
Reusa el patron de descarga+progreso que ya existe (Adoptium/Paper fill: reqwest +
evento de progreso). Mandar User-Agent valido tipo `digspawn/<version>` (Modrinth
lo exige, mismo patron que Paper fill).

Comandos Tauri nuevos:
- `search_plugins(query, server_name)` -> lista de resultados (nombre, autor,
  desc corta, downloads, version mas nueva compatible). Filtra por:
  - `project_type:plugin` y loaders `paper`/`spigot`/`bukkit` (un server Paper los corre).
  - **game_version = la version MC exacta del server** (el filtro duro anti-rompe-server).
- `install_plugin(server_name, project_id, version_id)` -> baja el `.jar` a
  `<server>/plugins/`. Maneja **dependencias required** en cascada (ver Trampas).
- `list_plugins(server_name)` -> los `.jar` presentes en `plugins/` (instalados).
- `uninstall_plugin(server_name, archivo)` -> borra el `.jar` (con confirmar en UI).

Modrinth API (estructura, verificar endpoint al implementar — regla AGENTS 6):
- Search: `GET https://api.modrinth.com/v2/search?query=...&facets=[...]` con facets
  de project_type y loaders.
- Versions de un proyecto: `GET https://api.modrinth.com/v2/project/{id}/version`
  -> cada release trae `game_versions[]`, `loaders[]`, `files[].url` (el jar) y
  `dependencies[]` (project_id + dependency_type required/optional).
NUNCA construir URLs de descarga a mano: usar el `files[].url` de la respuesta.

### 2. Frontend (vanilla TS/CSS) — pestana "Plugins" en Modo servidor
- Solo visible para servers tipo Paper. En Vanilla: ocultar la pestana (o aviso claro).
- **Buscador**: input + lista de resultados (nombre, autor, desc, downloads) +
  boton "Instalar".
- **Progreso de descarga**: reusar el overlay/evento de progreso del Java.
- **Instalados**: lista de los `.jar` activos, con boton "Desinstalar" (confirm).
- **Aviso de reinicio**: al instalar/desinstalar dejar claro que "aplica al
  reiniciar el server" (los plugins se cargan al boot).
- Es una pestana mas del Modo servidor: mimica el look de las existentes, nada de
  redisenar la UI del server.

## TRAMPAS (no repetir)
1. **Compatibilidad loader x version MC** = el corazon del "no te rompas el server".
   Un plugin que no corre en tu version de Paper hace que el server no bootee.
   Filtrar SIEMPRE por game_version exacta + loader compatible. Al filtrar bien
   la busqueda, el riesgo desaparece.
2. **Dependencias requeridas**: muchos plugins (Vault, PlaceholderAPI, ProtocolLib…)
   exigen otro plugin. Si no las instala, el plugin no carga y parece que "no
   anduvo". Instalar las `dependency_type: required` en cascada (o al menos
   mostrarlas claras). Las `optional` NO se instalan solas, solo se mencionan.
3. **Reinicio necesario**: los plugins se cargan al arranque. Despues de instalar,
   DEBE quedar explicito que hay que reiniciar el server para que aparezca.
4. **Es PLUGINS, no MODS**: solo jars de API Bukkit/Paper para servers Paper.
   NADA de mods (Fabric/Forge/NeoForge/Quilt) en este hito — ese es otro arbol.

## RESTRICCIONES / LIMITES
- **Solo Modrinth** en este hito. NO CurseForge (API key/ToS) ni Hangar — eso se
  evalua despues si hace falta.
- No se toca el WIZARD (sigue Vanilla + Paper). Plugins = feature del server ya
  creado, no del momento de creacion.
- No configurar plugins (config.yml ni nada). Eso seria otro hito.
- Reusar el patron de descarga+progreso existente; no inventar tooling.
- Compilacion principal en Arch. No borres/rompas la vista Consola/Jugadores/Ajustes.
- No subas a GitHub sin pedir. Todo local.

## DEFINIDO YA (no lo reabras)
Tauri v2, vanilla TS, backend Rust con reqwest/tokio, dataDir fijo
`%APPDATA%/digspawn/`, overlay de progreso de descarga existente (reusar).

## PLAYBOOK PARA CONFIRMAR QUE QUEDO BIEN
Verifica con tus propias herramientas, no me digas "quedo listo" sin probarlo:
- `cargo build` + `npm run build` (tsc) exitosos.
- En un server Paper de prueba (de tu version): buscar un plugin comun (ej:
  essentials / luckperms), instalar uno compatible, **reiniciar**, y verlo en la
  lista de instalados / que el server lo carga y sigue booteando.
- Un server Vanilla NO muestra la pestana Plugins (o avisa).
- Desinstalar uno y confirmar que desaparece del listado de plugins del server.
- Los endpoints de Modrinth andan (si la API cambio de forma, anota y usa lo que
  funcione, regla AGENTS 6).
Reporta SI corriste esa verificacion y que viste.
