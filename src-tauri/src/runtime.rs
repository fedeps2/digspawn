// Runtime manager — runtimes Java portables (Adoptium Temurin) por versión.
// Un runtime por familia: jdk8 (MC<1.17), jdk17 (1.17–1.20.4), jdk21 (1.20.5+).
//
// Desvío documentado: Adoptium no publica Java 16 (no es LTS), así que
// MC 1.17 usa el runtime 17 (lo corre sin problema).
// Si Adoptium no tuviera una familia vieja, se buscaría otra fuente (pendiente).

use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

use crate::errors::{Result, ServerError};

/// Feature de Temurin que corresponde al java requerido.
pub fn runtime_feature_for(required_java: u32) -> u32 {
    match required_java {
        0..=8 => 8,
        9..=17 => 17,
        18..=21 => 21,
        // 22+ (STS o LTS nuevos como el 25): última LTS servida por Adoptium.
        _ => 25,
    }
}

/// Java requerido para (tipo, versión): Fill/Mojang mandan (saben de los
/// versionados nuevos como 26.x); la tabla del SPEC queda de fallback offline.
pub async fn required_java_for(server_type: &str, version: &str) -> u32 {
    match server_type.to_lowercase().as_str() {
        "paper" => {
            if let Ok(vs) = crate::paper_api::list_versions().await {
                if let Some(v) = vs.iter().find(|v| v.id == version) {
                    if let Some(m) = v.min_java {
                        return m;
                    }
                }
            }
        }
        "vanilla" => {
            if let Ok(m) = crate::mojang_api::java_major(version).await {
                return m;
            }
        }
        _ => {}
    }
    crate::java::required_java(version)
}

/// URL del binario Adoptium para la feature, según el OS actual.
/// (En dev-Arch baja linux; en target-Windows, windows. Mismo código.)
pub fn adoptium_url(feature: u32) -> Result<String> {
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "linux" => "linux",
        _ => {
            return Err(ServerError::JavaNotFound(format!(
                "OS no soportado para autoinstall de Java: {}",
                std::env::consts::OS
            )))
        }
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        _ => {
            return Err(ServerError::JavaNotFound(format!(
                "Arquitectura no soportada para autoinstall de Java: {}",
                std::env::consts::ARCH
            )))
        }
    };
    Ok(format!(
        "https://api.adoptium.net/v3/binary/latest/{feature}/ga/{os}/{arch}/jdk/hotspot/normal/eclipse"
    ))
}

pub fn runtime_dir(app: &AppHandle, feature: u32) -> Result<PathBuf> {
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| ServerError::Io(format!("No se pudo resolver el dataDir: {e}")))?;
    Ok(data.join("runtime").join(format!("jdk{feature}")))
}

/// Misma resolución con ruta explícita (para tests y núcleos sin Tauri).
pub fn runtime_dir_at(base: &Path, feature: u32) -> PathBuf {
    base.join("runtime").join(format!("jdk{feature}"))
}

pub fn java_bin_in(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join("bin").join("java.exe")
    } else {
        dir.join("bin").join("java")
    }
}

async fn download_to(
    url: &str,
    dest: &Path,
    on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("digspawn/0.1.0")
        .build()
        .map_err(|e| ServerError::DownloadFailed(format!("Java: no se pudo crear cliente HTTP: {e}")))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| ServerError::DownloadFailed(format!("Java: no se pudo descargar el runtime: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::DownloadFailed(format!(
            "Java: Adoptium respondió {}",
            resp.status()
        )));
    }
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|e| ServerError::DownloadFailed(format!("Java: descarga interrumpida: {e}")))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    Ok(())
}

/// Extrae el ZIP de Temurin para Windows (trae una sola carpeta raíz).
fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ServerError::DownloadFailed(format!("Java: ZIP inválido: {e}")))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| ServerError::DownloadFailed(format!("Java: ZIP inválido: {e}")))?;
        let rel = entry
            .name()
            .split('/')
            .skip(1)
            .collect::<Vec<_>>()
            .join("/");
        if rel.is_empty() {
            continue;
        }
        let out = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut outfile)?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

