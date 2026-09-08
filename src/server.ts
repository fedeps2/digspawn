// Vista de server (hito 4): header con estado + medidores vivos,
// pestañas Consola (log + stdin) y Ajustes (props + RAM), preflight al iniciar.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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
  let tab: "consola" | "ajustes" = "consola";
  let props: Record<string, string> | null = null;
  let propsError: string | null = null;
  let propsMsg: string | null = null;
  let hostMaxRam = 8192;
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
      <button id="sv-start" type="button">Iniciar</button>
      <button id="sv-stop" type="button">Frenar</button>
      <button id="sv-restart" type="button">Reiniciar</button>
    </div>
    <p id="sv-msg" class="muted"></p>
    <div id="sv-java" class="muted" hidden></div>
    <div class="tabs">
      <button id="tab-consola" type="button">Consola</button>
      <button id="tab-ajustes" type="button">Ajustes</button>
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

  const running = () => state === "running" || state === "starting" || state === "stopping";

  function paint(): void {
    const label =
      state === "running" ? "🟢 corriendo" :
      state === "starting" ? "🟡 arrancando…" :
      state === "stopping" ? "🟡 frenando…" :
      state === "crashed" ? "🔴 crasheó" : "⚪ parado";
    stateEl.textContent = label;
    startBtn.disabled = busy || running();
    stopBtn.disabled = busy || !running();
    restartBtn.disabled = busy || !running();
    tabConsola.classList.toggle("sel", tab === "consola");
    tabAjustes.classList.toggle("sel", tab === "ajustes");
    if (tab === "consola") renderConsola();
    else void renderAjustes();
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
        <input id="sv-input" placeholder="Comando… (Enter envía)" autocomplete="off" />
        <button type="submit">Enviar</button>
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
    (renderConsola as { append?: (l: string) => void }).append?.(line);
  }

  async function loadHistory(): Promise<void> {
    try {
      const hist = await api.readLog(name, 200);
      hist.forEach(appendLine);
      if (hist.length > 0) appendLine("——— fin del historial ———");
    } catch {
      // Sin historial: no es error.
    }
  }

  // ---- Ajustes ----
  function sel(id: string, label: string, val: string, opts: string[], disabled: boolean): string {
    return `<label>${label}<select id="${id}" ${disabled ? "disabled" : ""}>${opts
      .map((o) => `<option value="${o}" ${o === val ? "selected" : ""}>${o}</option>`)
      .join("")}</select></label>`;
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
    }
    if (propsError !== null) {
      tabBody.innerHTML = `<p class="error">${esc(propsError)}</p>`;
      return;
    }
    const p = props ?? {};
    tabBody.innerHTML = `
      ${dis ? `<p class="muted">Frená el server para editar (aplica al arrancar).</p>` : ""}
      <div class="props-grid">
        <label>Puerto<input id="pp-port" type="number" min="1" max="65535" value="${esc(p["server-port"] ?? "25565")}" ${dis ? "disabled" : ""} /></label>
        ${sel("pp-online", "Online mode", p["online-mode"] ?? "true", BOOLS, dis)}
        ${sel("pp-diff", "Dificultad", p["difficulty"] ?? "normal", DIFFICULTIES, dis)}
        ${sel("pp-mode", "Gamemode", p["gamemode"] ?? "survival", GAMEMODES, dis)}
        ${sel("pp-pvp", "PVP", p["pvp"] ?? "true", BOOLS, dis)}
        ${sel("pp-wl", "Whitelist", p["white-list"] ?? "false", BOOLS, dis)}
        <label>Max jugadores<input id="pp-maxp" type="number" min="1" max="1000" value="${esc(p["max-players"] ?? "10")}" ${dis ? "disabled" : ""} /></label>
        <label>View distance<input id="pp-vd" type="number" min="2" max="32" value="${esc(p["view-distance"] ?? "10")}" ${dis ? "disabled" : ""} /></label>
        <label>MOTD<input id="pp-motd" type="text" maxlength="200" value="${esc(p["motd"] ?? "")}" ${dis ? "disabled" : ""} /></label>
        <label>RAM: <strong id="pp-ram-lbl">${ramMb} MB</strong>
          <input id="pp-ram" type="range" min="512" max="${hostMaxRam}" step="256" value="${Math.min(ramMb, hostMaxRam)}" ${dis ? "disabled" : ""} /></label>
      </div>
      <button id="pp-save" type="button" ${dis ? "disabled" : ""}>Guardar</button>
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
      if (ev.payload.server === name && tab === "consola") appendLine(ev.payload.line);
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

  view.querySelector("#sv-back")?.addEventListener("click", () => {
    unlistens.forEach((u) => u());
    window.clearInterval(pollTimer);
    onBack();
  });

  paint();
  pollTimer = window.setInterval(() => void pollStats(), 2000);
  void pollStats();
}
