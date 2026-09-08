// Importar server existente: elige carpeta con server.jar, se copia
// (sin logs) a la biblioteca con sidecar propio. El original intacto.

import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, errMsg, type ServerType, type VersionItem } from "./api";

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

export function openImport(root: HTMLElement, onDone: () => void): void {
  let type: ServerType = "paper";
  let versions: VersionItem[] = [];
  let version = "";
  let loading = false;
  let error: string | null = null;
  let importing = false;
  let ramInit = 2048;
  let eulaInit = false;
  let pathInit = "";
  let nameInit = "";

  function close(): void {
    root.innerHTML = "";
  }

  function render(): void {
    root.innerHTML = `
      <div class="overlay">
        <div class="wizard">
          <div class="wizard-head">
            <h2>Importar server</h2>
            <button id="im-close" type="button" title="Cancelar">✕</button>
          </div>
          <div class="wizard-body">
            <label data-tip="La carpeta que tiene el server.jar adentro.">Carpeta del server
              <div class="row" style="gap:.5rem">
                <input id="im-path" placeholder="/ruta/a/mi-server" style="flex:1" value="${esc(pathInit)}" />
                <button id="im-browse" type="button">Elegir…</button>
              </div>
            </label>
            <label data-tip="Nombre en tu biblioteca (puede ser distinto al de la carpeta).">Nombre
              <input id="im-name" maxlength="64" placeholder="Mi server importado" value="${esc(nameInit)}" />
            </label>
            <div class="type-cards">
              <button class="type-card ${type === "paper" ? "sel" : ""}" data-type="paper" type="button">Paper</button>
              <button class="type-card ${type === "vanilla" ? "sel" : ""}" data-type="vanilla" type="button">Vanilla</button>
            </div>
            <label data-tip="Versión que corre ese jar. Se guarda como dato del server.">Versión
              <select id="im-version" ${loading ? "disabled" : ""}>
                ${versions.map((v) => `<option value="${esc(v.id)}" ${v.id === version ? "selected" : ""}>${esc(v.id)}</option>`).join("")}
              </select>
            </label>
            <label data-tip="Memoria para este server.">RAM (MB)
              <input id="im-ram" type="number" min="512" value="${ramInit}" />
            </label>
            <label class="check" data-tip="Si el server ya trae eula=true no hace falta. Si no, hay que aceptarla."><input id="im-eula" type="checkbox" ${eulaInit ? "checked" : ""} /> Acepto la <a id="im-eula-link" href="#">EULA de Minecraft</a></label>
            <p class="muted">Se copia jar + config + mundos (sin logs). El original queda intacto.</p>
            ${error ? `<p class="error">${esc(error)}</p>` : ""}
          </div>
          <div class="wizard-foot">
            <span></span>
            <button id="im-go" type="button" ${importing ? "disabled" : ""}>${importing ? "Importando…" : "Importar"}</button>
          </div>
        </div>
      </div>`;
    root.querySelector("#im-close")?.addEventListener("click", close);
    root.querySelector("#im-browse")?.addEventListener("click", async () => {
      const sel = await open({ directory: true, multiple: false }).catch(() => null);
      if (typeof sel === "string") {
        const input = root.querySelector<HTMLInputElement>("#im-path");
        if (input) {
          input.value = sel;
          // Sugiere el nombre desde la carpeta.
          const nameInput = root.querySelector<HTMLInputElement>("#im-name");
          if (nameInput && !nameInput.value) {
            const base = sel.replace(/\\/g, "/").split("/").filter(Boolean).pop() ?? "";
            nameInput.value = base;
          }
        }
      }
    });
    root.querySelectorAll<HTMLButtonElement>("[data-type]").forEach((b) => {
      b.addEventListener("click", () => {
        type = (b.dataset.type ?? "paper") as ServerType;
        versions = [];
        version = "";
        render();
        void loadVersions();
      });
    });
    root.querySelector("#im-version")?.addEventListener("change", (e) => {
      version = (e.target as HTMLSelectElement).value;
    });
    root.querySelector("#im-eula-link")?.addEventListener("click", (e) => {
      e.preventDefault();
      void openUrl("https://aka.ms/MinecraftEULA");
    });
    root.querySelector("#im-go")?.addEventListener("click", () => void doImport());
  }

  async function loadVersions(): Promise<void> {
    loading = true;
    render();
    try {
      versions = await api.listVersions(type, false);
      if (versions.length > 0 && !versions.some((v) => v.id === version)) {
        version = versions[0].id;
      }
    } catch (e) {
      error = `No se pudieron cargar versiones: ${errMsg(e)}`;
    }
    loading = false;
    render();
  }

  async function doImport(): Promise<void> {
    const path = root.querySelector<HTMLInputElement>("#im-path")?.value.trim() ?? "";
    const name = root.querySelector<HTMLInputElement>("#im-name")?.value.trim() ?? "";
    const ram = Number(root.querySelector<HTMLInputElement>("#im-ram")?.value ?? ramInit);
    const accept = root.querySelector<HTMLInputElement>("#im-eula")?.checked ?? eulaInit;
    pathInit = path;
    nameInit = name;
    ramInit = ram;
    eulaInit = accept;
    error = null;
    if (!path || !name) {
      error = "Elegí la carpeta y ponle un nombre.";
      render();
      return;
    }
    importing = true;
    render();
    try {
      await api.importServer({ path, name, server_type: type, version, ram_mb: ram, accept_eula: accept });
    } catch (e) {
      importing = false;
      error = errMsg(e);
      render();
      return;
    }
    close();
    onDone();
  }

  render();
  void loadVersions();
}
