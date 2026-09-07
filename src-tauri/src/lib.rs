// Comandos Tauri del backend (hito 2: biblioteca + creación).
// Start/stop/consola = hito 3+.

pub mod errors;
pub mod java;
pub mod mojang_api;
pub mod paper_api;
pub mod properties;
pub mod server_manager;

use serde::Serialize;
use tauri::AppHandle;

use errors::{Result, ServerError};
use server_manager::{CreateInput, ServerInfo};

#[derive(Debug, Clone, Serialize)]
pub struct VersionItem {
    pub id: String,
    /// Tipo ("release"/"snapshot") en Vanilla; canal en Paper.
    pub kind: String,
    /// Java mínimo informado por Fill (solo Paper, si viene).
    pub min_java: Option<u32>,
}

#[tauri::command]
async fn list_servers(app: AppHandle) -> Result<Vec<ServerInfo>> {
    server_manager::list_servers(&app)
}

#[tauri::command]
async fn list_versions(server_type: String, include_snapshots: bool) -> Result<Vec<VersionItem>> {
    match server_type.to_lowercase().as_str() {
        "paper" => Ok(crate::paper_api::list_versions()
            .await?
            .into_iter()
            .map(|v| VersionItem {
                id: v.id,
                kind: "release".to_string(),
                min_java: v.min_java,
            })
            .collect()),
        "vanilla" => Ok(crate::mojang_api::list_versions(include_snapshots)
            .await?
            .into_iter()
            .map(|v| VersionItem {
                id: v.id,
                kind: v.kind,
                min_java: None,
            })
            .collect()),
        other => Err(ServerError::VersionsFailed(format!("Tipo desconocido: \"{other}\"."))),
    }
}

#[tauri::command]
async fn create_server(app: AppHandle, input: CreateInput) -> Result<ServerInfo> {
    server_manager::create_server(&app, input).await
}

#[tauri::command]
async fn delete_server(app: AppHandle, name: String) -> Result<()> {
    server_manager::delete_server(&app, &name)
}

#[tauri::command]
fn detect_java() -> Result<java::JavaInfo> {
    java::detect_java()
}

#[tauri::command]
fn host_ram_mb() -> Result<u64> {
    server_manager::host_ram_mb()
}

#[tauri::command]
fn required_java(mc_version: String) -> u32 {
    java::required_java(&mc_version)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_servers,
            list_versions,
            create_server,
            delete_server,
            detect_java,
            host_ram_mb,
            required_java,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
