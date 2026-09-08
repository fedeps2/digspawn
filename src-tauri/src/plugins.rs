// Plugins — base que después se generaliza a Mods (loaders modded).
// Hoy: jars en `plugins/` (Paper/Vanilla-con-plugins). Disable = `.jar.disabled`.

use std::path::Path;

use serde::Serialize;

use crate::errors::{Result, ServerError};
use crate::server_manager;

/// Hasta 50 MB por plugin (sobra para cualquier plugin real).
const MAX_PLUGIN_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub file: String,
    pub enabled: bool,
    pub size: u64,
}

/// Nombre de archivo sano: sin separadores ni escapes, termina en .jar
/// (o .jar.disabled para los apagados).
fn validate_plugin_file(file: &str) -> Result<String> {
    if file.len() > 128 || file.is_empty() {
        return Err(ServerError::InvalidName("Nombre de plugin inválido.".to_string()));
    }
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err(ServerError::InvalidName("Nombre de plugin inválido.".to_string()));
    }
    if !(file.ends_with(".jar") || file.ends_with(".jar.disabled")) {
        return Err(ServerError::InvalidName("El plugin tiene que ser un .jar.".to_string()));
    }
    Ok(file.to_string())
}

fn plugins_dir(server_dir: &Path) -> std::path::PathBuf {
    server_dir.join("plugins")
}

