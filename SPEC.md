# MC Server Launcher — Spec V1

## Qué es
Launcher de escritorio tipo Prism Launcher, pero para HOSTEAR servers de Minecraft.
Apuntado a que "el más burro" pueda levantar un server de Paper/Vanilla sin tocar
un archivo de texto ni una terminal NUNCA. Windows, portable.

**Stack:** Tauri v2 — Rust backend + UI web (webview nativo, exe ~15MB).
Frontend vanilla JS + CSS (sin framework runtime pesado → fluidez).

---

## Alcance V1
- Biblioteca de servers (cards estilo Prism).
- Crear server: Paper o Vanilla, elegís versión.
- Configurar server desde menú (specs, mundo, dificultad, whitelist, etc).
- Prender/apagar/restart con botones.
- Consola en vivo + pestañas (Jugadores / Comandos útiles / Ajustes).
- Comandos rápidos: say, op, give, time set, gamemode, whitelist add.
- Portable exe. Detección de Java.

## FUERA de V1 (anotado para V2)
- Buscador de mods (Modrinth + CurseForge) + instalación.
- Soporte Fabric/Forge.
- Backup / exportar / importar worlds.
- Multi-idioma.
- Instalador.

---

## Arquitectura

```
┌──────────────────────────────┐
│  Frontend (webview)          │
│  biblioteca · wizard · consola│
└──────────────┬───────────────┘
               │ Tauri commands (invoke) + events (logs streaming)
┌──────────────▼───────────────┐
│  Backend Rust                │
│  - ServerManager (procesos)  │
│  - PaperAPIClient / MojangAP │
│  - PropertiesParser          │
│  - JavaDetector              │
└──────────────────────────────┘
```

### Datos en disco
```
<dataDir>/servers/
    <ServerName>/
        server.jar
        server.properties
        eula.txt
        world/...
        icon.png          (imagen custom de la card)
```

- **dataDir Windows:** `%APPDATA%/mc-launcher/` (o el directorio del exe en modo
  portable-with-files). Configurable.
- Los servers viven en una carpeta aparte de la app → fácil backup/navegar.
- Importar server existente V1: botón que apunta a una carpeta con jar → la
  copia/usa in-place.

### Estado / procesos (Rust)
- `ServerManager` guarda el server activo: `tokio::process::Command` spawn java.
- **Input de comandos:** pipe `stdin` del proceso java. El server de MC lee
  comandos de stdin — no hace falta PTY.
- **Salida:** pipe `stdout` → se taggea en líneas y se emite como evento Tauri
  (`log-line`). El frontend appendea al log.
- **Stop graceful:** mandar `stop` por stdin, esperar exit con timeout
  (~20s), si cuelga → `kill`.
- Restart = stop + start.
- Evento `server-state` (`stopped | starting | running | stopping | crashed`)
  sincroniza la UI.

---

## UI / Pantallas

### 1. Biblioteca (inicio)
- Grid de cards de servers. Cada card:
  - `icon.png` custom (o icono default según tipo).
  - Nombre + badge `Paper 1.21.1` / `Vanilla 1.20.4`.
  - Indicador de estado (corriendo/parado).
- **Doble click** → arranca el server y entra en modo consola.
- **Click derecho** → menú: Editar / Propiedades / Renombrar / Borrar / Abrir carpeta.
- **"Nuevo server"** → wizard.

### 2. Wizard "Nuevo server"
1. Tipo: Paper (recomendado) | Vanilla.
2. Versión: dropdown desde API de Paper/Mojang (última por default).
3. Nombre.
4. RAM (MB) con default sensato (2048).
→ Crea carpeta, baja el jar, firma `eula=true`, genera `server.properties` default,
vuelve a la biblioteca con la card lista. (Primer arranque del server igual, si
pide confirmar eula el launcher lo muestra una vez.)

### 3. Modo servidor (ventana transformada)
- Header: nombre + estado + RAM/CPU en vivo + botones Start/Stop/Restart.
- **Izquierda: pestañas**
  - **Consola:** log en vivo + input de comandos abajo (Enter envía).
  - **Jugadores:** lista parseada del log (join/leave regex). Nombre + en qué
    mundo/jugando hace cuánto si el parse lo permite (V1: simple lista en línea).
  - **Comandos útiles:** botones con inputs mínimos:
    - `say` → un campo de texto.
    - `op` → `<jugador/selector>`.
    - `give` → `<jugador> <item> [cant]`.
    - `time set` → day / night / noon / <tick>.
    - `gamemode` → survival/creative/adventure/spectator + selector.
    - `whitelist add` → `<jugador>`.
    - Cada botón arma el comando y lo mete al pipe (y al log como eco).
  - **Ajustes:** editor por secciones de `server.properties` (lectura/escritura
    con el server parado; avisa si está corriendo que se aplica en el próximo restart).
- Cerrar la ventana con server corriendo → **minimiza a tray** (no lo mata por
  accidente) o confirma "¿cerrar el server?".

---

## Backend — comandos Tauri (Rust)

