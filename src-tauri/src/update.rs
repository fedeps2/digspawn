// Update A+ (SPEC §Bundling & Updates): auto-descarga del exe nuevo desde
// GitHub Releases + reemplazo + restart, manteniendo el portable.
//
// Garantías:
// - Los datos NUNCA se tocan: el swap solo opera sobre el exe. Si el exe
//   estuviera dentro del dataDir, se aborta.
// - Descarga a `<exe>.new.tmp` + verificación SHA256 antes de reemplazar nada.
// - Backup `<exe>.old` + rollback a la versión anterior.
// - Bloqueado con servers corriendo y en modo dev (no pisar target/debug).
// - Sin red o sin hash publicado: error claro, la app actual sigue andando.

use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::errors::{Result, ServerError};

/// Raw del meta-file (repo + rama + archivo fijo).
pub const META_URL: &str = "https://raw.githubusercontent.com/fedeps2/digspawn/main/version.json";

#[derive(Debug, Deserialize)]
struct MetaFile {
    latest_public: String,
    url: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    required: bool,
    /// Hex del SHA256 del asset (flat, solo Windows x86_64 por ahora).
    /// Vacío = release sin verificación → no se baja solo.
    #[serde(default)]
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: String,
    pub url: String,
    pub notes: String,
    pub required: bool,
    pub sha256: String,
    pub available: bool,
    /// false = no se pudo chequear (sin red, etc). La UI no muestra nada.
    pub checked: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadReport {
    pub staged: bool,
    pub total: Option<u64>,
}

/// Compara "1.0.1" vs "1.0.0" por segmentos numéricos.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let pa: Vec<u64> = a.split('.').map(|s| leading_num(s)).collect();
    let pb: Vec<u64> = b.split('.').map(|s| leading_num(s)).collect();
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            ord => return ord,
        }
    }
    Ordering::Equal
}

fn leading_num(s: &str) -> u64 {
    s.chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// Hex minúsculo del SHA256 de un buffer (testeable sin red).
pub fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn sha_matches(computed: &str, expected: &str) -> bool {
    computed.trim().eq_ignore_ascii_case(expected.trim())
}

/// Rutas del swap: (exe actual, staged `<exe>.new.tmp`, backup `<exe>.old`).
/// Todo en la carpeta del exe (mismo filesystem → rename atómico).
fn exe_paths() -> Result<(PathBuf, PathBuf, PathBuf)> {
    let exe = std::env::current_exe()
        .map_err(|e| ServerError::Io(format!("No se pudo ubicar el ejecutable: {e}")))?;
    let parent: PathBuf = exe
        .parent()
        .ok_or_else(|| ServerError::Io("El ejecutable no tiene carpeta padre.".to_string()))?
        .to_path_buf();
    let name: String = exe
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| ServerError::Io("Nombre de ejecutable inválido.".to_string()))?
        .to_string();
    Ok((
        exe,
        parent.join(format!("{name}.new.tmp")),
        parent.join(format!("{name}.old")),
    ))
}

/// Guards comunes antes de tocar el exe. Con `need_staged`, exige además que
/// la descarga verificada ya esté en disco.
fn preflight(app: &AppHandle, need_staged: bool) -> Result<(PathBuf, PathBuf, PathBuf)> {
    // Nunca pisar el binario de desarrollo (`target/debug/digspawn`).
    if cfg!(debug_assertions) {
        return Err(ServerError::Busy(
            "Los updates solo se aplican en la app instalada (esto es modo desarrollo).".to_string(),
        ));
    }
    let (exe, staged, backup) = exe_paths()?;
    // Los datos viven en el dataDir y el exe en otro lado: si algún día el exe
    // corriera desde dentro del dataDir, abortar antes de tocar nada.
    if let Ok(data) = app.path().app_data_dir() {
        if exe.starts_with(&data) {
            return Err(ServerError::Io(
                "El ejecutable está dentro de la carpeta de datos: movelo fuera para actualizar.".to_string(),
            ));
        }
    }
    // Un restart a mitad de save puede corromper un mundo: exigir todo parado.
    if !app.state::<crate::processes::ProcessState>().running_names().is_empty() {
        return Err(ServerError::AlreadyRunning(
            "Frená los servers antes de actualizar Digspawn.".to_string(),
        ));
    }
    // Permiso de escritura (sondeo en la carpeta, sin tocar el exe: en Windows
    // un exe en ejecución no se puede abrir para escritura).
    let probe = exe
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("digspawn.write.probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
        }
        Err(_) => {
            return Err(ServerError::Io(
                "Sin permiso de escritura en la carpeta de la app: bajá el exe nuevo a mano.".to_string(),
            ));
        }
    }
    if need_staged && !staged.is_file() {
        return Err(ServerError::NotFound(
            "La descarga no está lista: reintentá la actualización.".to_string(),
        ));
    }
    Ok((exe, staged, backup))
}

