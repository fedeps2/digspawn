// Digspawn — biblioteca + wizard + vista server (hito 3).

import "./styles.css";
import { api } from "./api";
import { renderLibrary } from "./library";
import { openWizard } from "./wizard";
import { openServer } from "./server";

const view = document.querySelector<HTMLElement>("#view");
const wizardRoot = document.querySelector<HTMLElement>("#wizard-root");
const newBtn = document.querySelector<HTMLButtonElement>("#new-server-btn");

if (!view || !wizardRoot || !newBtn) {
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

window.addEventListener("DOMContentLoaded", () => void showLibrary());
void showLibrary();
