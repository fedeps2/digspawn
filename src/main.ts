// Digspawn — biblioteca + wizard + vista server + import (hito 7).

import "./styles.css";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { api, errMsg, type UpdateCheck, type UpdateProgress } from "./api";
import { renderLibrary, unmountLibrary } from "./library";
import { openWizard } from "./wizard";
import { openServer, type ServerTab } from "./server";
import { openImport } from "./import";
import { openSettings } from "./settings";
import { installDesktopBehaviors } from "./desktop";

const view = document.querySelector<HTMLElement>("#view");
const wizardRoot = document.querySelector<HTMLElement>("#wizard-root");
const settingsBtn = document.querySelector<HTMLButtonElement>("#settings-btn");
const banner = document.querySelector<HTMLElement>("#update-banner");
const topUsage = document.querySelector<HTMLElement>("#top-usage");

if (!view || !wizardRoot || !settingsBtn || !banner) {
  throw new Error("Falta el shell base (index.html).");
}

// La UI se comporta como app de escritorio, no como página: sin menú
// contextual del navegador, sin zoom con Ctrl+rueda, sin selección accidental.
installDesktopBehaviors();

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
  showUpdateBanner(c);
}

// Banner A+: auto-update con un click (descarga verificada + restart) y
// fallback a descarga manual. Los errores ofrecen rollback si hay backup.
function showUpdateBanner(c: UpdateCheck): void {
  const b = banner as HTMLElement;
  b.hidden = false;
  b.innerHTML = `
    <div class="update-banner">
      <span>¡Nueva versión ${esc(c.latest)}!${c.required ? " (recomendada)" : ""} ${esc(c.notes)}</span>
      <button id="update-auto" type="button">Actualizar y reiniciar</button>
      <button id="update-manual" type="button" title="Bajar el exe nuevo a mano">Descarga manual</button>
      <button id="update-x" type="button" title="Cerrar">✕</button>
    </div>`;
  b.querySelector("#update-manual")?.addEventListener("click", () => void openUrl(c.url));
  b.querySelector("#update-x")?.addEventListener("click", () => {
    b.hidden = true;
  });
  b.querySelector("#update-auto")?.addEventListener("click", () => void runAutoUpdate(c));
}

function fmtMb(n: number): string {
  return (n / 1024 / 1024).toFixed(1);
}

async function runAutoUpdate(c: UpdateCheck): Promise<void> {
  const b = banner as HTMLElement;
  b.innerHTML = `
    <div class="update-banner">
      <span id="update-status">Bajando ${esc(c.latest)}…</span>
      <div class="update-progress"><div id="update-bar"></div></div>
    </div>`;
  const status = b.querySelector<HTMLElement>("#update-status")!;
  const bar = b.querySelector<HTMLElement>("#update-bar")!;
  const unlisten = await listen<UpdateProgress>("update-progress", (ev) => {
    const p = ev.payload;
    if (p.pct != null) {
      bar.style.width = `${Math.min(100, p.pct).toFixed(0)}%`;
      status.textContent = p.total != null
        ? `Bajando ${esc(c.latest)}… ${fmtMb(p.downloaded)} de ${fmtMb(p.total)} MB`
        : `Bajando ${esc(c.latest)}… ${fmtMb(p.downloaded)} MB`;
    } else {
      status.textContent = `Bajando ${esc(c.latest)}… ${fmtMb(p.downloaded)} MB`;
    }
  });
  try {
    await api.downloadUpdate();
    status.textContent = "Verificado. Reiniciando…";
    bar.style.width = "100%";
    await api.applyUpdate();
    // applyUpdate reinicia la app: si llegamos acá, falló el restart.
    throw new Error("No se pudo reiniciar la app: abrila a mano.");
  } catch (e) {
    const msg = errMsg(e);
    let canRollback = false;
    try {
      canRollback = await api.rollbackAvailable();
    } catch {
      canRollback = false;
    }
    b.innerHTML = `
      <div class="update-banner">
        <span class="error">No se pudo actualizar: ${esc(msg)} Tu versión sigue intacta.</span>
        ${canRollback ? `<button id="update-rollback" type="button">Volver a versión anterior</button>` : ""}
        <button id="update-manual" type="button">Descarga manual</button>
        <button id="update-x" type="button" title="Cerrar">✕</button>
      </div>`;
    b.querySelector("#update-manual")?.addEventListener("click", () => void openUrl(c.url));
    b.querySelector("#update-x")?.addEventListener("click", () => {
      b.hidden = true;
    });
    b.querySelector("#update-rollback")?.addEventListener("click", async () => {
      try {
        await api.rollbackUpdate();
      } catch (e2) {
        const s = b.querySelector(".error");
        if (s) s.textContent = `No se pudo volver atrás: ${errMsg(e2)}`;
      }
    });
  } finally {
    unlisten();
  }
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