async fn fetch_meta() -> Result<MetaFile> {
    let client = reqwest::Client::builder()
        .user_agent(format!("digspawn/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| ServerError::VersionsFailed(format!("update: {e}")))?;
    let resp = client
        .get(META_URL)
        .send()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("update: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::VersionsFailed(format!("update: http {}", resp.status())));
    }
    resp.json()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("update: meta inválido: {e}")))
}

pub async fn check_update() -> UpdateCheck {
    let current = env!("CARGO_PKG_VERSION").to_string();
    match fetch_meta().await {
        Ok(meta) => {
            let available = compare_versions(&meta.latest_public, &current) == Ordering::Greater;
            UpdateCheck {
                current,
                latest: meta.latest_public,
                url: meta.url,
                notes: meta.notes,
                required: meta.required,
                sha256: meta.sha256,
                available,
                checked: true,
            }
        }
        Err(_) => UpdateCheck {
            current,
            latest: String::new(),
            url: String::new(),
            notes: String::new(),
            required: false,
            sha256: String::new(),
            available: false,
            checked: false,
        },
    }
}

/// Baja el asset a `<exe>.new.tmp` con progreso (`update-progress`) y verifica
/// el SHA256 publicado. No toca el exe en ejecución.
pub async fn download_update(app: &AppHandle) -> Result<DownloadReport> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let meta = fetch_meta().await?;
    if compare_versions(&meta.latest_public, &current) != Ordering::Greater {
        return Err(ServerError::Busy("Ya estás en la última versión.".to_string()));
    }
    if meta.sha256.trim().is_empty() {
        return Err(ServerError::VersionsFailed(
            "El release no trae hash de verificación (sha256 vacío en version.json).".to_string(),
        ));
    }
    let (_exe, staged, _backup) = exe_paths()?;
    // La descarga verifica el hash igual en cualquier OS (sirve para probar en
    // Arch); el que está limitado a Windows es el reemplazo (apply_update).
    let client = reqwest::Client::builder()
        .user_agent(format!("digspawn/{current}"))
        .build()
        .map_err(|e| ServerError::DownloadFailed(format!("update: {e}")))?;
    let resp = client
        .get(&meta.url)
        .send()
        .await
        .map_err(|e| ServerError::DownloadFailed(format!("No se pudo bajar la actualización: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::DownloadFailed(format!(
            "Descarga falló: http {}",
            resp.status()
        )));
    }
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(&staged).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|e| ServerError::DownloadFailed(format!("Descarga interrumpida: {e}")))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        let pct = total.filter(|t| *t > 0).map(|t| downloaded as f64 / t as f64 * 100.0);
        let _ = app.emit("update-progress", UpdateProgress { downloaded, total, pct });
    }
    drop(file);
    let computed = format!("{:x}", hasher.finalize());
    if !sha_matches(&computed, &meta.sha256) {
        let _ = std::fs::remove_file(&staged);
        return Err(ServerError::DownloadFailed(
            "El archivo bajado no coincide con el hash publicado: se descartó, tu versión sigue intacta.".to_string(),
        ));
    }
    Ok(DownloadReport { staged: true, total })
}