pub fn list_plugins_at(server_dir: &Path) -> Vec<PluginInfo> {
    let dir = plugins_dir(server_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut out = vec![];
    for e in entries.flatten() {
        let fname = e.file_name().to_string_lossy().into_owned();
        let enabled = fname.ends_with(".jar");
        if !enabled && !fname.ends_with(".jar.disabled") {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(PluginInfo { file: fname, enabled, size });
    }
    out.sort_by(|a, b| a.file.to_lowercase().cmp(&b.file.to_lowercase()));
    out
}

/// Copia un .jar externo a plugins/. Devuelve el nombre final.
pub fn import_plugin_at(server_dir: &Path, src_path: &str) -> Result<String> {
    let src = Path::new(src_path);
    if !src.is_file() {
        return Err(ServerError::NotFound("Ese archivo no existe.".to_string()));
    }
    let fname = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    validate_plugin_file(&fname)?;
    let size = src.metadata().map(|m| m.len()).unwrap_or(0);
    if size == 0 || size > MAX_PLUGIN_BYTES {
        return Err(ServerError::InvalidName("Ese .jar no parece un plugin válido.".to_string()));
    }
    // Chequeo barato de formato ZIP (los jars son zips).
    let bytes = std::fs::read(src)?;
    if bytes.len() < 4 || &bytes[..4] != b"PK\x03\x04" {
        return Err(ServerError::InvalidName("Eso no es un .jar válido.".to_string()));
    }
    let dir = plugins_dir(server_dir);
    std::fs::create_dir_all(&dir)?;
    let dst = dir.join(&fname);
    if dst.exists() {
        return Err(ServerError::AlreadyExists(format!("Ya hay un plugin \"{fname}\".")));
    }
    std::fs::copy(src, &dst)?;
    Ok(fname)
}

pub fn delete_plugin_at(server_dir: &Path, file: &str) -> Result<()> {
    let fname = validate_plugin_file(file)?;
    let target = plugins_dir(server_dir).join(&fname);
    if !target.is_file() {
        return Err(ServerError::NotFound(format!("No existe el plugin \"{fname}\".")));
    }
    std::fs::remove_file(&target)?;
    Ok(())
}

/// Prende/apaga renombrando (.jar <-> .jar.disabled). Aplica al reiniciar.
/// Idempotente: si ya está en el estado pedido, devuelve Ok igual.
pub fn set_plugin_enabled_at(server_dir: &Path, file: &str, enabled: bool) -> Result<String> {
    let fname = validate_plugin_file(file)?;
    let dir = plugins_dir(server_dir);
    let base = fname.strip_suffix(".disabled").unwrap_or(&fname).to_string();
    let on = dir.join(&base);
    let off = dir.join(format!("{base}.disabled"));
    if enabled {
        if on.is_file() {
            return Ok(base);
        }
        if !off.is_file() {
            return Err(ServerError::NotFound(format!("No existe el plugin \"{fname}\".")));
        }
        std::fs::rename(&off, &on)?;
        Ok(base)
    } else {
        if off.is_file() {
            return Ok(off.file_name().expect("nombre").to_string_lossy().into_owned());
        }
        if !on.is_file() {
            return Err(ServerError::NotFound(format!("No existe el plugin \"{fname}\".")));
        }
        std::fs::rename(&on, &off)?;
        Ok(off.file_name().expect("nombre").to_string_lossy().into_owned())
    }
}

/// Carpeta del server (compartido con modrinth.rs: no duplicar).
pub(crate) fn server_dir_of(
    app: &tauri::AppHandle,
    name: &str,
) -> Result<std::path::PathBuf> {
    use tauri::Manager;
    let clean = server_manager::validate_name(name)?;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| ServerError::Io(format!("No se pudo resolver el dataDir: {e}")))?;
    Ok(data.join("servers").join(clean))
}

pub fn list_plugins(app: &tauri::AppHandle, name: &str) -> Result<Vec<PluginInfo>> {
    Ok(list_plugins_at(&server_dir_of(app, name)?))
}

pub fn import_plugin(app: &tauri::AppHandle, name: &str, path: &str) -> Result<String> {
    import_plugin_at(&server_dir_of(app, name)?, path)
}

pub fn delete_plugin(app: &tauri::AppHandle, name: &str, file: &str) -> Result<()> {
    delete_plugin_at(&server_dir_of(app, name)?, file)
}

pub fn set_plugin_enabled(
    app: &tauri::AppHandle,
    name: &str,
    file: &str,
    enabled: bool,
) -> Result<String> {
    set_plugin_enabled_at(&server_dir_of(app, name)?, file, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Dir único por test: los tests corren en paralelo y no pueden
    // compartir fixtures en /tmp.
    fn setup(tag: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!("digspawn-test-plugins-{tag}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    fn fake_jar(path: &Path) {
        let mut bytes = b"PK\x03\x04".to_vec();
        bytes.extend_from_slice(&[0u8; 100]);
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn import_list_toggle_delete_roundtrip() {
        let tmp = setup("roundtrip");
        assert!(list_plugins_at(&tmp).is_empty());

        let src = tmp.join("fuente");
        std::fs::create_dir_all(&src).unwrap();
        fake_jar(&src.join("EssentialsX-2.21.jar"));

        // No-jar se rechaza.
        std::fs::write(src.join("nota.txt"), b"hola").unwrap();
        assert!(import_plugin_at(&tmp, &src.join("nota.txt").to_string_lossy()).is_err());

        let fname = import_plugin_at(&tmp, &src.join("EssentialsX-2.21.jar").to_string_lossy()).unwrap();
        assert_eq!(fname, "EssentialsX-2.21.jar");
        // Duplicado se rechaza.
        assert!(import_plugin_at(&tmp, &src.join("EssentialsX-2.21.jar").to_string_lossy()).is_err());

        let list = list_plugins_at(&tmp);
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);

        // Apagar -> .disabled, prender -> .jar.
        let off = set_plugin_enabled_at(&tmp, "EssentialsX-2.21.jar", false).unwrap();
        assert_eq!(off, "EssentialsX-2.21.jar.disabled");
        let list = list_plugins_at(&tmp);
        assert!(!list[0].enabled);
        let on = set_plugin_enabled_at(&tmp, &off, true).unwrap();
        assert_eq!(on, "EssentialsX-2.21.jar");

        delete_plugin_at(&tmp, "EssentialsX-2.21.jar").unwrap();
        assert!(list_plugins_at(&tmp).is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rejects_nasty_names() {
        let tmp = setup("nasty");
        assert!(validate_plugin_file("../evil.jar").is_err());
        assert!(validate_plugin_file("a/b.jar").is_err());
        assert!(validate_plugin_file("x.txt").is_err());
        assert!(delete_plugin_at(&tmp, "../evil.jar").is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
