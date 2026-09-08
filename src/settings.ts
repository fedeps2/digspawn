// Config general de la app (base mínima, se agranda después).

import { api, errMsg } from "./api";

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

export async function openSettings(view: HTMLElement, onBack: () => void): Promise<void> {
  let check = true;
  let ram = 2048;
  let error: string | null = null;
  try {
    const s = await api.getSettings();
    check = s.check_updates_on_start;
    ram = s.default_ram_mb;
  } catch (e) {
    error = errMsg(e);
  }

  view.innerHTML = `
    <button id="st-back" type="button">← Biblioteca</button>
    <h2>General</h2>
    ${error ? `<p class="error">${esc(error)}</p>` : ""}
    <div class="props-grid">
      <label class="check" data-tip="Al abrir, Digspawn se fija si hay versión nueva en GitHub."><input id="st-updates" type="checkbox" ${check ? "checked" : ""} /> Buscar updates al abrir</label>
      <label data-tip="RAM con la que arranca el slider al crear un server.">RAM default (MB)
        <input id="st-ram" type="number" min="512" max="32768" step="256" value="${ram}" /></label>
    </div>
    <button id="st-save" type="button">Guardar</button>
    <p id="st-msg" class="muted"></p>`;

  view.querySelector("#st-back")?.addEventListener("click", onBack);
  view.querySelector("#st-save")?.addEventListener("click", async () => {
    const msg = view.querySelector<HTMLElement>("#st-msg")!;
    try {
      const s = await api.setSettings({
        check_updates_on_start: view.querySelector<HTMLInputElement>("#st-updates")?.checked ?? true,
        default_ram_mb: Number(view.querySelector<HTMLInputElement>("#st-ram")?.value ?? 2048),
      });
      check = s.check_updates_on_start;
      ram = s.default_ram_mb;
      msg.textContent = "Guardado.";
      msg.className = "muted";
    } catch (e) {
      msg.textContent = errMsg(e);
      msg.className = "error";
    }
  });
}
