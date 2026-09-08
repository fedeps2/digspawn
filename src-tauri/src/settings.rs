// Config general de la app (<dataDir>/digspawn.json).
// Base mínima: chequeo de updates al abrir + RAM default del wizard.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::errors::{Result, ServerError};

pub const SETTINGS_FILE: &str = "digspawn.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub check_updates_on_start: bool,
    pub default_ram_mb: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { check_updates_on_start: true, default_ram_mb: 2048 }
    }
}

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf> {
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| ServerError::Io(format!("No se pudo resolver el dataDir: {e}")))?;
    Ok(data.join(SETTINGS_FILE))
}

pub fn get_settings(app: &AppHandle) -> Result<Settings> {
    let path = settings_path(app)?;
    if !path.is_file() {
        return Ok(Settings::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    let mut s: Settings = serde_json::from_str(&raw)
        .map_err(|e| ServerError::Io(format!("Config inválida: {e}")))?;
    // Saneos baratos.
    if s.default_ram_mb < 512 {
        s.default_ram_mb = 512;
    }
    if s.default_ram_mb > 32768 {
        s.default_ram_mb = 32768;
    }
    Ok(s)
}

pub fn set_settings(app: &AppHandle, mut s: Settings) -> Result<Settings> {
    if s.default_ram_mb < 512 {
        return Err(ServerError::InvalidName("La RAM default mínima es 512 MB.".to_string()));
    }
    if s.default_ram_mb > 32768 {
        s.default_ram_mb = 32768;
    }
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&s)?)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let d = Settings::default();
        assert!(d.check_updates_on_start);
        assert_eq!(d.default_ram_mb, 2048);
    }
}