| Comando | Función |
|---|---|
| `list_servers` | Estado de todas las cards. |
| `create_server(type, version, name, ram)` | Baja jar + setup inicial. |
| `delete_server(name)` | Borra carpeta (con confirm). |
| `start_server(name, ram)` | Spawnea java, entra modo consola. |
| `stop_server(name)` | Command `stop` + graceful. |
| `restart_server(name)` | Stop+start. |
| `send_command(name, cmd)` | Escribe a stdin del proceso. |
| `get_properties(name)` / `set_properties(name, kvs)` | Lee/escribe props. |
| `list_versions(type)` | Versiones desde papermc/mojang API. |
| `detect_java()` | Encuentra java instalado. |
| `read_log(name)` | Últimas N líneas al abrir consola (history). |

### API de descarga
- **Paper:** `https://api.papermc.io/v2/projects/paper` → versions/builds.
  (Nota: Paper migró a API v3 — `https://api.papermc.io/v3` — VERIFICAR cuál
  usar al implementar; v2 sigue activa pero deprecándose.)
- **Vanilla:** manifest Mojang `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`
  → sacar url del `server.jar`.

### Detección de Java + auto-instalación
- **Estrategia V1: auto-instalar el JDK portable, sin tocar el sistema.**
- Adoptium tiene API pública y expone un ZIP portable (cero instalador, cero
  admin, cero PATH):
  `https://api.adoptium.net/v3/binary/latest/21/ga/windows/x64/jdk/hotspot/normal/eclipse`
  → descarga ZIP de Temurin JDK 21 → extraer dentro de `<dataDir>/runtime/jdk21/`
  → usar ese `java.exe` local. Autónomo para el amigo.
- Flujo: al primer arranque, `detect_java()`:
  1. Si hay un `runtime/jdk21` propio → usarlo.
  2. Sino, buscar `JAVA_HOME` / `java -version` en PATH / Program Files.
  3. Si no hay ninguno → pantalla "Te falto Java, lo bajo yo" con progreso de
     descarga → baja+extrae → listo. (Fallback: link a Adoptium por si la API falla.)
- Tener en cuenta: un único runtime compartido por todos los servers.
- Guardar "usar runtime propio sí/no" como setting global configurable.
- **Nota:** JDK (no solo JRE) porque algunos servers/scripts piden javac; tamaño
  extra es aceptable. Si quieren minimizar se baja JRE (`image_type=jre`).

### Arranque del server — flags por default
- **Todos los servers arrancan con JDK 21 + "Aikar's flags" (G1GC bonito) por
  default** — el launcher las mete en el command de arranque solo, el amigo
  nunca las ve. Esas flags las escribió el dev de Paper y son el tune estándar
  para MC server:
  `java -Xms{ram}M -Xmx{ram}M -XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200 -XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC -XX:+AlwaysPreTouch -XX:G1NewSizePercent=30 -XX:G1MaxNewSizePercent=40 -XX:G1HeapRegionSize=8M -XX:G1ReservePercent=20 -XX:G1HeapWastePercent=5 -XX:G1MixedGCCountTarget=4 -XX:InitiatingHeapOccupancyPercent=15 -XX:G1MixedGCLiveThresholdPercent=90 -XX:G1RSetUpdatingPauseTimePercent=5 -XX:SurvivorRatio=32 -XX:+PerfDisableSharedMem -XX:MaxTenuringThreshold=1 -jar server.jar --nogui`
- **Para server chico (amigo, 4-6 jug): `-Xms/-Xmx 2G` es el tamaño carne.** ZGC
  solo si se deseara un server grande (60+ jug / modding pesado) — feature
  "modo avanzado", no default.
- GraalVM NO — G1 es el único GC ahí y el net sobre Temurin es marginal para
  MC. JDK 21 Temurin/Zulu + Aikar's flags es el estándar probado.

### server.properties defaults (al crear)
```
online-mode=true, difficulty=normal, gamemode=survival, pvp=true,
max-players=10, motd=Nuestro server!, view-distance=10
```
Restos: valores vanilla default.

---

## Bundling
- `tauri build` → exe + (opcional) NSIS installer.
- **Portable primero:** un solo .exe que usa `%APPDATA%` como dataDir (o
  portable-with-files si el exe está en carpeta editable). Decisión de empaque
  al final, pero portable = ejes simple.

---

## Riesgos / trampas
- Paper API v2↔v3 churn: verificar endpoint activo al implementar.
- Java ausente es el caso #1 de "no me funciona" → juicio temprano y claro.
- `.exe` firmado/smart-screen: primera vez Windows muestra "Microsoft Defender
  SmartScreen, publisher desconocido" → instrucción de "More info → Run anyway"
  incluida en el README para el amigo.
- RAM: cap no dejar tocar más de lo físico / guard rail de 512MB mínimo.
- El servidor "crashea" sin log claro si falta Java o el jar se bajó mal →
  buen manejo del estado `crashed` con el motivo.

## Definir al arrancar la V1
- ¿Framework frontend? Sugerencia: vanilla JS + CSS (cero runtime, fluido).
  Si el wizard/estado se vuelve complejo, Svelte compilado es el upgrade natural.
- Nombre de la app / repo.
- DataDir fijo (APPDATA) vs. portable-with-files junto al exe.