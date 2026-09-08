// Wizard "Nuevo server" — 5 pasos (SPEC): nombre · tipo · versión · Java · RAM.
// Sin arranque: crear baja el jar, firma eula (con consentimiento), genera
// properties + sidecar, y vuelve a la biblioteca.

import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, errMsg, type DownloadProgress, type JavaInfo, type ServerType, type VersionItem } from "./api";

const EULA_URL = "https://aka.ms/MinecraftEULA";

interface WizardState {
  step: number;
  name: string;
  nameError: string | null;
  type: ServerType;
  versions: VersionItem[];
  versionsLoading: boolean;
  versionsError: string | null;
  version: string;
  includeSnapshots: boolean;
  requiredJava: number | null;
  detectedJava: JavaInfo | null;
  javaNote: string | null;
  ram: number;
  maxRam: number;
  eula: boolean;
  creating: boolean;
  progress: number | null;
  createError: string | null;
}

export function openWizard(root: HTMLElement, onDone: () => void): void {
  const st: WizardState = {
    step: 1,
    name: "",
    nameError: null,
    type: "paper",
    versions: [],
    versionsLoading: false,
    versionsError: null,
    version: "",
    includeSnapshots: false,
    requiredJava: null,
    detectedJava: null,
    javaNote: null,
    ram: 2048,
    maxRam: 8192,
    eula: false,
    creating: false,
    progress: null,
    createError: null,
  };

  function close(): void {
    root.innerHTML = "";
  }

  function render(): void {
    root.innerHTML = `
      <div class="overlay">
        <div class="wizard">
          <div class="wizard-head">
            <h2>Nuevo server (paso ${st.step} de 5)</h2>
            <button id="wz-close" type="button" title="Cancelar">✕</button>
          </div>
          <div class="wizard-body">${body()}</div>
          <div class="wizard-foot">${foot()}</div>
        </div>
      </div>`;
    root.querySelector("#wz-close")?.addEventListener("click", close);
    wire();
  }

  function body(): string {
    switch (st.step) {
      case 1:
        return `
          <label data-tip="El nombre que ves en la biblioteca. También va a ser el nombre de la carpeta.">Nombre del server
            <input id="wz-name" value="${esc(st.name)}" placeholder="Mi server" maxlength="64" />
          </label>
          ${st.nameError ? `<p class="error">${esc(st.nameError)}</p>` : ""}
          <p class="muted">Va a ser el nombre de la carpeta en tu biblioteca.</p>`;
      case 2:
        return `
          <div class="type-cards">
            <button class="type-card ${st.type === "paper" ? "sel" : ""}" data-type="paper" type="button" data-tip="Recomendado: más rápido que el Vanilla y acepta plugins.">
              <strong>Paper</strong><span>Recomendado — corre plugins, mejor performance.</span>
            </button>
            <button class="type-card ${st.type === "vanilla" ? "sel" : ""}" data-type="vanilla" type="button" data-tip="El server tal cual lo hizo Mojang: sin plugins ni mods.">
              <strong>Vanilla</strong><span>Jar oficial de Mojang, ni plugins ni mods.</span>
            </button>
          </div>`;
      case 3:
        if (st.versionsLoading) return `<p class="muted">Cargando versiones…</p>`;
        if (st.versionsError) return `<p class="error">${esc(st.versionsError)}</p><button id="wz-retry" type="button">Reintentar</button>`;
        return `
          <label data-tip="Versión de Minecraft del server. Por default va la última.">Versión
            <select id="wz-version">
              ${st.versions.map((v) => `<option value="${esc(v.id)}" ${v.id === st.version ? "selected" : ""}>${esc(v.id)}${v.kind !== "release" ? ` (${esc(v.kind)})` : ""}</option>`).join("")}
            </select>
          </label>
          ${st.type === "vanilla" ? `
          <label class="check" data-tip="Muestra versiones en desarrollo. Pueden tener bugs: solo si sabés lo que hacés."><input id="wz-snap" type="checkbox" ${st.includeSnapshots ? "checked" : ""} /> Incluir snapshots</label>` : ""}
          <p class="muted">Por default, la última versión.</p>`;
      case 4:
        return `
          <p>Java necesario: <strong>${st.requiredJava !== null ? `Java ${st.requiredJava}` : "…"}</strong> (automático según la versión).</p>
          ${st.detectedJava
            ? `<p>Detectado en tu sistema: <strong>Java ${st.detectedJava.version}</strong> <span class="muted">${esc(st.detectedJava.path)}</span></p>`
            : `<p class="error">${esc(st.javaNote ?? "Buscando Java…")}</p>`}
          <p class="muted">En este hito solo se detecta; el autoinstall portable llega después.</p>`;
      case 5:
        return `
          <label data-tip="Memoria para este server. 2048 MB alcanza para jugar de a varios.">RAM: <strong>${st.ram} MB</strong>
            <input id="wz-ram" type="range" min="512" max="${st.maxRam}" step="256" value="${st.ram}" />
          </label>
          <label class="check" data-tip="Las reglas de Mojang exigen aceptar su licencia para hostear un server."><input id="wz-eula" type="checkbox" ${st.eula ? "checked" : ""} />
            Acepto la <a id="wz-eula-link" href="#">EULA de Minecraft</a></label>
          ${st.creating ? `<p class="muted">Bajando el jar… ${st.progress !== null ? `${st.progress.toFixed(0)}%` : ""}</p><progress max="100" value="${st.progress ?? 0}"></progress>` : ""}
          ${st.createError ? `<p class="error">${esc(st.createError)}</p>` : ""}`;
      default:
        return "";
    }
  }

  function foot(): string {
    const back = st.step > 1 && !st.creating ? `<button id="wz-back" type="button">Atrás</button>` : `<span></span>`;
    if (st.step < 5) return `${back}<button id="wz-next" type="button">Siguiente</button>`;
    const label = st.creating ? "Creando…" : "Crear server";
    return `${back}<button id="wz-create" type="button" data-tip="Baja el jar, firma la eula y genera la config. Después aparece en tu biblioteca." ${st.creating ? "disabled" : ""}>${label}</button>`;
  }

  function wire(): void {
    root.querySelector("#wz-back")?.addEventListener("click", () => {
      st.step -= 1;
      st.createError = null;
      render();
    });
    root.querySelector("#wz-next")?.addEventListener("click", async () => {
      if (st.step === 1) {
        const input = root.querySelector<HTMLInputElement>("#wz-name");
        st.name = input?.value.trim() ?? "";
        if (st.name === "") {
          st.nameError = "Ponle un nombre al server.";
          render();
          return;
        }
        st.nameError = null;
      }
      st.step += 1;
      render();
      if (st.step === 3) await loadVersions();
      if (st.step === 4) await loadJava();
      if (st.step === 5) await loadRam();
    });

    root.querySelectorAll<HTMLButtonElement>("[data-type]").forEach((b) => {
      b.addEventListener("click", () => {
        st.type = (b.dataset.type ?? "paper") as ServerType;
        st.versions = [];
        st.version = "";
        render();
      });
    });
    root.querySelector("#wz-version")?.addEventListener("change", (e) => {
      st.version = (e.target as HTMLSelectElement).value;
      void refreshRequiredJava();
    });
    root.querySelector("#wz-snap")?.addEventListener("change", (e) => {
      st.includeSnapshots = (e.target as HTMLInputElement).checked;
      void loadVersions();
    });
    root.querySelector("#wz-retry")?.addEventListener("click", () => void loadVersions());
    root.querySelector("#wz-ram")?.addEventListener("input", (e) => {
      st.ram = Number((e.target as HTMLInputElement).value);
      const label = root.querySelector("#wz-ram")?.closest("label")?.querySelector("strong");
      if (label) label.textContent = `${st.ram} MB`;
    });
    root.querySelector("#wz-eula")?.addEventListener("change", (e) => {
      st.eula = (e.target as HTMLInputElement).checked;
    });
    root.querySelector("#wz-eula-link")?.addEventListener("click", (e) => {
      e.preventDefault();
      void openUrl(EULA_URL);
    });
    root.querySelector("#wz-create")?.addEventListener("click", () => void create());
  }

  async function loadVersions(): Promise<void> {
    st.versionsLoading = true;
    st.versionsError = null;
    render();
    try {
      st.versions = await api.listVersions(st.type, st.includeSnapshots);
      if (st.versions.length === 0) {
        st.versionsError = "No hay versiones disponibles.";
      } else if (!st.versions.some((v) => v.id === st.version)) {
        st.version = st.versions[0].id; // default: la última
      }
    } catch (e) {
      st.versionsError = `No se pudieron cargar las versiones: ${errMsg(e)}`;
    }
    st.versionsLoading = false;
    render();
    if (st.step === 3 && st.version) void refreshRequiredJava();
  }

  async function loadJava(): Promise<void> {
    try {
      st.detectedJava = await api.detectJava();
      st.javaNote = null;
    } catch (e) {
      st.detectedJava = null;
      st.javaNote = `Java no detectado: ${errMsg(e)}`;
    }
    await refreshRequiredJava();
    render();
  }

  async function refreshRequiredJava(): Promise<void> {
    // Paper informa su Java mínimo en Fill; si no, tabla del SPEC.
    const fromFill = st.versions.find((v) => v.id === st.version)?.min_java ?? null;
    if (fromFill !== null) {
      st.requiredJava = fromFill;
    } else if (st.version) {
      try {
        st.requiredJava = await api.requiredJava(st.type, st.version);
      } catch {
        st.requiredJava = null;
      }
    }
    if (st.step === 4) render();
  }

  async function loadRam(): Promise<void> {
    try {
      const host = await api.hostRamMb();
      st.maxRam = Math.max(512, Math.min(host, 32768));
    } catch {
      st.maxRam = 8192;
    }
    // Default de la config general (solo si el usuario no tocó el slider:
    // al entrar al paso 5 el valor sigue siendo el inicial).
    try {
      const s = await api.getSettings();
      st.ram = Math.min(Math.max(512, s.default_ram_mb), st.maxRam);
    } catch {
      st.ram = Math.min(2048, st.maxRam);
    }
    render();
  }

  async function create(): Promise<void> {
    if (!st.eula) {
      st.createError = "Tenés que aceptar la EULA de Minecraft para crear el server.";
      render();
      return;
    }
    st.creating = true;
    st.progress = null;
    st.createError = null;
    render();

    const server = st.name.trim();
    const unlisten = await listen<DownloadProgress>("download-progress", (ev) => {
      if (ev.payload.server !== server) return;
      st.progress = ev.payload.pct;
      const bar = root.querySelector<HTMLProgressElement>("progress");
      if (bar && ev.payload.pct !== null) bar.value = ev.payload.pct;
      const txt = root.querySelector(".wizard-body .muted");
      if (txt && ev.payload.pct !== null) txt.textContent = `Bajando el jar… ${ev.payload.pct.toFixed(0)}%`;
    });
    try {
      await api.createServer({
        name: server,
        server_type: st.type,
        version: st.version,
        ram_mb: st.ram,
        accept_eula: st.eula,
      });
    } catch (e) {
      st.creating = false;
      st.createError = errMsg(e);
      unlisten();
      render();
      return;
    }
    unlisten();
    close();
    onDone();
  }

  render();
}

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
