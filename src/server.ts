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

export async function openServer(view: HTMLElement, name: string, onBack: () => void): Promise<void> {
  const servers = await api.listServers().catch(() => [] as ServerInfo[]);
  const info = servers.find((s) => s.name === name);
  if (!info) {
    view.innerHTML = `<p class="error">No existe el server "${esc(name)}".</p><button id="sv-back" type="button">Volver</button>`;
    view.querySelector("#sv-back")?.addEventListener("click", onBack);
    return;
  }

  let state: string = info.state;
  let ramMb: number = info.ram_mb;
  let busy = false;
  let tab: "consola" | "ajustes" | "historial" | "comandos" | "jugadores" | "plugins" = "consola";
  let props: Record<string, string> | null = null;
  let propsError: string | null = null;
  let propsMsg: string | null = null;
  let hostMaxRam = 8192;
  let localIps: string[] | null = null;
  let iconUrl: string | null | undefined = undefined; // undefined = aún no pedido
  let historyLoaded = false;
  let histFiles: import("./api").LogFile[] | null = null;
  let histError: string | null = null;
  let histSel: string | null = null;
  let histLines: string[] = [];
  let histLoading = false;
  const unlistens: UnlistenFn[] = [];
  let pollTimer = 0;

  view.innerHTML = `
    <button id="sv-back" type="button">← Biblioteca</button>
    <div class="sv-head">
      <h2>${esc(info.name)}</h2>
      <span id="sv-state" class="badge"></span>
    </div>
    <p id="sv-stats" class="muted"></p>
    <div class="sv-actions">
      <button id="sv-start" type="button" data-tip="Arranca el server. Si falta el Java que necesita, lo descarga solo la primera vez.">Iniciar</button>
      <button id="sv-stop" type="button" data-tip="Apaga el server avisándole antes, así guarda el mundo.">Frenar</button>
      <button id="sv-restart" type="button" data-tip="Apaga y vuelve a prender. Sirve para aplicar cambios de Ajustes.">Reiniciar</button>
    </div>
    <p id="sv-msg" class="muted"></p>
    <div id="sv-java" class="muted" hidden></div>
    <div class="tabs">
      <button id="tab-consola" type="button" data-tip="Lo que el server está diciendo en vivo, y caja para mandarle comandos.">Consola</button>
      <button id="tab-jugadores" type="button" data-tip="Quién está conectado ahora (se detecta del log).">Jugadores</button>
      <button id="tab-comandos" type="button" data-tip="Atajos para los comandos más usados, sin escribirlos a mano.">Comandos</button>
      ${info.type === "paper" ? `<button id="tab-plugins" type="button" data-tip="Plugins (.jar) del server. Después va a servir también para mods.">Plugins</button>` : ""}
      <button id="tab-ajustes" type="button" data-tip="Configuración del server. Solo se edita frenado; aplica al arrancar.">Ajustes</button>
      <button id="tab-historial" type="button" data-tip="Logs guardados: el último log, rotados viejos y crashlogs.">Historial</button>
    </div>
    <div id="sv-tab-body"></div>`;

  const msgEl = view.querySelector<HTMLElement>("#sv-msg")!;
  const javaEl = view.querySelector<HTMLElement>("#sv-java")!;
  const stateEl = view.querySelector<HTMLElement>("#sv-state")!;
  const statsEl = view.querySelector<HTMLElement>("#sv-stats")!;
  const startBtn = view.querySelector<HTMLButtonElement>("#sv-start")!;
  const stopBtn = view.querySelector<HTMLButtonElement>("#sv-stop")!;
  const restartBtn = view.querySelector<HTMLButtonElement>("#sv-restart")!;
  const tabBody = view.querySelector<HTMLElement>("#sv-tab-body")!;
  const tabConsola = view.querySelector<HTMLButtonElement>("#tab-consola")!;
  const tabAjustes = view.querySelector<HTMLButtonElement>("#tab-ajustes")!;
  const tabHistorial = view.querySelector<HTMLButtonElement>("#tab-historial")!;
  const tabComandos = view.querySelector<HTMLButtonElement>("#tab-comandos")!;
  const tabJugadores = view.querySelector<HTMLButtonElement>("#tab-jugadores")!;
  const tabPlugins = view.querySelector<HTMLButtonElement>("#tab-plugins");
  const isPaper = info.type === "paper";
  const players = new Map<string, number>(); // nombre -> timestamp de join
  const serverVersion: string = info.version;
  let pendingEcho: string[] = [];
  let logQueue: string[] = [];
  let logFlushOn = false;

  const running = () => state === "running" || state === "starting" || state === "stopping";

  function paint(): void {
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
    startBtn.disabled = busy || running();
    stopBtn.disabled = busy || !running();
    restartBtn.disabled = busy || !running();
    tabConsola.classList.toggle("sel", tab === "consola");
    tabAjustes.classList.toggle("sel", tab === "ajustes");
    tabHistorial.classList.toggle("sel", tab === "historial");
    tabComandos.classList.toggle("sel", tab === "comandos");
    tabJugadores.classList.toggle("sel", tab === "jugadores");
    tabPlugins?.classList.toggle("sel", tab === "plugins");
    if (tab === "consola") renderConsola();
    else if (tab === "ajustes") void renderAjustes();
    else if (tab === "historial") void renderHistorial();
    else if (tab === "comandos") renderComandos();
    else if (tab === "jugadores") renderJugadores();
    else void renderPlugins();
  }

  function say(msg: string, isErr: boolean): void {
    msgEl.textContent = msg;
    msgEl.className = isErr ? "error" : "muted";
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
      const stick = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
      const div = document.createElement("div");
      div.textContent = line;
      logEl.appendChild(div);
      while (logEl.children.length > MAX_LINES) logEl.firstChild?.remove();
      if (stick) logEl.scrollTop = logEl.scrollHeight;
    };
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
    const num = (id: string, key: string, extra: string) =>
      `<input id="${id}" type="number" ${extra} value="${esc(p[key] ?? "")}" ${dis ? "disabled" : ""} />`;
    tabBody.innerHTML = `
      ${dis ? `<p class="muted">Frená el server para editar (aplica al arrancar).</p>` : ""}
      <div class="props-grid">
        ${field("Puerto", "Por dónde se conectan tus amigos: TU_IP:puerto. Cambialo solo si el 25565 está ocupado.", num("pp-port", "server-port", `min="1" max="65535"`))}
        ${field("IP del server", "Para selfhost sin complicaciones, usá ZeroTier o Radmin VPN y pegá acá la IP que ellos te dan. Vacío = escucha en todas las interfaces.", `<input id="pp-ip" type="text" placeholder="(vacío = todas)" value="${esc(p["server-ip"] ?? "")}" ${dis ? "disabled" : ""} />`)}
        ${(localIps ?? []).length > 0 ? `<p class="muted">Pasales a tus amigos así — IP:Puerto (con Radmin/ZeroTier, la IP es la que te da la VPN): ${(localIps ?? []).map((ip) => esc(`${ip}:${p["server-port"] ?? "25565"}`)).join(" · ")}</p>` : ""}
        ${sel("pp-online", "Online mode", "En true solo entran cuentas premium (originales). En false entra cualquiera, pero se puede usar cualquier nombre.", p["online-mode"] ?? "true", BOOLS, dis)}
        ${sel("pp-diff", "Dificultad", "Daño de monstruos, hambre y veneno: peaceful, easy, normal o hard.", p["difficulty"] ?? "normal", DIFFICULTIES, dis)}
        ${sel("pp-mode", "Gamemode", "Modo de juego al entrar: survival, creative, adventure o spectator.", p["gamemode"] ?? "survival", GAMEMODES, dis)}
        ${sel("pp-pvp", "PVP", "Si los jugadores pueden hacerse daño entre ellos.", p["pvp"] ?? "true", BOOLS, dis)}
        ${sel("pp-wl", "Whitelist", "En true solo entran los de la lista blanca (se agregan con whitelist add).", p["white-list"] ?? "false", BOOLS, dis)}
        ${field("Max jugadores", "Cuántos pueden estar a la vez. Más jugadores = más RAM usada.", num("pp-maxp", "max-players", `min="1" max="1000"`))}
        ${field("View distance", "Qué tan lejos se ve, en chunks. Más alto se ve mejor pero pide más RAM y CPU.", num("pp-vd", "view-distance", `min="2" max="32"`))}
        ${field("MOTD", "El mensajito bajo el nombre del server en la lista de servidores.", `<input id="pp-motd" type="text" maxlength="200" value="${esc(p["motd"] ?? "")}" ${dis ? "disabled" : ""} />`)}
        ${field("RAM", "Memoria para este server. 2048 MB alcanza para jugar de a varios.", `<strong id="pp-ram-lbl">${ramMb} MB</strong>
          <input id="pp-ram" type="range" min="512" max="${hostMaxRam}" step="256" value="${Math.min(ramMb, hostMaxRam)}" ${dis ? "disabled" : ""} />`)}
      </div>
      <button id="pp-save" type="button" data-tip="Guarda todo. Aplica la próxima vez que arranques." ${dis ? "disabled" : ""}>Guardar</button>
      <div class="icon-row" data-tip="Imagen de la card en la biblioteca. Tiene que ser PNG de hasta 1 MB.">
        <strong>Icono</strong>
        <img id="pp-icon-prev" alt="icono actual" hidden />
        <label> Elegir PNG… <input id="pp-icon" type="file" accept="image/png,.png" hidden /></label>
      </div>
      ${propsMsg ? `<p class="muted">${esc(propsMsg)}</p>` : ""}`;
    const ramInput = tabBody.querySelector<HTMLInputElement>("#pp-ram");
    ramInput?.addEventListener("input", () => {
      const lbl = tabBody.querySelector("#pp-ram-lbl");
      if (lbl && ramInput) lbl.textContent = `${ramInput.value} MB`;
    });
    tabBody.querySelector("#pp-save")?.addEventListener("click", () => void saveProps());
    // Icono actual (se cachea: era un GET+base64 en cada paint) + subida.
    const showIcon = (url: string | null) => {
      const prev = tabBody.querySelector<HTMLImageElement>("#pp-icon-prev");
      if (prev && url) {
        prev.src = url;
        prev.hidden = false;
      }
    };
    if (iconUrl === undefined) {
      api.getIcon(name).then((url) => {
        iconUrl = url;
        showIcon(url);
      }).catch(() => undefined);
    } else {
      showIcon(iconUrl);
    }
    tabBody.querySelector<HTMLInputElement>("#pp-icon")?.addEventListener("change", (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;
      const reader = new FileReader();
      reader.onload = () => {
        const url = String(reader.result ?? "");
        api.setIcon(name, url).then(() => {
          iconUrl = url; // refresca el cache
          const prev = tabBody.querySelector<HTMLImageElement>("#pp-icon-prev");
          if (prev) {
            prev.src = url;
            prev.hidden = false;
          }
          say("Icono actualizado.", false);
        }).catch((err: unknown) => say(`Icono: ${errMsg(err)}`, true));
      };
      reader.readAsDataURL(file);
    });
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
        "max-players": val("pp-maxp"),
        "view-distance": val("pp-vd"),
        motd: val("pp-motd"),
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
  let searchQuery = "";
  let searchHits: import("./api").SearchHit[] = [];
  let searching = false;
  let searchError: string | null = null;
  let installingId: string | null = null;
  let installProgress: import("./api").PluginProgress | null = null;
  let installMsg: string | null = null;

  async function renderPlugins(): Promise<void> {
    if (plugFiles === null && plugError === null) {
      tabBody.innerHTML = `<p class="muted">Cargando plugins…</p>`;
      try {
        plugFiles = await api.listPlugins(name);
      } catch (e) {
        plugError = errMsg(e);
      }
    }
    if (plugError !== null) {
      tabBody.innerHTML = `<p class="error">${esc(plugError)}</p>`;
      return;
    }
    const files = plugFiles ?? [];
    const mcVersion = serverVersion;
    tabBody.innerHTML = `
      <h3>Buscar plugins</h3>
      <form id="plug-search-form" class="row" style="gap:.5rem;margin-bottom:.75rem">
        <input id="plug-q" placeholder="ej: essentials, luckperms…" value="${esc(searchQuery)}" style="flex:1" data-tip="Busca en Modrinth, solo compatibles con tu versión." />
        <button type="submit">Buscar</button>
      </form>
      ${searching ? `<p class="muted">Buscando…</p>` : ""}
      ${searchError ? `<p class="error">${esc(searchError)}</p>` : ""}
      ${searchHits.length > 0 ? `<div class="players">${searchHits
        .map(
          (h) => {
            const compat = h.game_versions.includes(mcVersion);
            return `<div class="card">
              <div class="card-icon">🔌</div>
              <div class="card-body"><strong>${esc(h.title)}</strong>
                <span class="state">por ${esc(h.author)} · ${(h.downloads / 1000).toFixed(0)}k descargas</span>
                <span class="badge">${compat ? `compatible con tu ${esc(mcVersion)}` : "revisá compatibilidad"}</span>
                <span class="muted">${esc(h.description.slice(0, 120))}</span></div>
              <button data-install="${esc(h.project_id)}" type="button" ${installingId ? "disabled" : ""}>${installingId === h.project_id ? "…" : "Instalar"}</button>
            </div>`;
          },
        )
        .join("")}</div>` : ""}
      ${installProgress ? `<p id="plug-prog" class="muted">Bajando ${esc(installProgress.file)}… ${installProgress.pct !== null ? `${installProgress.pct.toFixed(0)}%` : ""}</p><progress max="100" value="${installProgress.pct ?? 0}"></progress>` : ""}
      ${installMsg ? `<p class="muted">${esc(installMsg)}</p>` : ""}
      <h3>Instalados</h3>
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
    tabBody.querySelector("#plug-add")?.addEventListener("click", () => void pickPlugins());
    tabBody.querySelectorAll<HTMLButtonElement>("[data-plug-toggle]").forEach((b) => {
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
    tabBody.querySelectorAll<HTMLButtonElement>("[data-plug-del]").forEach((b) => {
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
    tabBody.querySelector("#plug-search-form")?.addEventListener("submit", (e) => {
      e.preventDefault();
      searchQuery = tabBody.querySelector<HTMLInputElement>("#plug-q")?.value.trim() ?? "";
      void doSearch();
    });
    tabBody.querySelectorAll<HTMLButtonElement>("[data-install]").forEach((b) => {
      b.addEventListener("click", () => void doInstall(b.dataset.install ?? ""));
    });
  }

  async function doSearch(): Promise<void> {
    searching = true;
    searchError = null;
    searchHits = [];
    paint();
    try {
      searchHits = await api.searchPlugins(searchQuery);
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

  // ---- Stats en vivo ----
  async function pollStats(): Promise<void> {
    try {
      const st = await api.serverStats();
      const mine = st.servers.find((s) => s.name === name);
      const mineTxt = mine
        ? ` · este server: ${mine.ram_mb} MB, ${mine.cpu_pct.toFixed(0)}% CPU`
        : "";
      statsEl.textContent =
        `🖥 host: ${st.host.used_mb}/${st.host.total_mb} MB · CPU ${st.host.cpu_pct.toFixed(0)}%${mineTxt}`;
    } catch {
      // Poll best-effort: no rompe la vista.
    }
  }

  // ---- Eventos ----
  unlistens.push(
    await listen<LogLine>("log-line", (ev) => {
      if (ev.payload.server !== name) return;
      trackPlayers(ev.payload.line);
      if (tab === "consola") appendLine(ev.payload.line);
    }),
    await listen<ServerStateEvent>("server-state", (ev) => {
      if (ev.payload.server !== name) return;
      state = ev.payload.state;
      busy = false;
      if (state === "running") {
        say("Corriendo.", false);
        javaEl.hidden = true;
      } else if (state === "crashed") {
        say("El server crasheó. Mirá el final del log.", true);
        histFiles = null; // hay crashlog nuevo para ver en Historial
        histSel = null;
        histLines = [];
      } else if (state === "stopped") {
        say("Parado.", false);
      }
      if (tab === "ajustes") {
        props = null; // recargar (los controles se habilitan/deshabilitan)
        propsError = null;
      }
      paint();
      void pollStats();
    }),
    await listen<RuntimeProgress>("runtime-progress", (ev) => {
      if (ev.payload.server !== name) return;
      javaEl.hidden = false;
      const pct = ev.payload.pct !== null ? ` ${ev.payload.pct.toFixed(0)}%` : "";
      javaEl.textContent = `Bajando Java ${ev.payload.version} portable…${pct} (solo la primera vez)`;
    }),
    await listen<import("./api").PluginProgress>("plugin-progress", (ev) => {
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

  startBtn.addEventListener("click", () => void startWithPreflight());
  stopBtn.addEventListener("click", () => void run(() => api.stopServer(name), "Frenando…"));
  restartBtn.addEventListener("click", () => void run(() => api.restartServer(name), "Reiniciando…"));

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

  view.querySelector("#sv-back")?.addEventListener("click", () => {
    unlistens.forEach((u) => u());
    window.clearInterval(pollTimer);
    onBack();
  });

  // Drag & drop de .jar (solo actúa en la pestaña Plugins).
  try {
    const dropUnlisten = await getCurrentWebview().onDragDropEvent((ev) => {
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
}
