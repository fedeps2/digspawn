// Vista de server (hito 3): header con estado + Start/Stop/Restart,
// consola en vivo con input (stdin) + historial + progreso de Java.

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

export async function openServer(view: HTMLElement, name: string, onBack: () => void): Promise<void> {
  const servers = await api.listServers().catch(() => [] as ServerInfo[]);
  const info = servers.find((s) => s.name === name);
  if (!info) {
    view.innerHTML = `<p class="error">No existe el server "${esc(name)}".</p><button id="sv-back" type="button">Volver</button>`;
    view.querySelector("#sv-back")?.addEventListener("click", onBack);
    return;
  }

  let state: string = info.state;
  let busy = false;
  let javaNote: string | null = null;
  const unlistens: UnlistenFn[] = [];

  view.innerHTML = `
    <button id="sv-back" type="button">← Biblioteca</button>
    <div class="sv-head">
      <h2>${esc(info.name)}</h2>
      <span id="sv-state" class="badge"></span>
    </div>
    <div class="sv-actions">
      <button id="sv-start" type="button">Iniciar</button>
      <button id="sv-stop" type="button">Frenar</button>
      <button id="sv-restart" type="button">Reiniciar</button>
    </div>
    <p id="sv-msg" class="muted"></p>
    <div id="sv-java" class="muted" hidden></div>
    <div id="sv-log" class="console"></div>
    <form id="sv-form" class="row">
      <input id="sv-input" placeholder="Comando… (Enter envía)" autocomplete="off" />
      <button type="submit">Enviar</button>
    </form>`;

  const logEl = view.querySelector<HTMLElement>("#sv-log")!;
  const msgEl = view.querySelector<HTMLElement>("#sv-msg")!;
  const javaEl = view.querySelector<HTMLElement>("#sv-java")!;
  const stateEl = view.querySelector<HTMLElement>("#sv-state")!;
  const startBtn = view.querySelector<HTMLButtonElement>("#sv-start")!;
  const stopBtn = view.querySelector<HTMLButtonElement>("#sv-stop")!;
  const restartBtn = view.querySelector<HTMLButtonElement>("#sv-restart")!;
  const inputEl = view.querySelector<HTMLInputElement>("#sv-input")!;

  function paint(): void {
    const label =
      state === "running" ? "🟢 corriendo" :
      state === "starting" ? "🟡 arrancando…" :
      state === "stopping" ? "🟡 frenando…" :
      state === "crashed" ? "🔴 crasheó" : "⚪ parado";
    stateEl.textContent = label;
    const running = state === "running" || state === "starting" || state === "stopping";
    startBtn.disabled = busy || running;
    stopBtn.disabled = busy || !running;
    restartBtn.disabled = busy || !running;
    inputEl.disabled = state !== "running";
  }

  function append(line: string): void {
    const stick = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
    const div = document.createElement("div");
    div.textContent = line;
    logEl.appendChild(div);
    while (logEl.children.length > MAX_LINES) logEl.firstChild?.remove();
    if (stick) logEl.scrollTop = logEl.scrollHeight;
  }

  function say(msg: string, isErr: boolean): void {
    msgEl.textContent = msg;
    msgEl.className = isErr ? "error" : "muted";
  }

  // Historial.
  try {
    const hist = await api.readLog(name, 200);
    hist.forEach(append);
    if (hist.length > 0) append("——— fin del historial ———");
  } catch {
    // Sin historial: consola vacía, no es error.
  }

  unlistens.push(
    await listen<LogLine>("log-line", (ev) => {
      if (ev.payload.server === name) append(ev.payload.line);
    }),
    await listen<ServerStateEvent>("server-state", (ev) => {
      if (ev.payload.server !== name) return;
      state = ev.payload.state;
      busy = false;
      if (state === "running") {
        say("Corriendo.", false);
        javaEl.hidden = true;
        javaNote = null;
      } else if (state === "crashed") {
        say("El server crasheó. Mirá el final del log.", true);
      } else if (state === "stopped") {
        say("Parado.", false);
      }
      paint();
    }),
    await listen<RuntimeProgress>("runtime-progress", (ev) => {
      if (ev.payload.server !== name) return;
      javaEl.hidden = false;
      const pct = ev.payload.pct !== null ? ` ${ev.payload.pct.toFixed(0)}%` : "";
      javaNote = `Bajando Java ${ev.payload.version} portable…${pct}`;
      javaEl.textContent = javaNote + " (solo la primera vez)";
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

  startBtn.addEventListener("click", () => void run(() => api.startServer(name), "Arrancando…"));
  stopBtn.addEventListener("click", () => void run(() => api.stopServer(name), "Frenando…"));
  restartBtn.addEventListener("click", () => void run(() => api.restartServer(name), "Reiniciando…"));

  view.querySelector("#sv-back")?.addEventListener("click", () => {
    unlistens.forEach((u) => u());
    onBack();
  });

  view.querySelector("#sv-form")?.addEventListener("submit", (e) => {
    e.preventDefault();
    const cmd = inputEl.value.trim();
    if (!cmd || state !== "running") return;
    inputEl.value = "";
    append(`> ${cmd}`);
    api.sendCommand(name, cmd).catch((err: unknown) => say(errMsg(err), true));
  });

  paint();
}
