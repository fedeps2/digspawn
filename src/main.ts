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

if (!view || !wizardRoot || !settingsBtn || !banner) {
  throw new Error("Falta el shell base (index.html).");
}

async function showLibrary(): Promise<void> {
  const v = view as HTMLElement;
  await renderLibrary(v, {
    onNew: () => openWizard(wizardRoot as HTMLElement, () => void showLibrary()),
    onImport: () => openImport(wizardRoot as HTMLElement, () => void showLibrary()),
    onOpen: (name, tab: ServerTab) => {
      unmountLibrary();
      void openServer(v, name, () => void showLibrary(), tab);
    },
  });
}

settingsBtn.addEventListener("click", () => {
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

window.addEventListener("DOMContentLoaded", () => {
  void showLibrary();
  void checkUpdateOnce();
});
void showLibrary();
void checkUpdateOnce();
