# Digspawn

Launcher de escritorio para hostear servers de Minecraft, pensado para que el
usuario más no-técnico ("el pana") cree, arranque y administre un server sin
tocar un `.txt` ni una terminal.

Construido con **Tauri v2** (Rust backend + webview nativo) y **vanilla
TypeScript/CSS**. Binario portable, sin instalador.

## Qué hace

- Wizard: elegís tipo (Vanilla / Paper) → versión → arranca solo.
- Descarga e instala el **JDK correcto por versión** de MC automáticamente
  (Adoptium portable, sin tocar tu PATH).
- Arranca el server con las **flags de Aikar** tuneadas de base.
- Consola integrada: logs en vivo, comandos rápidos (`say`, `op`, `give`…),
  historial y crashlogs.
- Múltiples servers a la vez, cada uno con su carpeta y su puerto.

## Desarrollar

```bash
npm install
npm run tauri dev      # levanta la app en modo dev
cargo build            # build backend
npm run build          # tsc / build frontend
```

## Licencia

[![License: PolyForm Noncommercial 1.0.0](https://img.shields.io/badge/License-PolyForm%20Noncommercial%201.0.0-orange.svg)](LICENSE)

**PolyForm Noncommercial 1.0.0** — código abierto en el sentido *source-available*:
podés verlo, usarlo, forkkearlo y modificarlo para uso **no comercial**. Queda
prohibido su uso con fines comerciales. Si te copa, podés apoyarlo con una
donación; el proyecto no se vende.

Ver [`LICENSE`](LICENSE) para los términos completos.
