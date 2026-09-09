// Digspawn — biblioteca + wizard + vista server + import (hito 7).

import "./styles.css";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "./api";
import { renderLibrary, unmountLibrary } from "./library";
import { openWizard } from "./wizard";
import { openServer, type ServerTab } from "./server";
import { openImport } from "./import";
import { openSettings } from "./settings";

const view = document.querySelector<HTMLElement>("#view");
const wizardRoot = document.querySelector<HTMLElement>("#wizard-root");
const settingsBtn = document.querySelector<HTMLButtonElement>("#settings-btn");
const banner = document.querySelector<HTMLElement>("#update-banner");
const topUsage = document.querySelector<HTMLElement>("#top-usage");

if (!view || !wizardRoot || !settingsBtn || !banner) {
  throw new Error("Falta el shell base (index.html).");
}

async function showLibrary(): Promise<void> {
  const v = view as HTMLElement;
  // Marca sincrónica: invalida de inmediato los handlers de la vista server
  // aunque el render de biblioteca aún esté cargando.
  v.dataset.mode = "library";
  await renderLibrary(v, {
    onNew: () => openWizard(wizardRoot as HTMLElement, () => void showLibrary()),
    onImport: () => openImport(wizardRoot as HTMLElement, () => void showLibrary()),
    onOpen: (name, tab: ServerTab) => {
      // Cambio de modo antes del await de openServer: cualquier render de
      // biblioteca en vuelo ve mode !== "library" y se aborta.
      v.dataset.mode = "server";
      unmountLibrary();
      void openServer(v, name, () => void showLibrary(), tab);
    },
  });
}

settingsBtn.addEventListener("click", () => {
  (view as HTMLElement).dataset.mode = "settings";
  unmountLibrary();
  void openSettings(view as HTMLElement, () => void showLibrary());
});

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

// Update check una vez por sesión (silent fail: sin banner si no hay red).
// Respeta el setting general.
let updateChecked = false;
async function checkUpdateOnce(): Promise<void> {
  if (updateChecked) return;
  updateChecked = true;
  try {
    const s = await api.getSettings();
    if (!s.check_updates_on_start) return;
  } catch {
    // Sin settings se sigue igual (default: chequear).
  }
  let c;
  try {
    c = await api.checkUpdate();
  } catch {
    return;
  }
  if (!c.checked || !c.available) return;
  const b = banner as HTMLElement;
  b.hidden = false;
  b.innerHTML = `
    <div class="update-banner">
      <span>¡Nueva versión ${esc(c.latest)}!${c.required ? " (recomendada)" : ""} ${esc(c.notes)}</span>
      <button id="update-go" type="button">Descargar</button>
      <button id="update-x" type="button" title="Cerrar">✕</button>
    </div>`;
  b.querySelector("#update-go")?.addEventListener("click", () => void openUrl(c.url));
  b.querySelector("#update-x")?.addEventListener("click", () => {
    b.hidden = true;
  });
}

let booted = false;
function boot(): void {
  if (booted) return;
  booted = true;
  void showLibrary();
  void checkUpdateOnce();
  window.setInterval(() => void pollTopUsage(), 3000);
  void pollTopUsage();
}

// Uso agregado de los servers en el topbar (texto) + total de la PC (hover).
// Best-effort: si falla, se conserva el último valor.
async function pollTopUsage(): Promise<void> {
  if (!topUsage) return;
  let st;
  try {
    st = await api.serverStats();
  } catch {
    return;
  }
  if (st.servers.length === 0) {
    topUsage.textContent = "";
    topUsage.removeAttribute("data-tip");
    return;
  }
  const cpu = st.servers.reduce((a, s) => a + s.cpu_pct, 0);
  const ramGb = st.servers.reduce((a, s) => a + s.ram_mb, 0) / 1024;
  topUsage.textContent = `CPU ${cpu.toFixed(0)}% · RAM ${ramGb.toFixed(1)} GB`;
  topUsage.dataset.tip =
    `Toda tu PC: CPU ${st.host.cpu_pct.toFixed(0)}% · RAM ${(st.host.used_mb / 1024).toFixed(1)} de ${(st.host.total_mb / 1024).toFixed(1)} GB en uso`;
}

window.addEventListener("DOMContentLoaded", boot);
// El script se carga con `defer`: el DOM ya está listo, pero si algún día
// cambia el orden de carga el listener de arriba cubre el arranque.
boot();
