// Biblioteca: grid + sidebar derecha de acciones.
// Click = selecciona · doble click = abre la vista · estados en vivo.
// Borrado seguro: armar (5s) + mantener presionado para confirmar.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, errMsg, type ServerInfo, type ServerStateEvent } from "./api";
import type { ServerTab } from "./server";

function badge(s: ServerInfo): string {
  const label = s.type === "paper" ? "Paper" : "Vanilla";
  return `${label} ${s.version}`;
}

export interface LibraryHooks {
  onNew: () => void;
  onImport: () => void;
  onOpen: (name: string, tab: ServerTab) => void;
}

const HOLD_MS = 1500;
const ARM_MS = 5000;

let liveUnlisten: UnlistenFn | null = null;
const iconCache = new Map<string, string | null>();

export function unmountLibrary(): void {
  liveUnlisten?.();
  liveUnlisten = null;
}

export async function renderLibrary(view: HTMLElement, hooks: LibraryHooks): Promise<void> {
  unmountLibrary();
  view.innerHTML = `<p class="muted">Cargando servers…</p>`;
  let servers: ServerInfo[];
  try {
    servers = await api.listServers();
  } catch (e) {
    view.innerHTML = `<p class="error">No se pudo leer la biblioteca: ${errMsg(e)}</p>`;
    return;
  }

  let selected: string | null = null;
  let armed = false;
  let armLeft = 0;
  let armTimer = 0;
  let holding = false;
  let sidebarMsg: { text: string; err: boolean } | null = null;
  let preflightWarn: string[] | null = null;
  let busy = false;

  const byName = (n: string) => servers.find((s) => s.name === n);

  function render(): void {
    const gridHtml = servers.length === 0
      ? `<div class="empty">
          <p>No tenés ningún server todavía.</p>
          <div class="row" style="gap:.5rem;justify-content:center">
            <button id="empty-new" type="button">Nuevo server</button>
            <button id="empty-import" type="button">Importar</button>
          </div>
        </div>`
      : `<div class="grid">${servers
        .map(
          (s) => `
        <div class="tile ${selected === s.name ? "sel" : ""}" data-open="${escapeHtml(s.name)}" data-tip="Click para seleccionar, doble click para abrir.">
          <div class="tile-icon" data-icon="${escapeHtml(s.name)}">${s.type === "paper" ? "📄" : "🧱"}</div>
          <strong class="tile-name">${escapeHtml(s.name)}</strong>
          <span class="badge">${escapeHtml(badge(s))}</span>
          <span class="tile-meta">${s.ram_mb} MB</span>
          <span class="state"><span class="dot ${dotClass(s.state)}"></span>${stateLabel(s.state)}</span>
        </div>`,
        )
        .join("")}</div>`;
    view.innerHTML = `
      <div class="lib-layout">
        ${gridHtml}
        <aside class="sidebar">${sidebar()}</aside>
      </div>`;
    view.querySelector("#empty-new")?.addEventListener("click", hooks.onNew);
    view.querySelector("#empty-import")?.addEventListener("click", hooks.onImport);
    wireCards();
    wireSidebar();
    void paintIcons();
  }

  function dotClass(st: string): string {
    switch (st) {
      case "running": return "ok";
      case "starting":
      case "stopping": return "wait";
      case "crashed": return "bad";
      default: return "";
    }
  }

  function stateLabel(st: string): string {
    switch (st) {
      case "running": return "corriendo";
      case "starting": return "arrancando";
      case "stopping": return "frenando";
      case "crashed": return "crasheó";
      default: return "parado";
    }
  }

  function sidebar(): string {
    const s = selected ? byName(selected) : undefined;
    if (!s) {
      return `
        <p class="muted">Elegí un server para ver qué podés hacer.</p>
        <button data-act="new" type="button">Nuevo server</button>
        <button data-act="import" type="button">Importar</button>`;
    }
    const st = s.state;
    const active = st === "running" || st === "starting" || st === "stopping";
    const mainBtn = active
      ? `<button data-act="stop" type="button" ${busy || st !== "running" ? "disabled" : ""}>Frenar</button>`
      : `<button data-act="start" type="button" ${busy ? "disabled" : ""}>Iniciar</button>`;
    return `
      <div class="sb-head">
        <strong>${escapeHtml(s.name)}</strong>
        <span class="badge">${escapeHtml(badge(s))}</span>
        <span class="state"><span class="dot ${dotClass(st)}"></span>${stateLabel(st)}</span>
      </div>
      ${mainBtn}
      ${st === "running" ? `<button data-act="restart" type="button" ${busy ? "disabled" : ""}>Reiniciar</button>` : ""}
      <button data-act="config" type="button">Config del server</button>
      ${s.type === "paper" ? `<button data-act="plugins" type="button">Plugins</button>` : ""}
      <button type="button" disabled data-tip="Próximamente: para servidores híbridos con mods.">Mods</button>
      <button data-act="logs" type="button">Logs</button>
      ${preflightWarn ? `<div class="warn-box">${preflightWarn.map((w) => `<p>${escapeHtml(w)}</p>`).join("")}<button data-act="force-start" type="button">Arrancar igual</button></div>` : ""}
      ${sidebarMsg ? `<p class="${sidebarMsg.err ? "error" : "muted"}">${escapeHtml(sidebarMsg.text)}</p>` : ""}
      <div class="danger-zone">
        <button id="sb-delete" type="button" class="${armed ? "armed" : ""}" data-tip="Borra el server y su carpeta completa. Hay que mantener presionado.">
          ${armed ? `Mantené para borrar (${(armLeft / 1000).toFixed(1)}s)` : "Borrar"}
        </button>
        ${armed ? `<div class="hold-bar"><div class="hold-fill" style="width:0%"></div></div>` : ""}
      </div>
      <div class="sb-foot">
        <button data-act="new" type="button">Nuevo</button>
        <button data-act="import" type="button">Importar</button>
      </div>`;
  }

  function wireCards(): void {
    view.querySelectorAll<HTMLElement>("[data-open]").forEach((card) => {
      const name = card.dataset.open ?? "";
      card.addEventListener("click", () => {
        if (selected !== name) {
          selected = name;
          disarm();
          sidebarMsg = null;
          preflightWarn = null;
          render();
        }
      });
      card.addEventListener("dblclick", () => hooks.onOpen(name, "consola"));
    });
  }

  function wireSidebar(): void {
    view.querySelectorAll('[data-act="new"]').forEach((b) =>
      b.addEventListener("click", hooks.onNew),
    );
    view.querySelectorAll('[data-act="import"]').forEach((b) =>
      b.addEventListener("click", hooks.onImport),
    );
    view.querySelector('[data-act="start"]')?.addEventListener("click", () => void doStart(false));
    view.querySelector('[data-act="force-start"]')?.addEventListener("click", () => void doStart(true));
    view.querySelector('[data-act="stop"]')?.addEventListener("click", () => void doStop());
    view.querySelector('[data-act="restart"]')?.addEventListener("click", () => void doRestart());
    view.querySelector('[data-act="config"]')?.addEventListener("click", () => {
      if (selected) hooks.onOpen(selected, "ajustes");
    });
    view.querySelector('[data-act="plugins"]')?.addEventListener("click", () => {
      if (selected) hooks.onOpen(selected, "plugins");
    });
    view.querySelector('[data-act="logs"]')?.addEventListener("click", () => {
      if (selected) hooks.onOpen(selected, "historial");
    });
    const del = view.querySelector<HTMLButtonElement>("#sb-delete");
    del?.addEventListener("click", () => {
      if (!armed) arm();
    });
    del?.addEventListener("pointerdown", () => {
      if (!armed || holding) return;
      holding = true;
      const fill = view.querySelector<HTMLElement>(".hold-fill");
      const t0 = Date.now();
      const tick = () => {
        if (!holding || !armed) return;
        const pct = Math.min(100, ((Date.now() - t0) / HOLD_MS) * 100);
        if (fill) fill.style.width = `${pct}%`;
        if (pct >= 100) {
          holding = false;
          void doDelete();
          return;
        }
        requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    });
    const cancelHold = () => {
      holding = false;
    };
    del?.addEventListener("pointerup", cancelHold);
    del?.addEventListener("pointerleave", cancelHold);
  }

  function arm(): void {
    armed = true;
    armLeft = ARM_MS;
    render();
    const t0 = Date.now();
    window.clearInterval(armTimer);
    armTimer = window.setInterval(() => {
      armLeft = Math.max(0, ARM_MS - (Date.now() - t0));
      const btn = view.querySelector<HTMLButtonElement>("#sb-delete");
      if (btn && armed) btn.textContent = `Mantené para borrar (${(armLeft / 1000).toFixed(1)}s)`;
      if (armLeft <= 0) disarm();
    }, 100);
  }

  function disarm(): void {
    if (!armed) return;
    armed = false;
    holding = false;
    window.clearInterval(armTimer);
    if (view.querySelector("#sb-delete")) render();
  }

  async function paintIcons(): Promise<void> {
    await Promise.all(
      servers.map(async (s) => {
        let url = iconCache.get(s.name);
        if (url === undefined) {
          url = await api.getIcon(s.name).catch(() => null);
          iconCache.set(s.name, url);
        }
        if (!url) return;
        const icon = view.querySelector(`[data-icon="${CSS.escape(s.name)}"]`);
        if (icon) icon.innerHTML = `<img src="${url}" alt="icono" />`;
      }),
    );
  }

  async function doStart(force: boolean): Promise<void> {
    if (!selected || busy) return;
    busy = true;
    sidebarMsg = { text: force ? "Arrancando…" : "Chequeando memoria…", err: false };
    render();
    if (!force) {
      try {
        const pf = await api.preflight(selected);
        if (pf.warnings.length > 0) {
          busy = false;
          preflightWarn = pf.warnings;
          sidebarMsg = null;
          render();
          return;
        }
      } catch (e) {
        busy = false;
        sidebarMsg = { text: errMsg(e), err: true };
        render();
        return;
      }
    }
    try {
      await api.startServer(selected);
      preflightWarn = null;
      sidebarMsg = { text: "Arrancando…", err: false };
    } catch (e) {
      sidebarMsg = { text: errMsg(e), err: true };
    }
    busy = false;
    render();
  }

  async function doStop(): Promise<void> {
    if (!selected || busy) return;
    busy = true;
    sidebarMsg = { text: "Frenando…", err: false };
    render();
    try {
      await api.stopServer(selected);
    } catch (e) {
      sidebarMsg = { text: errMsg(e), err: true };
    }
    busy = false;
    render();
  }

  async function doRestart(): Promise<void> {
    if (!selected || busy) return;
    busy = true;
    sidebarMsg = { text: "Reiniciando…", err: false };
    render();
    try {
      await api.restartServer(selected);
    } catch (e) {
      sidebarMsg = { text: errMsg(e), err: true };
    }
    busy = false;
    render();
  }

  async function doDelete(): Promise<void> {
    if (!selected) return;
    disarm();
    const name = selected;
    try {
      await api.deleteServer(name);
    } catch (e) {
      sidebarMsg = { text: errMsg(e), err: true };
      render();
      return;
    }
    iconCache.delete(name);
    selected = null;
    sidebarMsg = null;
    try {
      servers = await api.listServers();
    } catch (e) {
      view.innerHTML = `<p class="error">No se pudo leer la biblioteca: ${errMsg(e)}</p>`;
      return;
    }
    render();
  }

  // Estados en vivo: actualiza sin perder la selección.
  liveUnlisten = await listen<ServerStateEvent>("server-state", (ev) => {
    const s = servers.find((x) => x.name === ev.payload.server);
    if (!s) return;
    s.state = ev.payload.state;
    if (armed) disarm();
    else render();
  }).catch(() => null);

  render();
}

function escapeHtml(s: string): string {
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
