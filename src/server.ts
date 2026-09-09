// Vista de server (hito 4): header con estado + medidores vivos,
// pestañas Consola (log + stdin) y Ajustes (props + RAM), preflight al iniciar.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  errMsg,
  type LogLine,
  type RuntimeProgress,
  type ServerInfo,
  type ServerStateEvent,
} from "./api";
import { avatarHtml, placeholderHtml } from "./avatar";

const MAX_LINES = 2000;

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => {
    switch (c) {
      case "&": return "&amp;";
      case "<": return "&lt;";
      case ">": return "&gt;";
      case '"': return "&quot;";
      default: return "&#39;";
    }
  });
}

const DIFFICULTIES = ["peaceful", "easy", "normal", "hard"];
const GAMEMODES = ["survival", "creative", "adventure", "spectator"];
const BOOLS = ["true", "false"];

export type ServerTab = "consola" | "ajustes" | "historial" | "comandos" | "jugadores" | "plugins" | "backups";

export async function openServer(view: HTMLElement, name: string, onBack: () => void, initialTab: ServerTab = "consola"): Promise<void> {
  view.dataset.mode = "server";
  let mounted = true;
  const isActive = () => mounted && view.dataset.mode === "server";
  const servers = await api.listServers().catch(() => [] as ServerInfo[]);
  if (!isActive()) return;
  const info = servers.find((s) => s.name === name);
  if (!info) {
    view.innerHTML = `<p class="error">No existe el server "${esc(name)}".</p><button id="sv-back" type="button">Volver</button>`;
    view.querySelector("#sv-back")?.addEventListener("click", onBack);
    return;
  }

  let state: string = info.state;
  let ramMb: number = info.ram_mb;
  let busy = false;
  if (initialTab === "plugins" && info.type !== "paper") initialTab = "ajustes";
  let tab: ServerTab = initialTab;
  let props: Record<string, string> | null = null;
  let propsError: string | null = null;
  let propsMsg: string | null = null;
  let hostMaxRam = 8192;
  let localIps: string[] | null = null;
  let historyLoaded = false;
  let histFiles: import("./api").LogFile[] | null = null;
  let histError: string | null = null;
  let histSel: string | null = null;
  let histLines: string[] = [];
  let histLoading = false;
  let bkFiles: import("./api").BackupInfo[] | null = null;
  let bkError: string | null = null;
  let bkBusy = false;
  let bkMsg: string | null = null;
  let backingUp = false;
  let bkProgress: import("./api").BackupProgress | null = null;
  let bkCfg: import("./api").BackupConfig | null = null;
  let bkMode: "manual" | "auto" | "onstart" = "manual";
  let bkRetMode: "count" | "gb" | null = null;
  let bkRetModeStart: "count" | "gb" | null = null;
  const unlistens: UnlistenFn[] = [];
  let pollTimer = 0;

  view.innerHTML = `
      <button id="sv-back" type="button">← Biblioteca</button>
    <div class="sv-top">
      <div id="sv-avatar" role="button" tabindex="0" data-tip="Click para cambiar el icono (PNG de hasta 1 MB)."></div>
      <input id="sv-icon" type="file" accept="image/png,.png" hidden />
      <div class="sv-id">
        <div class="sv-name-row">
          <h2>${esc(info.name)}</h2>
          <span id="sv-state" class="badge"></span>
        </div>
        <p id="sv-usage" class="muted"></p>
      </div>
      <div class="sv-actions">
        <button id="sv-toggle" type="button">▶ Iniciar</button>
        <button id="sv-restart" type="button" data-tip="Apaga y vuelve a prender. Sirve para aplicar cambios de Ajustes.">↻ Reiniciar</button>
      </div>
    </div>
    <div id="sv-memwarn" class="warn-box" hidden></div>
    <div id="sv-crash" class="crash-box" hidden></div>
    <p id="sv-msg" class="muted"></p>
    <div id="sv-java" class="muted" hidden></div>
    <div class="sv-main">
      <div class="tabs">
        <button id="tab-consola" type="button" data-tip="Lo que el server está diciendo en vivo, y caja para mandarle comandos.">Consola</button>
        <button id="tab-jugadores" type="button" data-tip="Quién está conectado ahora (se detecta del log).">Jugadores</button>
        <button id="tab-comandos" type="button" data-tip="Atajos para los comandos más usados, sin escribirlos a mano.">Comandos</button>
        ${info.type === "paper" ? `<button id="tab-plugins" type="button" data-tip="Plugins (.jar) del server. Después va a servir también para mods.">Plugins</button>` : ""}
        <button id="tab-ajustes" type="button" data-tip="Configuración del server. Solo se edita frenado; aplica al arrancar.">Ajustes</button>
        <button id="tab-historial" type="button" data-tip="Logs guardados: el último log, rotados viejos y crashlogs.">Historial</button>
        <button id="tab-backups" type="button" data-tip="Copias del mundo y la config. Restaurar es un click, sin tocar archivos.">Backups</button>
      </div>
      <div id="sv-tab-body"></div>
    </div>
    <div id="sv-modal-root"></div>`;

  const msgEl = view.querySelector<HTMLElement>("#sv-msg")!;
  const javaEl = view.querySelector<HTMLElement>("#sv-java")!;
  const stateEl = view.querySelector<HTMLElement>("#sv-state")!;
  const usageEl = view.querySelector<HTMLElement>("#sv-usage")!;
  const memwarnEl = view.querySelector<HTMLElement>("#sv-memwarn")!;
  const crashEl = view.querySelector<HTMLElement>("#sv-crash")!;
  const avatarEl = view.querySelector<HTMLElement>("#sv-avatar")!;
  const toggleBtn = view.querySelector<HTMLButtonElement>("#sv-toggle")!;
  const restartBtn = view.querySelector<HTMLButtonElement>("#sv-restart")!;
  const modalRoot = view.querySelector<HTMLElement>("#sv-modal-root")!;
  let stopRequested = false;
  const tabBody = view.querySelector<HTMLElement>("#sv-tab-body")!;
  const tabConsola = view.querySelector<HTMLButtonElement>("#tab-consola")!;
  const tabAjustes = view.querySelector<HTMLButtonElement>("#tab-ajustes")!;
  const tabHistorial = view.querySelector<HTMLButtonElement>("#tab-historial")!;
  const tabComandos = view.querySelector<HTMLButtonElement>("#tab-comandos")!;
  const tabJugadores = view.querySelector<HTMLButtonElement>("#tab-jugadores")!;
  const tabBackups = view.querySelector<HTMLButtonElement>("#tab-backups")!;
  const tabPlugins = view.querySelector<HTMLButtonElement>("#tab-plugins");
  const isPaper = info.type === "paper";
  const players = new Map<string, number>(); // nombre -> timestamp de join
  const serverVersion: string = info.version;
  let pendingEcho: string[] = [];
  let logQueue: string[] = [];
  let logFlushOn = false;
  // Líneas ya mostradas: al repintar (ej. cambio de estado) la consola se
  // reconstruye y sin este buffer quedaba en negro.
  let shownLines: string[] = [];

  const running = () => state === "running" || state === "starting" || state === "stopping";

  function paint(): void {
    if (!isActive()) return;
    const dotCls =
      state === "running" ? "ok" :
      state === "starting" || state === "stopping" ? "wait" :
      state === "crashed" ? "bad" : "off";
    const label =
      state === "running" ? "corriendo" :
      state === "starting" ? "arrancando…" :
      state === "stopping" ? "frenando…" :
      state === "crashed" ? "crasheó" : "parado";
    stateEl.innerHTML = `<span class="dot ${dotCls}"></span> ${label}`;
    const on = state === "running" || state === "starting" || state === "stopping";
    toggleBtn.disabled = busy || backingUp;
    toggleBtn.textContent = on ? "⏸ Frenar" : "▶ Iniciar";
    toggleBtn.dataset.tip = backingUp
      ? "Hay un backup en curso: esperá a que termine para arrancar."
      : on
        ? "Apaga el server avisándole antes, así guarda el mundo. Dos veces seguidas = forzar."
        : "Arranca el server. Si falta el Java que necesita, lo descarga solo la primera vez.";
    restartBtn.disabled = busy || !on || backingUp;
    tabConsola.classList.toggle("sel", tab === "consola");
    tabAjustes.classList.toggle("sel", tab === "ajustes");
    tabHistorial.classList.toggle("sel", tab === "historial");
    tabBackups.classList.toggle("sel", tab === "backups");
    tabComandos.classList.toggle("sel", tab === "comandos");
    tabJugadores.classList.toggle("sel", tab === "jugadores");
    tabPlugins?.classList.toggle("sel", tab === "plugins");
    if (tab === "consola") renderConsola();
    else if (tab === "ajustes") void renderAjustes();
    else if (tab === "historial") void renderHistorial();
    else if (tab === "backups") void renderBackups();
    else if (tab === "comandos") renderComandos();
    else if (tab === "jugadores") renderJugadores();
    else void renderPlugins();
  }

  function say(msg: string, isErr: boolean): void {
    msgEl.textContent = msg;
    msgEl.className = isErr ? "error" : "muted";
  }

  function hideCrash(): void {
    crashEl.hidden = true;
    crashEl.innerHTML = "";
  }

  async function showCrash(): Promise<void> {
    if (!isActive()) return;
    let d: import("./api").CrashDiagnosis | null = null;
    try {
      d = await api.diagnoseCrash(name);
    } catch {
      d = null;
    }
    if (!isActive()) return;
    if (d) {
      crashEl.innerHTML =
        `<p><strong>Posible causa: ${esc(d.cause)}</strong></p><p>${esc(d.hint)}</p>`;
    } else {
      crashEl.innerHTML =
        `<p><strong>Se cayó por un motivo desconocido.</strong></p><p>Mirá el final del log en Consola o el crash-report en Historial.</p>`;
    }
    crashEl.hidden = false;
  }

  // ---- Consola ----
  function renderConsola(): void {
    tabBody.innerHTML = `
      <div id="sv-log" class="console"></div>
      <form id="sv-form" class="row">
        <input id="sv-input" placeholder="Comando… (Enter envía)" autocomplete="off" data-tip="Escribí como si fueras la consola del server: say hola, stop, op, etc." />
        <button type="submit" data-tip="Manda lo escrito al server.">Enviar</button>
      </form>`;
    const logEl = tabBody.querySelector<HTMLElement>("#sv-log")!;
    const inputEl = tabBody.querySelector<HTMLInputElement>("#sv-input")!;
    inputEl.disabled = state !== "running";
    (renderConsola as { append?: (l: string) => void }).append = (line: string) => {
      shownLines.push(line);
      if (shownLines.length > MAX_LINES) shownLines.splice(0, shownLines.length - MAX_LINES);
      const stick = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
      const div = document.createElement("div");
      div.textContent = line;
      logEl.appendChild(div);
      while (logEl.children.length > MAX_LINES) logEl.firstChild?.remove();
      if (stick) logEl.scrollTop = logEl.scrollHeight;
    };
    // Repone lo ya visto (el repintado por cambio de estado vaciaba la vista).
    for (const l of shownLines) {
      const div = document.createElement("div");
      div.textContent = l;
      logEl.appendChild(div);
    }
    logEl.scrollTop = logEl.scrollHeight;
    tabBody.querySelector("#sv-form")?.addEventListener("submit", (e) => {
      e.preventDefault();
      const cmd = inputEl.value.trim();
      if (!cmd || state !== "running") return;
      inputEl.value = "";
      appendLine(`> ${cmd}`);
      api.sendCommand(name, cmd).catch((err: unknown) => say(errMsg(err), true));
    });
    void loadHistory();
  }

  function appendLine(line: string): void {
    // Se encola y se vuelca por rAF: con un server que loguea mucho,
    // un append+reflow por línea traba la UI (era el "se traba" general).
    logQueue.push(line);
    if (logQueue.length > MAX_LINES) logQueue.splice(0, logQueue.length - MAX_LINES);
    if (!logFlushOn) {
      logFlushOn = true;
      requestAnimationFrame(() => {
        logFlushOn = false;
        const append = (renderConsola as { append?: (l: string) => void }).append;
        if (!append || logQueue.length === 0) return; // se acumula hasta volver a Consola
        const batch = logQueue;
        logQueue = [];
        for (const l of batch) append(l);
      });
    }
  }

  async function loadHistory(): Promise<void> {
    if (historyLoaded) return; // una sola vez por vista (la cola cubre el resto)
    historyLoaded = true;
    try {
      const hist = await api.readLog(name, 200);
      hist.forEach((l) => {
        appendLine(l);
        trackPlayers(l);
      });
      if (hist.length > 0) appendLine("——— fin del historial ———");
    } catch {
      // Sin historial: no es error.
    }
    // Ecos de atajos mandados desde la pestaña Comandos.
    if (pendingEcho.length > 0) {
      pendingEcho.forEach(appendLine);
      pendingEcho = [];
    }
  }

  // ---- Ajustes ----
  function sel(
    id: string,
    label: string,
    tip: string,
    val: string,
    opts: string[],
    disabled: boolean,
  ): string {
    return `<label data-tip="${tip}">${label}<select id="${id}" ${disabled ? "disabled" : ""}>${opts
      .map((o) => `<option value="${o}" ${o === val ? "selected" : ""}>${o}</option>`)
      .join("")}</select></label>`;
  }

  function field(
    label: string,
    tip: string,
    inner: string,
  ): string {
    return `<label data-tip="${tip}">${label}${inner}</label>`;
  }

  async function renderAjustes(): Promise<void> {
    const dis = running();
    if (props === null && propsError === null) {
      tabBody.innerHTML = `<p class="muted">Cargando ajustes…</p>`;
      try {
        props = await api.getProperties(name);
      } catch (e) {
        propsError = errMsg(e);
      }
      try {
        hostMaxRam = await api.hostRamMb();
      } catch {
        hostMaxRam = 8192;
      }
      try {
        localIps = await api.localIps();
      } catch {
        localIps = [];
      }
    }
    if (propsError !== null) {
      tabBody.innerHTML = `<p class="error">${esc(propsError)}</p>`;
      return;
    }
    const p = props ?? {};
    const num = (id: string, key: string, extra: string, dflt = "") =>
      `<input id="${id}" type="number" ${extra} value="${esc(p[key] ?? dflt)}" ${dis ? "disabled" : ""} />`;
    const txt = (id: string, key: string, dflt: string, extra = "") =>
      `<input id="${id}" type="text" value="${esc(p[key] ?? dflt)}" ${extra} ${dis ? "disabled" : ""} />`;
    tabBody.innerHTML = `
      ${dis ? `<p class="muted">Frená el server para editar (aplica al arrancar).</p>` : ""}
      <div class="props-grid props-list">
        <h3 class="props-sect">Básicos</h3>
        ${field("Puerto", "Por dónde se conectan tus amigos: TU_IP:puerto. Cambialo solo si el 25565 está ocupado.", num("pp-port", "server-port", `min="1" max="65535"`))}
        ${field("IP del server", "Para selfhost sin complicaciones, usá ZeroTier o Radmin VPN y pegá acá la IP que ellos te dan. Vacío = escucha en todas las interfaces.", `<input id="pp-ip" type="text" placeholder="(vacío = todas)" value="${esc(p["server-ip"] ?? "")}" ${dis ? "disabled" : ""} />`)}
        ${(localIps ?? []).length > 0 ? `<p class="muted">Pasales a tus amigos así — IP:Puerto (con Radmin/ZeroTier, la IP es la que te da la VPN): ${(localIps ?? []).map((ip) => esc(`${ip}:${p["server-port"] ?? "25565"}`)).join(" · ")}</p>` : ""}
        ${sel("pp-online", "Online mode", "En true solo entran cuentas premium (originales). En false entra cualquiera, pero se puede usar cualquier nombre.", p["online-mode"] ?? "true", BOOLS, dis)}
        ${sel("pp-diff", "Dificultad", "Daño de monstruos, hambre y veneno: peaceful, easy, normal o hard.", p["difficulty"] ?? "normal", DIFFICULTIES, dis)}
        ${sel("pp-mode", "Gamemode", "Modo de juego al entrar: survival, creative, adventure o spectator.", p["gamemode"] ?? "survival", GAMEMODES, dis)}
        ${sel("pp-pvp", "PVP", "Si los jugadores pueden hacerse daño entre ellos.", p["pvp"] ?? "true", BOOLS, dis)}
        ${sel("pp-wl", "Whitelist", "En true solo entran los de la lista blanca (se agregan con whitelist add).", p["white-list"] ?? "false", BOOLS, dis)}
        ${sel("pp-cmdblock", "Command blocks", "Permite usar bloques de comandos en el mundo. Para mapas de aventura y sistemas automáticos.", p["enable-command-block"] ?? "false", BOOLS, dis)}
        ${sel("pp-fly", "Volar", "Permite volar en creativo o con plugins. En survival sin esto el server te patea por flotar.", p["allow-flight"] ?? "false", BOOLS, dis)}
        ${sel("pp-nether", "Nether", "Permite entrar al Nether. Apagarlo ahorra RAM y CPU en servers chicos.", p["allow-nether"] ?? "true", BOOLS, dis)}
        ${sel("pp-hardcore", "Hardcore", "Muerte permanente: al morir quedás baneado del server. Pensalo dos veces.", p["hardcore"] ?? "false", BOOLS, dis)}
        ${field("Max jugadores", "Cuántos pueden estar a la vez. Más jugadores = más RAM usada.", num("pp-maxp", "max-players", `min="1" max="1000"`))}
        ${field("View distance", "Qué tan lejos se ve, en chunks. Más alto se ve mejor pero pide más RAM y CPU.", num("pp-vd", "view-distance", `min="2" max="32"`))}
        ${field("Simulation distance", "Chunks que simulan entidades y máquinas. Menos = menos CPU.", num("pp-simdist", "simulation-distance", `min="2" max="32"`, "10"))}
        ${field("Protección del spawn", "Radio en bloques donde solo los ops pueden construir. 0 = sin protección.", num("pp-spawnprot", "spawn-protection", `min="0" max="10000"`, "16"))}
        ${field("MOTD", "El mensajito bajo el nombre del server en la lista de servidores.", `<input id="pp-motd" type="text" maxlength="200" value="${esc(p["motd"] ?? "")}" ${dis ? "disabled" : ""} />`)}
        ${field("RAM", "Memoria para este server. 2048 MB alcanza para jugar de a varios.", `<strong id="pp-ram-lbl">${ramMb} MB</strong>
          <input id="pp-ram" type="range" min="512" max="${hostMaxRam}" step="256" value="${Math.min(ramMb, hostMaxRam)}" ${dis ? "disabled" : ""} />`)}
        <details class="adv">
          <summary data-tip="Claves que casi nunca hay que tocar: mundo, query y RCON. Solo con el server frenado.">Avanzado</summary>
          <p class="muted">Solo si sabés lo que hacés. Todo esto también aplica al arrancar.</p>
          <h4>Mundo</h4>
          <div class="adv-body">
            ${field("Nombre del mundo", "Carpeta del mundo dentro del server. Cambiarlo arranca un mundo nuevo (el anterior queda guardado).", txt("pp-levelname", "level-name", "world", `maxlength="64"`))}
            ${field("Seed", "Semilla del mundo. Vacío = aleatoria. Cambiarla solo sirve en un mundo nuevo.", txt("pp-seed", "level-seed", "", `placeholder="(vacío = aleatoria)" maxlength="64"`))}
            ${field("Tipo de mundo", "Ej: minecraft\\:normal, minecraft\\:flat, minecraft\\:large_biomes, minecraft\\:amplified. Solo en mundo nuevo.", txt("pp-leveltype", "level-type", "minecraft\\:normal", `maxlength="64"`))}
            ${field("Generator settings", "JSON de generación (mundos planos o custom). Vacío = default.", txt("pp-gensettings", "generator-settings", "", `placeholder="{}" maxlength="500"`))}
          </div>
          <h4>Juego</h4>
          <div class="adv-body">
            ${field("Max tick time", "Ms antes de considerar colgado al server y frenarlo. -1 = desactivado.", num("pp-ticktime", "max-tick-time", `min="-1"`, "60000"))}
          </div>
          <h4>Acceso remoto (query / RCON)</h4>
          <div class="adv-body">
            ${sel("pp-query", "Query", "Protocolo para ver estado/jugadores desde afuera (webs de estado lo usan).", p["enable-query"] ?? "false", BOOLS, dis)}
            ${field("Puerto query", "Puerto del protocolo query.", num("pp-queryport", "query.port", `min="1" max="65535"`, "25565"))}
            ${sel("pp-rcon", "RCON", "Consola remota: permite mandar comandos desde fuera del juego. Poné password sí o sí.", p["enable-rcon"] ?? "false", BOOLS, dis)}
            ${field("Puerto RCON", "Puerto de la consola remota.", num("pp-rconport", "rcon.port", `min="1" max="65535"`, "25575"))}
            ${field("Password RCON", "Clave de la consola remota. Vacío = sin clave (peligroso si prendés RCON).", `<input id="pp-rconpwd" type="password" autocomplete="off" value="${esc(p["rcon.password"] ?? "")}" ${dis ? "disabled" : ""} />`)}
            ${sel("pp-bcrcon", "Avisar RCON a ops", "Muestra en el chat de ops los comandos que entran por RCON.", p["broadcast-rcon-to-ops"] ?? "true", BOOLS, dis)}
            ${sel("pp-bcconsole", "Avisar consola a ops", "Muestra en el chat de ops los comandos de consola.", p["broadcast-console-to-ops"] ?? "true", BOOLS, dis)}
            ${sel("pp-syncchunks", "Escritura sync de chunks", "Guarda chunks en el acto (más seguro, más lento). En false es más rápido pero arriesga datos.", p["sync-chunk-writes"] ?? "true", BOOLS, dis)}
          </div>
        </details>
      </div>
      <button id="pp-save" type="button" data-tip="Guarda todo. Aplica la próxima vez que arranques." ${dis ? "disabled" : ""}>Guardar</button>
      ${propsMsg ? `<p class="muted">${esc(propsMsg)}</p>` : ""}`;
    const ramInput = tabBody.querySelector<HTMLInputElement>("#pp-ram");
    ramInput?.addEventListener("input", () => {
      const lbl = tabBody.querySelector("#pp-ram-lbl");
      if (lbl && ramInput) lbl.textContent = `${ramInput.value} MB`;
    });
    tabBody.querySelector("#pp-save")?.addEventListener("click", () => void saveProps());
  }

  function val(id: string): string {
    return tabBody.querySelector<HTMLInputElement | HTMLSelectElement>(`#${id}`)?.value ?? "";
  }

  async function saveProps(): Promise<void> {
    propsMsg = null;
    say("Guardando…", false);
    try {
      await api.setProperties(name, {
        "server-port": val("pp-port"),
        "server-ip": val("pp-ip"),
        "online-mode": val("pp-online"),
        difficulty: val("pp-diff"),
        gamemode: val("pp-mode"),
        pvp: val("pp-pvp"),
        "white-list": val("pp-wl"),
        "enable-command-block": val("pp-cmdblock"),
        "allow-flight": val("pp-fly"),
        "allow-nether": val("pp-nether"),
        hardcore: val("pp-hardcore"),
        "max-players": val("pp-maxp"),
        "view-distance": val("pp-vd"),
        motd: val("pp-motd"),
        "level-name": val("pp-levelname"),
        "level-seed": val("pp-seed"),
        "level-type": val("pp-leveltype"),
        "generator-settings": val("pp-gensettings"),
        "spawn-protection": val("pp-spawnprot"),
        "simulation-distance": val("pp-simdist"),
        "max-tick-time": val("pp-ticktime"),
        "enable-query": val("pp-query"),
        "query.port": val("pp-queryport"),
        "enable-rcon": val("pp-rcon"),
        "rcon.port": val("pp-rconport"),
        "rcon.password": val("pp-rconpwd"),
        "broadcast-rcon-to-ops": val("pp-bcrcon"),
        "broadcast-console-to-ops": val("pp-bcconsole"),
        "sync-chunk-writes": val("pp-syncchunks"),
      });
      const ram = Number(tabBody.querySelector<HTMLInputElement>("#pp-ram")?.value ?? ramMb);
      await api.setRam(name, ram);
      ramMb = ram;
      props = await api.getProperties(name);
      propsMsg = "Guardado. Aplica al arrancar.";
      say("Ajustes guardados.", false);
    } catch (e) {
      say(errMsg(e), true);
      return;
    }
    paint();
  }

  // ---- Historial ----
  function kindLabel(kind: string): string {
    switch (kind) {
      case "crash": return "crashlog";
      case "rotated": return "rotado";
      default: return "último";
    }
  }

  function fmtDate(ts: number): string {
    const d = new Date(ts * 1000);
    return `${d.toLocaleDateString()} ${d.toLocaleTimeString()}`;
  }

  async function renderHistorial(): Promise<void> {
    if (histFiles === null && histError === null) {
      tabBody.innerHTML = `<p class="muted">Cargando archivos…</p>`;
      try {
        histFiles = await api.listLogFiles(name);
        if (histFiles.length > 0 && histSel === null) histSel = histFiles[0].file;
      } catch (e) {
        histError = errMsg(e);
      }
    }
    if (histError !== null) {
      tabBody.innerHTML = `<p class="error">${esc(histError)}</p>`;
      return;
    }
    const files = histFiles ?? [];
    if (files.length === 0) {
      tabBody.innerHTML = `<p class="muted">Todavía no hay logs. Arrancá el server una vez para generarlos.</p>`;
      return;
    }
    tabBody.innerHTML = `
      <div class="hist-layout">
        <div class="hist-list">
          ${files
            .map(
              (f) => `<button class="hist-item ${f.file === histSel ? "sel" : ""}" data-file="${esc(f.file)}" type="button">
                <strong>[${kindLabel(f.kind)}]</strong> ${esc(f.file.split("/").pop() ?? f.file)}
                <span class="muted">${fmtDate(f.modified)} · ${(f.size / 1024).toFixed(0)} KB</span>
              </button>`,
            )
            .join("")}
        </div>
        <div id="hist-view" class="console">${histLoading ? "Cargando…" : histLines.map(esc).join("\n")}</div>
      </div>`;
    tabBody.querySelectorAll<HTMLButtonElement>("[data-file]").forEach((b) => {
      b.addEventListener("click", () => {
        histSel = b.dataset.file ?? null;
        void loadHistFile();
      });
    });
    if (histSel !== null && histLines.length === 0 && !histLoading) void loadHistFile();
  }

  async function loadHistFile(): Promise<void> {
    if (histSel === null) return;
    histLoading = true;
    const view = tabBody.querySelector("#hist-view");
    if (view) view.textContent = "Cargando…";
    try {
      histLines = await api.readLogFile(name, histSel, 500);
    } catch (e) {
      histLines = [`Error: ${errMsg(e)}`];
    }
    histLoading = false;
    if (tab === "historial") {
      const v = tabBody.querySelector("#hist-view");
      if (v) {
        v.textContent = histLines.join("\n");
        v.scrollTop = v.scrollHeight;
      }
      tabBody.querySelectorAll<HTMLButtonElement>("[data-file]").forEach((b) => {
        b.classList.toggle("sel", b.dataset.file === histSel);
      });
    }
  }

  // ---- Backups ----
  function bkSize(bytes: number): string {
    if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
    if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(0)} MB`;
    return `${(bytes / 1024).toFixed(0)} KB`;
  }

  function bkWhen(ts: number): string {
    const d = new Date(ts * 1000);
    return `${d.toLocaleDateString()} ${d.toLocaleTimeString()}`;
  }

  async function renderBackups(): Promise<void> {
    if ((bkFiles === null || bkCfg === null) && bkError === null) {
      tabBody.innerHTML = `<p class="muted">Cargando backups…</p>`;
      try {
        bkFiles = await api.listBackups(name);
        backingUp = await api.isBackingUp(name);
        bkCfg = await api.getBackupConfig(name);
      } catch (e) {
        bkError = errMsg(e);
      }
    }
    if (bkError !== null) {
      tabBody.innerHTML = `<p class="error">${esc(bkError)}</p>`;
      return;
    }
    const files = bkFiles ?? [];
    const cfg = bkCfg ?? { auto_enabled: false, auto_hours: 12, auto_scope: "full", keep_count: 10, keep_gb: 0, on_start_enabled: false, on_start_scope: "full", onstart_keep_count: 5, onstart_keep_gb: 0 };
    const retMode = bkRetMode ?? (cfg.keep_gb > 0 && cfg.keep_count === 0 ? "gb" : "count");
    const retModeStart = bkRetModeStart ?? (cfg.onstart_keep_gb > 0 && cfg.onstart_keep_count === 0 ? "gb" : "count");
    const lastAuto = files.filter((f) => f.kind !== "manual").map((f) => f.modified).reduce((a, b) => Math.max(a, b), 0);
    const prog = backingUp
      ? (bkProgress
        ? `Respaldando ${esc(bkProgress.file)}… ${bkProgress.files_done}/${bkProgress.files_total} archivos${bkProgress.pct !== null ? ` (${bkProgress.pct.toFixed(0)}%)` : ""}`
        : "Preparando archivos…")
      : null;
    const panel =
      bkMode === "auto" ? `
        <div class="bk-form">
          <label class="bk-check" data-tip="Cada tantas horas, con el server corriendo, se crea solo un backup. Funciona aunque la ventana esté en el tray."><input id="bk-auto-on" type="checkbox" ${cfg.auto_enabled ? "checked" : ""} /> Backup automático</label>
          <div class="bk-ctl">cada <input id="bk-auto-hours" type="number" min="1" max="720" step="1" value="${cfg.auto_hours}" /> hs · <select id="bk-auto-scope"><option value="full" ${cfg.auto_scope === "full" ? "selected" : ""}>completo</option><option value="world" ${cfg.auto_scope === "world" ? "selected" : ""}>solo mundo</option></select></div>
          <span class="bk-name" data-tip="Cómo limitar los automáticos: por cantidad o por tamaño total. Al pasar el tope se borra el más viejo. Los manuales nunca se borran solos.">Conservar por</span>
          <div class="bk-ctl"><select id="bk-ret-mode" data-tip="Elegí si el tope es en cantidad de backups o en GB totales."><option value="count" ${retMode === "count" ? "selected" : ""}>cantidad</option><option value="gb" ${retMode === "gb" ? "selected" : ""}>tamaño (GB)</option></select>
            <span id="bk-keep-n-wrap">últimos <input id="bk-keep-n" type="number" min="0" max="10000" step="1" value="${cfg.keep_count}" /> automáticos</span>
            <span id="bk-keep-gb-wrap" ${retMode === "gb" ? "" : "hidden"}>hasta <input id="bk-keep-gb" type="number" min="0" max="100000" step="1" value="${cfg.keep_gb}" /> GB</span></div>
          <div class="span row-btns"><button id="bk-auto-save" type="button">Guardar automático</button></div>
          ${lastAuto > 0 ? `<p class="span muted">Último automático: ${bkWhen(lastAuto)}</p>` : ""}
        </div>`
      : bkMode === "onstart" ? `
        <div class="bk-form">
          <label class="bk-check" data-tip="Al arrancar el server se crea solo un backup antes de que entren jugadores. Cuenta como automático para la retención."><input id="bk-onstart-on" type="checkbox" ${cfg.on_start_enabled ? "checked" : ""} /> Backup al arrancar</label>
          <div class="bk-ctl"><select id="bk-onstart-scope"><option value="full" ${cfg.on_start_scope === "full" ? "selected" : ""}>completo</option><option value="world" ${cfg.on_start_scope === "world" ? "selected" : ""}>solo mundo</option></select></div>
          <span class="bk-name" data-tip="Cómo limitar los backups de arranque: por cantidad o por tamaño total. Al pasar el tope se borra el más viejo de este pool.">Conservar por</span>
          <div class="bk-ctl"><select id="bk-ret-mode-start"><option value="count" ${retModeStart === "count" ? "selected" : ""}>cantidad</option><option value="gb" ${retModeStart === "gb" ? "selected" : ""}>tamaño (GB)</option></select>
            <span id="bk-keep-n-start-wrap">últimos <input id="bk-keep-n-start" type="number" min="0" max="10000" step="1" value="${cfg.onstart_keep_count}" /> de arranque</span>
            <span id="bk-keep-gb-start-wrap" ${retModeStart === "gb" ? "" : "hidden"}>hasta <input id="bk-keep-gb-start" type="number" min="0" max="100000" step="1" value="${cfg.onstart_keep_gb}" /> GB</span></div>
          <div class="span row-btns"><button id="bk-onstart-save" type="button">Guardar al arrancar</button></div>
        </div>`
      : `
        <div class="plug-actions">
          <button id="bk-make" type="button" data-tip="Elegís el alcance y se crea la copia. Funciona corriendo; el arranque se inhabilita hasta terminar." ${backingUp || bkBusy ? "disabled" : ""}>${backingUp ? "Respaldando…" : "Crear backup"}</button>
        </div>`;
    tabBody.innerHTML = `
      <div class="plug-actions">
        <label data-tip="Elegí qué clase de backup querés usar o configurar.">Tipo
          <select id="bk-mode">
            <option value="manual" ${bkMode === "manual" ? "selected" : ""}>Backup manual</option>
            <option value="auto" ${bkMode === "auto" ? "selected" : ""}>Backup automático</option>
            <option value="onstart" ${bkMode === "onstart" ? "selected" : ""}>Backup al iniciar</option>
          </select>
        </label>
      </div>
      ${prog !== null ? `<p id="bk-prog" class="muted">${prog}</p><progress id="bk-bar" max="100" value="${bkProgress?.pct ?? 0}"></progress>` : ""}
      ${bkMsg ? `<p class="muted">${esc(bkMsg)}</p>` : ""}
      ${panel}
      <h3>Guardados</h3>
      ${files.length === 0 && !backingUp
        ? `<p class="muted">Todavía no hay backups. Hacé el primero desde Backup manual: si algo se rompe, volvés en un click.</p>`
        : `<div class="players">${files
            .map(
              (f) => `<div class="card">
                <div class="card-icon">💾</div>
                <div class="card-body"><strong>${esc(f.file)}</strong>
                <span class="state">${bkWhen(f.modified)} · ${bkSize(f.size)}</span>
                <span class="badge">${f.scope === "world" ? "solo mundo" : "completo"}</span>${f.kind === "auto" ? ` <span class="badge">auto</span>` : f.kind === "onstart" ? ` <span class="badge">al arrancar</span>` : ""}</div>
                <button data-bk-restore="${esc(f.file)}" type="button" title="Restaurar" ${running() || backingUp || bkBusy ? "disabled" : ""}>↩</button>
                <button data-bk-del="${esc(f.file)}" type="button" title="Borrar">✕</button>
              </div>`,
            )
            .join("")}</div>
          ${running() ? `<p class="muted">Frená el server para restaurar un backup.</p>` : ""}`}`;
    tabBody.querySelector("#bk-mode")?.addEventListener("change", (e) => {
      const v = (e.target as HTMLSelectElement).value;
      bkMode = v === "auto" ? "auto" : v === "onstart" ? "onstart" : "manual";
      paint();
    });
    tabBody.querySelector("#bk-make")?.addEventListener("click", () => openBackupModal());
    tabBody.querySelector("#bk-ret-mode")?.addEventListener("change", (e) => {
      bkRetMode = (e.target as HTMLSelectElement).value === "gb" ? "gb" : "count";
      tabBody.querySelector("#bk-keep-n-wrap")?.toggleAttribute("hidden", bkRetMode !== "count");
      tabBody.querySelector("#bk-keep-gb-wrap")?.toggleAttribute("hidden", bkRetMode !== "gb");
    });
    tabBody.querySelector("#bk-ret-mode-start")?.addEventListener("change", (e) => {
      bkRetModeStart = (e.target as HTMLSelectElement).value === "gb" ? "gb" : "count";
      tabBody.querySelector("#bk-keep-n-start-wrap")?.toggleAttribute("hidden", bkRetModeStart !== "count");
      tabBody.querySelector("#bk-keep-gb-start-wrap")?.toggleAttribute("hidden", bkRetModeStart !== "gb");
    });
    tabBody.querySelector("#bk-auto-save")?.addEventListener("click", () => void saveAutoConfig());
    tabBody.querySelector("#bk-onstart-save")?.addEventListener("click", () => void saveOnStartConfig());
    tabBody.querySelectorAll<HTMLButtonElement>("[data-bk-restore]").forEach((b) => {
      b.addEventListener("click", () => void doRestore(b.dataset.bkRestore ?? ""));
    });
    tabBody.querySelectorAll<HTMLButtonElement>("[data-bk-del]").forEach((b) => {
      b.addEventListener("click", () => void doDeleteBackup(b.dataset.bkDel ?? ""));
    });
  }

  function openBackupModal(): void {
    modalRoot.innerHTML = `
      <div class="overlay">
        <div class="modal">
          <h3>Crear backup</h3>
          <p class="muted">¿Qué incluís en la copia?</p>
          <div class="bk-pick">
            <button id="bk-pick-full" type="button"><strong>Completo</strong><span>Mundo + configs + plugins (sin logs ni jar).</span></button>
            <button id="bk-pick-world" type="button"><strong>Solo mundo</strong><span>Más liviano: sin plugins ni ajustes.</span></button>
            <button id="bk-pick-cancel" type="button">Cancelar</button>
          </div>
        </div>
      </div>`;
    const close = () => {
      modalRoot.innerHTML = "";
    };
    modalRoot.querySelector("#bk-pick-cancel")?.addEventListener("click", close);
    modalRoot.querySelector(".overlay")?.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).classList.contains("overlay")) close();
    });
    modalRoot.querySelector("#bk-pick-full")?.addEventListener("click", () => {
      close();
      void doBackup("full");
    });
    modalRoot.querySelector("#bk-pick-world")?.addEventListener("click", () => {
      close();
      void doBackup("world");
    });
  }

  function bkNum(id: string, dflt: number): number {
    const raw = tabBody.querySelector<HTMLInputElement>(`#${id}`)?.value ?? "";
    const n = Number(raw);
    return Number.isFinite(n) ? n : dflt;
  }

  function baseCfg(): import("./api").BackupConfig {
    return bkCfg ?? {
      auto_enabled: false,
      auto_hours: 12,
      auto_scope: "full",
      keep_count: 10,
      keep_gb: 0,
      on_start_enabled: false,
      on_start_scope: "full",
      onstart_keep_count: 5,
      onstart_keep_gb: 0,
    };
  }

  async function saveAutoConfig(): Promise<void> {
    bkMsg = null;
    const byCount =
      (tabBody.querySelector<HTMLSelectElement>("#bk-ret-mode")?.value ?? "count") === "count";
    const cfg: import("./api").BackupConfig = {
      ...baseCfg(),
      auto_enabled: tabBody.querySelector<HTMLInputElement>("#bk-auto-on")?.checked ?? false,
      auto_hours: Math.floor(bkNum("bk-auto-hours", 12)),
      auto_scope: tabBody.querySelector<HTMLSelectElement>("#bk-auto-scope")?.value === "world" ? "world" : "full",
      keep_count: byCount ? Math.floor(bkNum("bk-keep-n", 10)) : 0,
      keep_gb: byCount ? 0 : bkNum("bk-keep-gb", 0),
    };
    say("Guardando automático…", false);
    try {
      bkCfg = await api.setBackupConfig(name, cfg);
      bkRetMode = byCount ? "count" : "gb";
      bkMsg = cfg.auto_enabled
        ? `Listo: backup ${cfg.auto_scope === "world" ? "solo mundo" : "completo"} cada ${cfg.auto_hours} hs con el server corriendo.`
        : "Automático apagado.";
      say("Config de backups guardada.", false);
    } catch (e) {
      say(errMsg(e), true);
      return;
    }
    paint();
  }

  async function saveOnStartConfig(): Promise<void> {
    bkMsg = null;
    const byCount =
      (tabBody.querySelector<HTMLSelectElement>("#bk-ret-mode-start")?.value ?? "count") === "count";
    const cfg: import("./api").BackupConfig = {
      ...baseCfg(),
      on_start_enabled: tabBody.querySelector<HTMLInputElement>("#bk-onstart-on")?.checked ?? false,
      on_start_scope: tabBody.querySelector<HTMLSelectElement>("#bk-onstart-scope")?.value === "world" ? "world" : "full",
      onstart_keep_count: byCount ? Math.floor(bkNum("bk-keep-n-start", 5)) : 0,
      onstart_keep_gb: byCount ? 0 : bkNum("bk-keep-gb-start", 0),
    };
    say("Guardando backup al arrancar…", false);
    try {
      bkCfg = await api.setBackupConfig(name, cfg);
      bkRetModeStart = byCount ? "count" : "gb";
      bkMsg = cfg.on_start_enabled ? "Listo: backup solo al arrancar el server." : "Backup al arrancar apagado.";
      say("Config de backups guardada.", false);
    } catch (e) {
      say(errMsg(e), true);
      return;
    }
    paint();
  }

  async function doBackup(scope: "full" | "world"): Promise<void> {
    if (bkBusy || backingUp) return;
    bkBusy = true;
    bkMsg = null;
    paint();
    try {
      const rep = await api.createBackup(name, scope);
      bkMsg = `Backup listo: ${rep.file}.`;
      bkFiles = null; // refrescar lista
    } catch (e) {
      say(errMsg(e), true);
    }
    bkBusy = false;
    paint();
  }

  async function doRestore(file: string): Promise<void> {
    if (!file || running() || backingUp || bkBusy) return;
    const ok = window.confirm(
      `¿Restaurar "${file}"? Reemplaza el mundo y la config actual. Lo de ahora se pierde.`,
    );
    if (!ok) return;
    bkBusy = true;
    paint();
    try {
      await api.restoreBackup(name, file);
      bkMsg = "Restaurado. Arrancá el server para jugar en ese punto.";
      say("Backup restaurado.", false);
    } catch (e) {
      say(errMsg(e), true);
    }
    bkBusy = false;
    paint();
  }

  async function doDeleteBackup(file: string): Promise<void> {
    if (!file || backingUp || bkBusy) return;
    if (!window.confirm(`¿Borrar el backup "${file}"?`)) return;
    bkBusy = true;
    paint();
    try {
      await api.deleteBackup(name, file);
      bkFiles = null;
    } catch (e) {
      say(errMsg(e), true);
    }
    bkBusy = false;
    paint();
  }

  // ---- Jugadores (parse join/leave del log) ----
  const JOIN_RE = /([A-Za-z0-9_]{3,16}) (joined|left) the game/;

  function trackPlayers(line: string): void {
    const m = JOIN_RE.exec(line);
    if (!m) return;
    if (m[2] === "joined") players.set(m[1], Date.now());
    else players.delete(m[1]);
    if (tab === "jugadores") renderJugadores();
  }

  function renderJugadores(): void {
    const list = [...players.entries()].sort((a, b) => a[0].localeCompare(b[0]));
    tabBody.innerHTML = `
      <p class="muted" data-tip="Se detecta del log: si el server se reinició, la lista arranca vacía.">${list.length} conectado${list.length === 1 ? "" : "s"}</p>
      ${list.length === 0
        ? `<p class="muted">Nadie por ahora. Cuando alguien entre, aparece acá.</p>`
        : `<div class="players">${list
            .map(
              ([n, since]) =>
                `<div class="card"><div class="card-icon">⛏️</div><div class="card-body"><strong>${esc(n)}</strong><span class="state">en línea desde ${new Date(since).toLocaleTimeString()}</span></div></div>`,
            )
            .join("")}</div>`}`;
  }

  // ---- Comandos rápidos (SPEC) ----
  function cmdVal(id: string): string {
    return tabBody.querySelector<HTMLInputElement | HTMLSelectElement>(`#${id}`)?.value.trim() ?? "";
  }

  function quickRow(label: string, tip: string, inner: string): string {
    return `<div class="quick-row" data-tip="${tip}"><strong>${label}</strong><div class="quick-inputs">${inner}</div><button data-quick="${label}" type="button">Enviar</button></div>`;
  }

  function renderComandos(): void {
    const dis = state !== "running";
    tabBody.innerHTML = `
      ${dis ? `<p class="muted">Arrancá el server para usar los atajos.</p>` : ""}
      <div class="quicks">
        ${quickRow("say", "Manda un mensaje a todos los conectados.", `<input id="q-say" placeholder="Mensaje" ${dis ? "disabled" : ""} />`)}
        ${quickRow("op", "Le da operador (admin) a un jugador.", `<input id="q-op" placeholder="Jugador" ${dis ? "disabled" : ""} />`)}
        ${quickRow("give", "Le da un item a un jugador. Ej: diamond_sword 1.", `<input id="q-give-p" placeholder="Jugador" ${dis ? "disabled" : ""} /><input id="q-give-i" placeholder="Item" ${dis ? "disabled" : ""} /><input id="q-give-c" placeholder="Cant." ${dis ? "disabled" : ""} />`)}
        ${quickRow("time set", "Cambia la hora del mundo.", `<select id="q-time" ${dis ? "disabled" : ""}><option value="day">day</option><option value="noon">noon</option><option value="night">night</option><option value="midnight">midnight</option></select><input id="q-time-n" placeholder="o ticks" ${dis ? "disabled" : ""} />`)}
        ${quickRow("gamemode", "Cambia el modo de juego de alguien.", `<select id="q-gm-m" ${dis ? "disabled" : ""}><option>survival</option><option>creative</option><option>adventure</option><option>spectator</option></select><input id="q-gm-p" placeholder="Jugador" ${dis ? "disabled" : ""} />`)}
        ${quickRow("whitelist add", "Agrega a alguien a la lista blanca.", `<input id="q-wl" placeholder="Jugador" ${dis ? "disabled" : ""} />`)}
      </div>`;
    const builders: Record<string, () => string | null> = {
      say: () => {
        const t = cmdVal("q-say");
        return t ? `say ${t}` : null;
      },
      op: () => {
        const t = cmdVal("q-op");
        return t ? `op ${t}` : null;
      },
      give: () => {
        const p = cmdVal("q-give-p");
        const i = cmdVal("q-give-i");
        const c = cmdVal("q-give-c");
        return p && i ? `give ${p} ${i}${c ? ` ${c}` : ""}` : null;
      },
      "time set": () => {
        const n = cmdVal("q-time-n");
        return n ? `time set ${n}` : `time set ${cmdVal("q-time")}`;
      },
      gamemode: () => {
        const p = cmdVal("q-gm-p");
        return p ? `gamemode ${cmdVal("q-gm-m")} ${p}` : null;
      },
      "whitelist add": () => {
        const t = cmdVal("q-wl");
        return t ? `whitelist add ${t}` : null;
      },
    };
    tabBody.querySelectorAll<HTMLButtonElement>("[data-quick]").forEach((b) => {
      b.addEventListener("click", () => {
        if (state !== "running") return;
        const cmd = builders[b.dataset.quick ?? ""]?.();
        if (!cmd) {
          say("Completá los campos del atajo.", true);
          return;
        }
        api.sendCommand(name, cmd).catch((err: unknown) => say(errMsg(err), true));
        pendingEcho.push(`> ${cmd}`);
        say(`Enviado: ${cmd}`, false);
      });
    });
  }

  // ---- Plugins (base para futuro Mods) ----
  let plugFiles: import("./api").PluginInfo[] | null = null;
  let plugError: string | null = null;
  let plugBusy = false;
  let plugSub: "list" | "search" = "list";
  let searchQuery = "";
  let searchCat: string | null = null;
  let searchHits: import("./api").SearchHit[] = [];
  let searching = false;
  let searchError: string | null = null;
  let searchTouched = false;
  let installingId: string | null = null;
  let installProgress: import("./api").PluginProgress | null = null;
  let installMsg: string | null = null;

  async function renderPlugins(): Promise<void> {
    const n = plugFiles?.length ?? 0;
    tabBody.innerHTML = `
      <div class="tabs sub">
        <button id="psub-list" type="button" class="${plugSub === "list" ? "sel" : ""}" data-tip="Los .jar que ya tiene este server.">Instalados${plugFiles ? ` (${n})` : ""}</button>
        <button id="psub-search" type="button" class="${plugSub === "search" ? "sel" : ""}" data-tip="Buscar en Modrinth e instalar en un click.">Buscar plugins</button>
      </div>
      <div id="psub-body"></div>`;
    const body = tabBody.querySelector<HTMLElement>("#psub-body")!;
    tabBody.querySelector("#psub-list")?.addEventListener("click", () => {
      plugSub = "list";
      paint();
    });
    tabBody.querySelector("#psub-search")?.addEventListener("click", () => {
      plugSub = "search";
      paint();
    });
    if (plugSub === "search") {
      // Primera vez en el buscador: mostrar populares (query vacía).
      if (!searchTouched) {
        searchTouched = true;
        void doSearch();
        return;
      }
      renderPlugSearch(body);
    } else {
      await renderPlugList(body);
    }
  }

  async function renderPlugList(el: HTMLElement): Promise<void> {
    if (plugFiles === null && plugError === null) {
      el.innerHTML = `<p class="muted">Cargando plugins…</p>`;
      try {
        plugFiles = await api.listPlugins(name);
      } catch (e) {
        plugError = errMsg(e);
      }
      if (tab === "plugins" && plugSub === "list") paint();
      return;
    }
    if (plugError !== null) {
      el.innerHTML = `<p class="error">${esc(plugError)}</p>`;
      return;
    }
    const files = plugFiles ?? [];
    el.innerHTML = `
      <div class="plug-actions">
        <button id="plug-add" type="button" data-tip="Elegí un .jar de tu compu para sumarlo a este server.">Importar .jar</button>
        <span class="muted">o arrastrá el .jar acá adentro. Cambios aplican al reiniciar.</span>
      </div>
      <div id="plug-drop" class="plug-drop" hidden> soltá para importar </div>
      ${plugBusy ? `<p class="muted">Trabajando…</p>` : ""}
      ${files.length === 0
        ? `<p class="muted">Sin plugins. Los .jar van a la carpeta plugins/ del server.</p>`
        : `<div class="players">${files
            .map(
              (f) => `<div class="card ${f.enabled ? "" : "off"}">
                <div class="card-icon">${f.enabled ? "🧩" : "💤"}</div>
                <div class="card-body"><strong>${esc(f.file)}</strong>
                <span class="state">${f.enabled ? "prendido" : "apagado"} · ${(f.size / 1024).toFixed(0)} KB</span></div>
                <button data-plug-toggle="${esc(f.file)}" type="button" title="${f.enabled ? "Apagar" : "Prender"}">${f.enabled ? "⏸" : "▶"}</button>
                <button data-plug-del="${esc(f.file)}" type="button" title="Borrar">✕</button>
              </div>`,
            )
            .join("")}</div>`}`;
    el.querySelector("#plug-add")?.addEventListener("click", () => void pickPlugins());
    el.querySelectorAll<HTMLButtonElement>("[data-plug-toggle]").forEach((b) => {
      b.addEventListener("click", async () => {
        const file = b.dataset.plugToggle ?? "";
        const cur = plugFiles?.find((p) => p.file === file);
        plugBusy = true;
        paint();
        try {
          await api.setPluginEnabled(name, file, !(cur?.enabled ?? true));
          plugFiles = null;
        } catch (e) {
          say(errMsg(e), true);
        }
        plugBusy = false;
        paint();
      });
    });
    el.querySelectorAll<HTMLButtonElement>("[data-plug-del]").forEach((b) => {
      b.addEventListener("click", async () => {
        const file = b.dataset.plugDel ?? "";
        if (!window.confirm(`¿Borrar el plugin "${file}"?`)) return;
        plugBusy = true;
        paint();
        try {
          await api.deletePlugin(name, file);
          plugFiles = null;
        } catch (e) {
          say(errMsg(e), true);
        }
        plugBusy = false;
        paint();
      });
    });
  }

  function renderPlugSearch(el: HTMLElement): void {
    const mcVersion = serverVersion;
    const cats: Array<[string, string | null]> = [
      ["Todos", null],
      ["Economía", "economy"],
      ["Minijuegos", "minigame"],
      ["Gestión", "management"],
      ["Social", "social"],
      ["Utilidades", "utility"],
      ["Magia", "magic"],
      ["Aventura", "adventure"],
    ];
    el.innerHTML = `
      <form id="plug-search-form" class="row" style="gap:.5rem;margin-bottom:.5rem">
        <input id="plug-q" placeholder="ej: essentials… o vacío para ver populares" value="${esc(searchQuery)}" style="flex:1" data-tip="Busca en Modrinth. Vacío = los más descargados." />
        <button type="submit">Buscar</button>
      </form>
      <div class="chips" style="margin-bottom:.75rem">
        ${cats.map(([label, slug]) => `<button data-cat="${slug ?? ""}" type="button" class="${(searchCat ?? null) === slug ? "sel" : ""}" data-tip="${slug ? `Solo ${label.toLowerCase()}` : "Sin filtro: todo"}">${label}</button>`).join("")}
      </div>
      ${searching ? `<p class="muted">Buscando…</p>` : ""}
      ${searchError ? `<p class="error">${esc(searchError)}</p>` : ""}
      ${!searching && searchTouched && searchHits.length === 0 && !searchError ? `<p class="muted">Sin resultados. Probá con otra palabra, otra categoría o la búsqueda vacía.</p>` : ""}
      ${searchHits.length > 0 ? `<div class="players">${searchHits
        .map(
          (h, i) => {
            const compat = h.game_versions.includes(mcVersion);
            return `<div class="card plug" data-idx="${i}" data-tip="Click para ver detalle, fotos y descripción completa.">
              <div class="card-icon">${h.icon_url ? `<img src="${esc(h.icon_url)}" alt="" loading="lazy" />` : "🔌"}</div>
              <div class="card-body"><strong>${esc(h.title)}</strong>
                <span class="state">por ${esc(h.author)} · ${(h.downloads / 1000).toFixed(0)}k descargas</span>
                <span class="badge">${compat ? `compatible con tu ${esc(mcVersion)}` : "revisá compatibilidad"}</span>
                <span class="muted plug-desc">${esc(h.description)}</span></div>
              <button data-install="${esc(h.project_id)}" type="button" ${installingId ? "disabled" : ""}>${installingId === h.project_id ? "…" : "Instalar"}</button>
            </div>`;
          },
        )
        .join("")}</div>` : ""}
      ${installProgress ? `<p id="plug-prog" class="muted">Bajando ${esc(installProgress.file)}… ${installProgress.pct !== null ? `${installProgress.pct.toFixed(0)}%` : ""}</p><progress max="100" value="${installProgress.pct ?? 0}"></progress>` : ""}
      ${installMsg ? `<p class="muted">${esc(installMsg)}</p>` : ""}`;
    el.querySelector("#plug-search-form")?.addEventListener("submit", (e) => {
      e.preventDefault();
      searchQuery = el.querySelector<HTMLInputElement>("#plug-q")?.value.trim() ?? "";
      void doSearch();
    });
    el.querySelectorAll<HTMLButtonElement>("[data-cat]").forEach((b) => {
      b.addEventListener("click", () => {
        const slug = b.dataset.cat || null;
        if (searchCat === slug) return;
        searchCat = slug;
        void doSearch();
      });
    });
    // Click en la card abre el detalle (el botón Instalar no).
    el.querySelectorAll<HTMLElement>(".card.plug").forEach((c) => {
      c.addEventListener("click", (e) => {
        if ((e.target as HTMLElement).closest("button")) return;
        const hit = searchHits[Number(c.dataset.idx ?? -1)];
        if (hit) openPlugModal(hit);
      });
    });
    el.querySelectorAll<HTMLButtonElement>("[data-install]").forEach((b) => {
      b.addEventListener("click", () => void doInstall(b.dataset.install ?? ""));
    });
  }

  async function doSearch(): Promise<void> {
    searching = true;
    searchError = null;
    searchHits = [];
    paint();
    try {
      searchHits = await api.searchPlugins(searchQuery, searchCat);
    } catch (e) {
      searchError = errMsg(e);
    }
    searching = false;
    if (tab === "plugins") paint();
  }

  async function doInstall(projectId: string): Promise<void> {
    installingId = projectId;
    installProgress = null;
    installMsg = null;
    paint();
    try {
      const rep = await api.installPlugin(name, projectId);
      const parts = [`Instalado: ${rep.installed.join(", ") || "nada nuevo"}.`];
      if (rep.skipped.length > 0) parts.push(`Ya estaban: ${rep.skipped.join(", ")}.`);
      if (rep.optional_deps.length > 0) {
        parts.push(`Opcionales que podrías querer: ${rep.optional_deps.join(", ")} (no se instalan solas).`);
      }
      parts.push("Reiniciá el server para que carguen.");
      installMsg = parts.join(" ");
      plugFiles = null; // refrescar instalados
    } catch (e) {
      installMsg = null;
      say(errMsg(e), true);
    }
    installingId = null;
    installProgress = null;
    paint();
  }

  // Markdown/HTML mínimo a texto (los body de Modrinth mezclan ambos).
  // OJO: solo se sacan tags conocidos; `<mobName>` o `<jugador>` son texto
  // y deben quedar (un strip genérico se los comería).
  function mdText(md: string): string {
    return md
      .replace(/<script[\s\S]*?<\/script>/gi, "")
      .replace(/<style[\s\S]*?<\/style>/gi, "")
      .replace(/<img[^>]*>/gi, "")
      .replace(/<br\s*\/?>/gi, "\n")
      .replace(/<hr\s*\/?>/gi, "\n---\n")
      .replace(/<li[^>]*>/gi, "\n- ")
      .replace(/<\/(p|div|h[1-6]|tr|table|ul|ol|center|blockquote|li)>/gi, "\n")
      .replace(/<(center|div|p|h[1-6]|ul|ol|table|tr|td|th|br|hr|a|span|strong|em|b|i|u|code|pre|blockquote|sub|sup|font)\b[^>]*>/gi, "")
      .replace(/<\/(a|span|strong|em|b|i|u|code|pre|td|th|font|sub|sup)>/gi, "")
      .replace(/&nbsp;/gi, " ")
      .replace(/&amp;/gi, "&")
      .replace(/&lt;/gi, "<")
      .replace(/&gt;/gi, ">")
      .replace(/&quot;/gi, '"')
      .replace(/&#39;/gi, "'")
      .replace(/!\[[^\]]*\]\([^)]+\)/g, "")
      .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
      .replace(/^#{1,6}\s?/gm, "")
      .replace(/\*\*([^*]+)\*\*/g, "$1")
      .replace(/__([^_]+)__/g, "$1")
      .replace(/`([^`]+)`/g, "$1")
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
  }

  function openPlugModal(hit: import("./api").SearchHit): void {
    const compat = hit.game_versions.includes(serverVersion);
    modalRoot.innerHTML = `
      <div class="overlay">
        <div class="modal plug-modal">
          <div class="plug-head">
            <div class="card-icon big">${hit.icon_url ? `<img src="${esc(hit.icon_url)}" alt="" />` : "🔌"}</div>
            <div class="card-body"><strong>${esc(hit.title)}</strong>
              <span class="state">por ${esc(hit.author)} · ${(hit.downloads / 1000).toFixed(0)}k descargas</span>
              <span class="badge">${compat ? `compatible con tu ${esc(serverVersion)}` : "revisá compatibilidad"}</span></div>
            <button id="plug-m-close" type="button" title="Cerrar">✕</button>
          </div>
          <div id="plug-m-body"><p class="muted">Cargando detalle…</p></div>
          <div class="row-btns" style="margin-top:.75rem">
            <button id="plug-m-install" type="button" ${installingId ? "disabled" : ""}>Instalar</button>
          </div>
        </div>
      </div>`;
    const close = () => {
      modalRoot.innerHTML = "";
    };
    modalRoot.querySelector("#plug-m-close")?.addEventListener("click", close);
    modalRoot.querySelector(".overlay")?.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).classList.contains("overlay")) close();
    });
    modalRoot.querySelector("#plug-m-install")?.addEventListener("click", () => {
      close();
      void doInstall(hit.project_id);
    });
    api.pluginDetails(hit.project_id).then((d) => {
      const body = modalRoot.querySelector("#plug-m-body");
      if (!body) return; // se cerró antes de que llegue
      const long = mdText(d.body);
      body.innerHTML = `
        ${d.gallery.length > 0 ? `<div class="plug-gal">${d.gallery
          .map((g) => `<img src="${esc(g.url)}" alt="${esc(g.title)}" title="${esc(g.title)}" loading="lazy" />`)
          .join("")}</div>` : ""}
        <p>${esc(hit.description)}</p>
        ${long && long !== hit.description.trim() ? `<div class="plug-long">${esc(long)}</div>` : ""}`;
    }).catch(() => {
      const body = modalRoot.querySelector("#plug-m-body");
      if (!body) return;
      body.innerHTML = `<p>${esc(hit.description)}</p><p class="muted">No se pudo traer la descripción larga ni las fotos.</p>`;
    });
  }

  async function pickPlugins(): Promise<void> {
    const sel = await open({
      multiple: true,
      filters: [{ name: "Plugins", extensions: ["jar"] }],
    }).catch(() => null);
    const paths = Array.isArray(sel) ? sel : sel ? [sel] : [];
    if (paths.length === 0) return;
    await importPluginPaths(paths);
  }

  async function importPluginPaths(paths: string[]): Promise<void> {
    plugBusy = true;
    paint();
    let ok = 0;
    let lastErr = "";
    for (const p of paths) {
      try {
        await api.importPlugin(name, p);
        ok += 1;
      } catch (e) {
        lastErr = errMsg(e);
      }
    }
    plugFiles = null;
    plugBusy = false;
    paint();
    if (ok > 0) say(`Importado${ok === 1 ? "" : "s"} ${ok} plugin${ok === 1 ? "" : "s"}.`, false);
    if (lastErr) say(lastErr, true);
  }

  // ---- Stats en vivo (solo DEL server) + aviso de memoria de la PC ----
  function fmtGB(mb: number): string {
    return `${(mb / 1024).toFixed(1)} GB`;
  }
  let lastStatWarn = 0;

  async function pollStats(): Promise<void> {
    if (!isActive()) return;
    try {      const st = await api.serverStats();
      if (!isActive()) return;
      const mine = st.servers.find((s) => s.name === name);
      if (mine) {
        usageEl.textContent =
          `CPU ${mine.cpu_pct.toFixed(0)}% · RAM ${mine.ram_mb} MB de ${ramMb} MB máx`;
      } else {
        usageEl.textContent = `${ramMb} MB asignados · parado`;
      }
      // Tooltip con el TOTAL de la PC (el texto muestra solo este server).
      usageEl.dataset.tip =
        `Toda tu PC: CPU ${st.host.cpu_pct.toFixed(0)}% · RAM ${fmtGB(st.host.used_mb)} de ${fmtGB(st.host.total_mb)} en uso`;
      // Aviso PC: usado + máximo del server > 75% del total.
      const total = st.host.total_mb;
      const projected = st.host.used_mb + ramMb;
      if (total > 0 && projected / total > 0.75) {
        memwarnEl.hidden = false;
        memwarnEl.innerHTML =
          `<p>⚠️ Ojo: con este server prendido usarías ~${fmtGB(projected)} de ${fmtGB(total)} ` +
          `(${((projected / total) * 100).toFixed(0)}% de tu PC). Puede trabarte todo.</p>`;
      } else {
        memwarnEl.hidden = true;
        memwarnEl.innerHTML = "";
      }
    } catch (e) {
      // Poll best-effort: no rompe la vista, pero avisa en consola (throttle
      // 30s) para que un fallo sistemático no sea invisible.
      const now = Date.now();
      if (now - lastStatWarn > 30000) {
        lastStatWarn = now;
        console.warn("[digspawn] serverStats falló:", e);
      }
    }
  }

  // ---- Eventos ----
  // Cada handler ignora eventos si la vista ya no está activa: evita que una
  // vista vieja (listeners aún vivos durante la transición) pinte sobre la
  // biblioteca u otra vista.
  unlistens.push(
    await listen<LogLine>("log-line", (ev) => {
      if (!isActive()) return;
      if (ev.payload.server !== name) return;
      trackPlayers(ev.payload.line);
      if (tab === "consola") appendLine(ev.payload.line);
    }),
    await listen<ServerStateEvent>("server-state", (ev) => {
      if (!isActive()) return;
      if (ev.payload.server !== name) return;
      state = ev.payload.state;
      busy = false;
      if (state === "running") {
        say("Corriendo.", false);
        javaEl.hidden = true;
        stopRequested = false;
        hideCrash();
      } else if (state === "crashed") {
        say("El server crasheó. Mirá el final del log.", true);
        stopRequested = false;
        histFiles = null; // hay crashlog nuevo para ver en Historial
        histSel = null;
        histLines = [];
        void showCrash();
      } else if (state === "stopped") {
        say("Parado.", false);
        stopRequested = false;
        hideCrash();
      }
      if (tab === "ajustes") {
        props = null; // recargar (los controles se habilitan/deshabilitan)
        propsError = null;
      }
      paint();
      void pollStats();
    }),
    await listen<RuntimeProgress>("runtime-progress", (ev) => {
      if (!isActive()) return;
      if (ev.payload.server !== name) return;
      javaEl.hidden = false;
      const pct = ev.payload.pct !== null ? ` ${ev.payload.pct.toFixed(0)}%` : "";
      javaEl.textContent = `Bajando Java ${ev.payload.version} portable…${pct} (solo la primera vez)`;
    }),
    await listen<import("./api").PluginProgress>("plugin-progress", (ev) => {
      if (!isActive()) return;
      if (ev.payload.server !== name || tab !== "plugins") return;
      installProgress = ev.payload;
      // Update en el lugar (sin re-render que mataría el input de búsqueda).
      const bar = tabBody.querySelector<HTMLProgressElement>("progress");
      if (bar && ev.payload.pct !== null) bar.value = ev.payload.pct;
      const txt = tabBody.querySelector("#plug-prog");
      if (txt && ev.payload.pct !== null) {
        txt.textContent = `Bajando ${ev.payload.file}… ${ev.payload.pct.toFixed(0)}%`;
      }
    }),
    await listen<import("./api").BackupProgress>("backup-progress", (ev) => {
      if (!isActive()) return;
      if (ev.payload.server !== name) return;
      const started = !backingUp;
      backingUp = true;
      bkProgress = ev.payload;
      // Solo se repinta al arrancar (inhabilita botones); el resto va en el lugar.
      if (started || tab !== "backups") {
        paint();
        return;
      }
      // Update en el lugar (sin re-render que mataría el select de alcance).
      const bar = tabBody.querySelector<HTMLProgressElement>("#bk-bar");
      if (bar && ev.payload.pct !== null) bar.value = ev.payload.pct;
      const txt = tabBody.querySelector("#bk-prog");
      if (txt) {
        txt.textContent =
          `Respaldando ${ev.payload.file}… ${ev.payload.files_done}/${ev.payload.files_total} archivos${ev.payload.pct !== null ? ` (${ev.payload.pct.toFixed(0)}%)` : ""}`;
      }
    }),
    await listen<import("./api").BackupStateEvent>("backup-state", (ev) => {
      if (!isActive()) return;
      if (ev.payload.server !== name) return;
      backingUp = ev.payload.backing_up;
      if (!backingUp) {
        bkFiles = null; // refrescar lista al terminar
        bkProgress = null;
      }
      paint();
      void pollStats();
    }),
  );

  async function run(op: () => Promise<void>, label: string): Promise<void> {
    busy = true;
    paint();
    say(label, false);
    try {
      await op();
    } catch (e) {
      busy = false;
      say(errMsg(e), true);
      paint();
    }
  }

  async function startWithPreflight(): Promise<void> {
    try {
      const pf = await api.preflight(name);
      if (pf.warnings.length > 0) {
        const ok = window.confirm(
          `${pf.warnings.join("\n")}\n\n¿Arrancar igual? (Cancelar = volver y bajar la RAM en Ajustes)`,
        );
        if (!ok) return;
      }
    } catch (e) {
      say(`No se pudo chequear memoria: ${errMsg(e)}`, true);
      return;
    }
    await run(() => api.startServer(name), "Arrancando…");
  }

  async function onToggle(): Promise<void> {
    if (!isActive()) return;
    const active = state === "running" || state === "starting" || state === "stopping";
    // El 2º Frenar abre el modal aunque el botón esté en busy: si no,
    // el modal era inalcanzable (busy deshabilita hasta que muere).
    if (stopRequested && active) {
      openForceModal();
      return;
    }
    if (busy) return;
    if (active) {
      if (stopRequested) {
        openForceModal();
        return;
      }
      if (!confirmKickPlayers()) return;
      stopRequested = true;
      await run(() => api.stopServer(name), "Frenando… (otra vez = forzar)");
    } else {
      await startWithPreflight();
    }
  }

  // Si hay gente jugando, pedir confirmación antes de patearlos.
  // Devuelve false si se cancela.
  function confirmKickPlayers(): boolean {
    if (players.size === 0) return true;
    const names = [...players.keys()].join(", ");
    return window.confirm(
      `Hay ${players.size} conectado${players.size === 1 ? "" : "s"} (${names}). ¿Frenar igual? Se van a desconectar.`,
    );
  }

  function openForceModal(): void {
    modalRoot.innerHTML = `
      <div class="overlay">
        <div class="modal">
          <h3>¿Forzar el apagado?</h3>
          <p>Esto mata el proceso en seco, <strong>sin guardar</strong>. Puede dañar
          guardados o el mundo. Usalo como último recurso, si el frenado normal
          no responde.</p>
          <div class="row" style="gap:.5rem;justify-content:flex-end">
            <button id="sv-force-cancel" type="button">Cancelar</button>
            <button id="sv-force-go" type="button" class="danger">Forzar apagado</button>
          </div>
        </div>
      </div>`;
    modalRoot.querySelector("#sv-force-cancel")?.addEventListener("click", closeForceModal);
    modalRoot.querySelector("#sv-force-go")?.addEventListener("click", async () => {
      closeForceModal();
      stopRequested = false;
      await run(() => api.forceStop(name), "Forzando apagado…");
    });
  }

  function closeForceModal(): void {
    modalRoot.innerHTML = "";
  }

  toggleBtn.addEventListener("click", () => void onToggle());
  restartBtn.addEventListener("click", () => {
    if (!isActive()) return;
    if (!confirmKickPlayers()) return;
    void run(() => api.restartServer(name), "Reiniciando…");
  });

  tabConsola.addEventListener("click", () => {
    tab = "consola";
    paint();
  });
  tabAjustes.addEventListener("click", () => {
    tab = "ajustes";
    paint();
  });
  tabComandos.addEventListener("click", () => {
    tab = "comandos";
    paint();
  });
  tabJugadores.addEventListener("click", () => {
    tab = "jugadores";
    paint();
  });
  tabPlugins?.addEventListener("click", () => {
    if (!isPaper) return;
    tab = "plugins";
    paint();
  });
  tabHistorial.addEventListener("click", () => {
    tab = "historial";
    paint();
  });
  tabBackups.addEventListener("click", () => {
    tab = "backups";
    paint();
  });

  view.querySelector("#sv-back")?.addEventListener("click", () => {
    mounted = false;
    unlistens.forEach((u) => {
      try {
        u();
      } catch {
        // best-effort
      }
    });
    unlistens.length = 0;
    window.clearInterval(pollTimer);
    onBack();
  });

  // Drag & drop de .jar (solo actúa en la pestaña Plugins).
  try {
    const dropUnlisten = await getCurrentWebview().onDragDropEvent((ev) => {
      if (!isActive()) return;
      if (tab !== "plugins") return;
      if (ev.payload.type === "over" || ev.payload.type === "enter") {
        tabBody.querySelector("#plug-drop")?.removeAttribute("hidden");
      } else if (ev.payload.type === "leave") {
        tabBody.querySelector("#plug-drop")?.setAttribute("hidden", "");
      } else if (ev.payload.type === "drop") {
        tabBody.querySelector("#plug-drop")?.setAttribute("hidden", "");
        const jars = ev.payload.paths.filter((p) => p.toLowerCase().endsWith(".jar"));
        if (jars.length === 0) {
          say("Eso no es un .jar.", true);
          return;
        }
        void importPluginPaths(jars);
      }
    });
    unlistens.push(dropUnlisten);
  } catch {
    // Sin drag&drop queda el botón Importar (no es error).
  }

  paint();
  pollTimer = window.setInterval(() => void pollStats(), 2000);
  void pollStats();
  // Si se entra con el server ya crasheado, mostrar el diagnóstico igual.
  if (state === "crashed") void showCrash();

  // Icono: el avatar del header es el botón (click o Enter).
  // Custom o placeholder con la inicial.
  const iconInput = view.querySelector<HTMLInputElement>("#sv-icon")!;
  const paintAvatar = (url: string | null) => {
    if (!isActive()) return;
    avatarEl.innerHTML = avatarHtml(info.name, url);
  };
  avatarEl.classList.add("clickable");
  const pickIcon = () => iconInput.click();
  avatarEl.addEventListener("click", pickIcon);
  avatarEl.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      pickIcon();
    }
  });
  iconInput.addEventListener("change", () => {
    const file = iconInput.files?.[0];
    iconInput.value = "";
    if (!file || !isActive()) return;
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result ?? "");
      api.setIcon(name, url).then(() => {
        paintAvatar(url);
        say("Icono actualizado.", false);
      }).catch((err: unknown) => say(`Icono: ${errMsg(err)}`, true));
    };
    reader.readAsDataURL(file);
  });
  avatarEl.innerHTML = placeholderHtml(info.name);
  api.getIcon(name).then((url) => paintAvatar(url)).catch(() => undefined);
}
