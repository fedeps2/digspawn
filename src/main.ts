// Digspawn — biblioteca + wizard (hito 2).

import "./styles.css";
import { renderLibrary } from "./library";
import { openWizard } from "./wizard";

const view = document.querySelector<HTMLElement>("#view");
const wizardRoot = document.querySelector<HTMLElement>("#wizard-root");
const newBtn = document.querySelector<HTMLButtonElement>("#new-server-btn");

if (!view || !wizardRoot || !newBtn) {
  throw new Error("Falta el shell base (index.html).");
}

async function showLibrary(): Promise<void> {
  await renderLibrary(view as HTMLElement, () => {
    openWizard(wizardRoot as HTMLElement, () => void showLibrary());
  });
}

newBtn.addEventListener("click", () => {
  openWizard(wizardRoot as HTMLElement, () => void showLibrary());
});

window.addEventListener("DOMContentLoaded", () => void showLibrary());
void showLibrary();
