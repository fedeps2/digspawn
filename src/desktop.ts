// Comportamiento de "app de escritorio" en vez de webview.
//
// Un webview trae hábitos de navegador que delatan que la UI es HTML: el menú
// contextual de Chromium (Atrás / Recargar / Guardar imagen), el zoom con
// Ctrl+rueda, seleccionar texto de labels, arrastrar imágenes fuera de la
// ventana. Acá se apagan. Es un complemento de styles.css, que cubre la parte
// estática (user-select, cursor, scrollbars).

/** Zonas donde el texto es texto de verdad: se copia y se pega. */
const TEXT_ENTRY =
  "input, textarea, [contenteditable], .selectable, .console, #sv-log, pre, code";

function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return target.closest(TEXT_ENTRY) !== null;
}

export function installDesktopBehaviors(): void {
  // 1. Menú contextual del navegador: afuera. Se conserva donde aporta algo
  //    (Copiar/Pegar en campos y en la consola, que es cuando el usuario lo
  //    necesita de verdad). Bloquearlo en todos lados rompería copiar/pegar.
  window.addEventListener(
    "contextmenu",
    (ev) => {
      if (isTextEntry(ev.target)) return;
      ev.preventDefault();
    },
    { capture: true },
  );

  // 2. Zoom del navegador: no existe en una app de escritorio.
  window.addEventListener(
    "wheel",
    (ev) => {
      if (ev.ctrlKey) ev.preventDefault();
    },
    { capture: true, passive: false },
  );
  window.addEventListener(
    "keydown",
    (ev) => {
      if (!ev.ctrlKey && !ev.metaKey) return;
      if (ev.altKey) return;
      const k = ev.key;
      if (k === "+" || k === "-" || k === "=" || k === "_" || k === "0") {
        ev.preventDefault();
      }
    },
    { capture: true },
  );

  // 3. Arrastrar elementos/imágenes fuera de la ventana (el CSS cubre el
  //    user-select; esto cubre el drag nativo del motor).
  window.addEventListener(
    "dragstart",
    (ev) => {
      if (isTextEntry(ev.target)) return;
      ev.preventDefault();
    },
    { capture: true },
  );
}
