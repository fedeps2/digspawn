# Digspawn

Launcher de escritorio para hostear servers de Minecraft (Vanilla o Paper),
pensado para que puedas crear, arrancar y administrar un server **sin tocar
un archivo de texto ni una terminal**. Windows, portable: es un solo `.exe`.

## Para jugar (no necesitás saber nada técnico)

### Qué necesitás

- Windows 10 u 11 de 64 bits con internet (la primera vez).
- Nada más: el **Java lo descarga solo** la primera vez que arrancás un server
  (~200 MB, una sola vez) y no te instala ni te cambia nada del sistema.

### Instalación en 3 pasos

1. Bajá el `Digspawn.exe` de la sección
   [**Releases**](https://github.com/fedeps2/digspawn/releases/latest).
2. Ponelo en una carpeta propia (ej: `Documentos\Digspawn`). **No lo metas
   dentro de `AppData`**: tus servers viven ahí y es mejor no mezclarlos.
3. Doble click. Listo.

> **Si Windows te frena con una pantalla azul que dice "Windows protegió su
> equipo" (SmartScreen):** es porque el programa es nuevo y Microsoft todavía
> no lo conoce. Hacé click en **"Más información"** y después en
> **"Ejecutar de todas formas"**. Tus datos no corren riesgo.

### Tu primer server

1. Botón **"Nuevo server"**: poné nombre → elegí tipo (**Paper** recomendado,
   acepta plugins; **Vanilla** es el original pelado) → versión → RAM (con el
   valor de fábrica alcanza).
2. Tildá **"Acepto la EULA de Minecraft"** (las reglas de Mojang para hostear).
3. Doble click en la tarjeta del server: arranca y ves la consola en vivo.

### Invitar a tus amigos

- Si están en tu misma red o VPN (ej: **Radmin VPN** o **ZeroTier**), pasales
  la IP que muestra la app + el puerto (ej: `26.12.34.56:25565`).
- Si tu server es **no-premium** (`online-mode` en false, en Ajustes), leé el
  aviso que aparece ahí: cualquiera puede entrar con cualquier nombre, así que
  conviene poner plugin de login + whitelist.

### Updates

Cuando haya versión nueva, la app te avisa con un cartel: **"Actualizar y
reiniciar"** la baja verificada y la instala sola. Si algo sale mal, tu
versión sigue intacta y podés **"Volver a la versión anterior"** desde el
cartel de error o desde ⚙ General.

### Tus datos

Tus servers, mundos y config viven en `%APPDATA%\digspawn\` (pegá eso en el
explorador de Windows), **separados del exe**: actualizar o borrar el exe
nunca toca tus mundos. Para desinstalar del todo, borrá el exe y esa carpeta.

## Si algo no anda (troubleshooting)

- **El server no arranca:** abrí la pestaña **Historial** (crashlogs) o mirá
  la **Consola**: ahí dice el motivo en castellano siempre que se puede.
- **"Puerto ocupado":** otro programa (u otro server tuyo) ya usa ese puerto.
  Cambialo en Ajustes → Puerto.
- **La primera vez tarda:** está bajando Java y el jar del server. Dale unos
  minutos con internet.
- **"Frená el server" para todo:** Ajustes, backups y updates se hacen con el
  server parado; la app te lo pide sola.

## Desarrollar

```bash
npm install
npm run tauri dev      # levanta la app en modo dev
cargo build            # build backend
npm run build          # tsc / build frontend
```

Ver `SPEC.md` (fuente de verdad del producto), `AGENTS.md` (reglas de
trabajo) y `RELEASE.md` (cómo publicar una versión).

## Licencia

[![License: PolyForm Noncommercial 1.0.0](https://img.shields.io/badge/License-PolyForm%20Noncommercial%201.0.0-orange.svg)](LICENSE)

**PolyForm Noncommercial 1.0.0** — código abierto en el sentido *source-available*:
podés verlo, usarlo, forkkearlo y modificarlo para uso **no comercial**. Queda
prohibido su uso con fines comerciales. Si te copa, podés apoyarlo con una
donación; el proyecto no se vende.

Ver [`LICENSE`](LICENSE) para los términos completos.
