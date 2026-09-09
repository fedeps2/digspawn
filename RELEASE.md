# Release flow — auto-update A+ (Windows x86_64)

El auto-update baja el exe de GitHub Releases y verifica su SHA256 contra
`version.json`. Sin hash publicado, la app se niega a bajar sola (solo ofrece
descarga manual).

## Checklist por release

1. **Bump sincronizado** (las 4 tienen que decir lo mismo, ej `0.2.0`):
   - `src-tauri/Cargo.toml` → `version`
   - `src-tauri/tauri.conf.json` → `version`
   - `package.json` → `version`
2. **Buildear el exe Windows** (`x86_64-pc-windows-msvc`, juego posterior
   según AGENTS.md; en esta máquina solo se compila/prueba en Arch).
3. **Nombrar el asset** exactamente así (el updater no adivina nombres):
   `digspawn-x86_64-windows.exe`
4. **Generar el hash**: `sha256sum digspawn-x86_64-windows.exe` → guardar el hex.
5. **Crear el Release en GitHub** (tag `v0.2.0`) con el exe adjunto.
6. **Actualizar `version.json` en `main`** (flat, solo Windows por ahora):
   ```json
   {
     "latest_public": "0.2.0",
     "url": "https://github.com/fedeps2/digspawn/releases/download/v0.2.0/digspawn-x86_64-windows.exe",
     "sha256": "<hex del paso 4>",
     "notes": "Qué cambió, en una línea por cambio",
     "required": false
   }
   ```
   La `url` debe ser **descarga directa** (`/releases/download/...`, no
   `/releases/latest`, que es una página HTML y falla el hash).

## Notas

- `sha256` vacío o ausente = release sin verificación: el banner solo ofrece
  descarga manual. Nunca publicar sin hash.
- El backup `.old` guarda un solo nivel (la inmediata anterior).
- Cuando haya instalable NSIS/MSI, este archivo se reemplaza por el flujo del
  updater oficial (firma + `latest.json`).