/// Extrae el tar.gz de Temurin para Linux (una sola carpeta raíz).
/// Adoptium sirve .tar.gz en Linux/macOS y .zip en Windows.
fn extract_tar_gz(tgz_path: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let file = std::fs::File::open(tgz_path)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| ServerError::DownloadFailed(format!("Java: tar.gz inválido: {e}")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| ServerError::DownloadFailed(format!("Java: tar.gz inválido: {e}")))?;
        let rel = entry
            .path()
            .map_err(|e| ServerError::DownloadFailed(format!("Java: tar.gz inválido: {e}")))?
            .components()
            .skip(1)
            .collect::<std::path::PathBuf>();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(&rel);
        entry
            .unpack(&out)
            .map_err(|e| ServerError::DownloadFailed(format!("Java: no se pudo extraer: {e}")))?;
    }
    Ok(())
}

/// Despacha según extensión (Adoptium: .zip en Windows, .tar.gz en Linux).
fn extract_archive(archive_path: &Path, dest: &Path) -> Result<()> {
    let name = archive_path.to_string_lossy().into_owned();
    if name.ends_with(".tar.gz.tmp") || name.ends_with(".tar.gz") {
        extract_tar_gz(archive_path, dest)
    } else {
        extract_zip(archive_path, dest)
    }
}

/// Núcleo testeable: asegura el runtime bajo `base` (sin Tauri).
pub async fn ensure_runtime_at(
    base: &Path,
    required_java: u32,
    on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<PathBuf> {
    let feature = runtime_feature_for(required_java);
    let dir = runtime_dir_at(base, feature);
    let bin = java_bin_in(&dir);
    if bin.is_file() {
        return Ok(bin);
    }
    // Si el sistema ya tiene el java justo, se usa (sin descargar nada).
    if let Ok(sys) = crate::java::detect_java() {
        if sys.version == required_java
            || (required_java == 16 && sys.version == 17)
        {
            return Ok(PathBuf::from(sys.path));
        }
    }
    let url = adoptium_url(feature)?;
    std::fs::create_dir_all(dir.parent().expect("runtime tiene padre"))?;
    // Adoptium: .zip en Windows, .tar.gz en Linux/macOS.
    let tmp = if cfg!(windows) {
        dir.with_extension("zip.tmp")
    } else {
        dir.with_extension("tar.gz.tmp")
    };
    download_to(&url, &tmp, on_progress).await?;
    // La extracción puede tardar segundos: fuera del runtime async.
    let dest = dir.clone();
    let tmp_c = tmp.clone();
    tokio::task::spawn_blocking(move || extract_archive(&tmp_c, &dest))
        .await
        .map_err(|e| ServerError::DownloadFailed(format!("Java: falló la extracción: {e}")))??;
    let _ = std::fs::remove_file(&tmp);
    if !bin.is_file() {
        return Err(ServerError::DownloadFailed(
            "Java: el runtime se descargó pero no apareció bin/java.".to_string(),
        ));
    }
    Ok(bin)
}

/// Asegura el runtime para el java requerido. Devuelve el `bin/java` a usar.
pub async fn ensure_runtime(
    app: &AppHandle,
    required_java: u32,
    on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<PathBuf> {
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| ServerError::Io(format!("No se pudo resolver el dataDir: {e}")))?;
    ensure_runtime_at(&data, required_java, on_progress).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_families() {
        assert_eq!(runtime_feature_for(8), 8);
        assert_eq!(runtime_feature_for(16), 17); // sin 16 en Adoptium
        assert_eq!(runtime_feature_for(17), 17);
        assert_eq!(runtime_feature_for(21), 21);
        assert_eq!(runtime_feature_for(25), 25);
        assert_eq!(runtime_feature_for(26), 25); // STS/nuevos -> última LTS
    }

    #[test]
    fn url_points_at_adoptium() {
        let u = adoptium_url(21).unwrap();
        assert!(u.starts_with("https://api.adoptium.net/v3/binary/latest/21/ga/"));
        assert!(u.ends_with("/jdk/hotspot/normal/eclipse"));
    }

    /// Requiere red. Descarga Temurin 21 de verdad (~190MB JDK).
    #[test]
    #[ignore = "needs-network-heavy"]
    fn live_downloads_temurin_21() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let tmp = std::env::temp_dir().join("digspawn-test-runtime");
        let _ = std::fs::remove_dir_all(&tmp);
        let bin = rt
            .block_on(ensure_runtime_at(&tmp, 21, &|_, _| {}))
            .expect("debe descargar Temurin 21");
        assert!(bin.is_file());
        // El binario tiene que ejecutarse e informar major 21.
        let out = std::process::Command::new(&bin).arg("-version").output().unwrap();
        let raw = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(crate::java::parse_java_major(&raw), Some(21));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