/// Reemplaza el exe (actual → `.old`, staged → definitivo) y reinicia la app.
/// Solo Windows x86_64 por ahora (decisión de producto).
pub fn apply_update(app: &AppHandle) -> Result<()> {
    if std::env::consts::OS != "windows" {
        return Err(ServerError::Busy(
            "El auto-update solo aplica en el exe de Windows por ahora.".to_string(),
        ));
    }
    let (exe, staged, backup) = preflight(app, true)?;
    // Un solo nivel de backup: la "versión anterior" es la inmediata anterior.
    if backup.is_file() {
        std::fs::remove_file(&backup)?;
    }
    // En Windows renombrar el exe en ejecución está permitido (lo prohibido es
    // sobreescribirlo/borrarlo): por eso el swap es rename, nunca write.
    std::fs::rename(&exe, &backup)
        .map_err(|e| ServerError::Io(format!("No se pudo resguardar la versión actual: {e}")))?;
    if let Err(e) = std::fs::rename(&staged, &exe) {
        let _ = std::fs::rename(&backup, &exe);
        return Err(ServerError::Io(format!(
            "No se pudo colocar la versión nueva (se restauró la anterior): {e}"
        )));
    }
    tauri::process::restart(&app.env());
}

/// ¿Hay backup `.old` para volver atrás? (para mostrar el botón de rollback).
pub fn rollback_available() -> bool {
    exe_paths().map(|(_, _, backup)| backup.is_file()).unwrap_or(false)
}

/// Vuelve al `.old` (actual → staged, backup → definitivo) y reinicia.
pub fn rollback_update(app: &AppHandle) -> Result<()> {
    let (exe, staged, backup) = preflight(app, false)?;
    if !backup.is_file() {
        return Err(ServerError::NotFound("No hay versión anterior guardada.".to_string()));
    }
    if staged.is_file() {
        std::fs::remove_file(&staged)?;
    }
    std::fs::rename(&exe, &staged)
        .map_err(|e| ServerError::Io(format!("No se pudo apartar la versión actual: {e}")))?;
    if let Err(e) = std::fs::rename(&backup, &exe) {
        let _ = std::fs::rename(&staged, &exe);
        return Err(ServerError::Io(format!(
            "No se pudo restaurar la versión anterior (se mantuvo la actual): {e}"
        )));
    }
    tauri::process::restart(&app.env());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver() {
        assert_eq!(compare_versions("1.0.1", "1.0.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.9.9", "0.10.0"), Ordering::Less);
        assert_eq!(compare_versions("0.2", "0.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("2.0.0", "10.0.0"), Ordering::Less);
    }

    #[test]
    fn sha256_known_vector() {
        // Vector estándar: sha256("abc").
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha_matches_ignores_case_and_whitespace() {
        assert!(sha_matches("BA7816BF8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        assert!(!sha_matches("00", "ff"));
    }

    #[test]
    fn swap_paths_stay_beside_exe() {
        // Nombres simétricos: el backup y el staged viven junto al exe y no
        // cambian su extensión de forma destructiva.
        let dir = std::env::temp_dir();
        let exe = dir.join("digspawn.exe");
        let name = exe.file_name().unwrap().to_str().unwrap();
        let staged = dir.join(format!("{name}.new.tmp"));
        let backup = dir.join(format!("{name}.old"));
        assert_eq!(staged.file_name().unwrap(), "digspawn.exe.new.tmp");
        assert_eq!(backup.file_name().unwrap(), "digspawn.exe.old");
        assert_eq!(staged.parent(), exe.parent());
    }

    /// Requiere red. El meta-file real debe parsear (aunque no haya update).
    #[test]
    #[ignore = "needs-network"]
    fn live_meta_parses() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let c = rt.block_on(check_update());
        assert!(c.checked, "el meta-file debe estar publicado");
        assert!(!c.current.is_empty());
    }
}
