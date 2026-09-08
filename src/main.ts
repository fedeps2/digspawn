// Digspawn — biblioteca + wizard + vista server + import (hito 7).

import "./styles.css";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "./api";
import { renderLibrary } from "./library";
import { openWizard } from "./wizard";
import { openServer } from "./server";
import { openImport } from "./import";

const view = document.querySelector<HTMLElement>("#view");
const wizardRoot = document.querySelector<HTMLElement>("#wizard-root");
const newBtn = document.querySelector<HTMLButtonElement>("#new-server-btn");
const importBtn = document.querySelector<HTMLButtonElement>("#import-btn");
const banner = document.querySelector<HTMLElement>("#update-banner");

if (!view || !wizardRoot || !newBtn || !importBtn || !banner) {
  throw new Error("Falta el shell base (index.html).");
}

async function showLibrary(): Promise<void> {
  const v = view as HTMLElement;
  await renderLibrary(v, {
    onNew: () => openWizard(wizardRoot as HTMLElement, () => void showLibrary()),
    onOpen: (name) => void openServer(v, name, () => void showLibrary()),
    onQuickStart: (name) => {
      void api.startServer(name).catch(() => undefined).finally(() => {
        void openServer(v, name, () => void showLibrary());
      });
    },
  });
}

newBtn.addEventListener("click", () => {
  openWizard(wizardRoot as HTMLElement, () => void showLibrary());
});

importBtn.addEventListener("click", () => {
  openImport(wizardRoot as HTMLElement, () => void showLibrary());
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
let updateChecked = false;
async function checkUpdateOnce(): Promise<void> {
  if (updateChecked) return;
  updateChecked = true;
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
