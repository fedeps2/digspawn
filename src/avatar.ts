// Avatar del server: icon.png custom o placeholder reutilizable
// (círculo con la inicial). Se usa en biblioteca y vista del server.

export function escHtml(s: string): string {
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

/// Placeholder: círculo con la inicial del nombre.
export function placeholderHtml(name: string): string {
  const initial = (name.trim()[0] ?? "?").toUpperCase();
  return `<span class="avatar avatar-ph">${escHtml(initial)}</span>`;
}

/// img si hay icon custom, placeholder si no.
export function avatarHtml(name: string, iconUrl: string | null): string {
  if (iconUrl) return `<img class="avatar" src="${iconUrl}" alt="" />`;
  return placeholderHtml(name);
}
