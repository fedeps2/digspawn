// Biblioteca: grid de cards desde disco + borrar con confirmación.

import { api, errMsg, type ServerInfo } from "./api";

function badge(s: ServerInfo): string {
  const label = s.type === "paper" ? "Paper" : "Vanilla";
  return `${label} ${s.version}`;
}

export async function renderLibrary(view: HTMLElement, onNew: () => void): Promise<void> {
  view.innerHTML = `<p class="muted">Cargando servers…</p>`;
  let servers: ServerInfo[];
  try {
    servers = await api.listServers();
  } catch (e) {
    view.innerHTML = `<p class="error">No se pudo leer la biblioteca: ${errMsg(e)}</p>`;
    return;
  }

  if (servers.length === 0) {
    view.innerHTML = `
      <div class="empty">
        <p>No tenés ningún server todavía.</p>
        <button id="empty-new" type="button">Nuevo server</button>
      </div>`;
    view.querySelector("#empty-new")?.addEventListener("click", onNew);
    return;
  }

  const cards = servers
    .map(
      (s) => `
      <div class="card" data-name="${escapeHtml(s.name)}">
        <div class="card-icon">${s.type === "paper" ? "📄" : "🧱"}</div>
        <div class="card-body">
          <strong>${escapeHtml(s.name)}</strong>
          <span class="badge">${escapeHtml(badge(s))}</span>
          <span class="state">parado · ${s.ram_mb} MB</span>
        </div>
        <button class="card-del" data-del="${escapeHtml(s.name)}" type="button" title="Borrar">✕</button>
      </div>`,
    )
    .join("");
  view.innerHTML = `<div class="grid">${cards}</div>`;

  view.querySelectorAll<HTMLButtonElement>("[data-del]").forEach((btn) => {
    btn.addEventListener("click", async (ev) => {
      ev.stopPropagation();
      const name = btn.dataset.del ?? "";
      if (!window.confirm(`¿Borrar el server "${name}"? Se elimina su carpeta completa.`)) return;
      try {
        await api.deleteServer(name);
      } catch (e) {
        window.alert(`No se pudo borrar: ${errMsg(e)}`);
        return;
      }
      await renderLibrary(view, onNew);
    });
  });
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
