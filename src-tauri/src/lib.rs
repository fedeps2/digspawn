// Comandos Tauri del backend (hito 3: biblioteca + creación + procesos).
// Comandos rápidos / props editables / update-check = hito 4.

pub mod errors;
pub mod java;
pub mod mojang_api;
pub mod paper_api;
pub mod processes;
pub mod properties;
pub mod runtime;
pub mod server_manager;

use serde::Serialize;
use tauri::{AppHandle, State};

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
async fn list_servers(app: AppHandle, procs: State<'_, processes::ProcessState>) -> Result<Vec<ServerInfo>> {
    let mut servers = server_manager::list_servers(&app)?;
    for s in &mut servers {
        if procs.is_running(&s.name) {
            s.state = processes::STATE_RUNNING.to_string();
        }
    }
    Ok(servers)
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
async fn delete_server(
    app: AppHandle,
    procs: State<'_, processes::ProcessState>,
    name: String,
) -> Result<()> {
    if procs.is_running(name.trim()) {
        return Err(ServerError::AlreadyRunning(
            "Frená el server antes de borrarlo.".to_string(),
        ));
    }
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
async fn required_java(server_type: String, mc_version: String) -> u32 {
    runtime::required_java_for(&server_type, &mc_version).await
}

#[tauri::command]
async fn start_server(app: AppHandle, name: String) -> Result<()> {
    processes::start_server(&app, &name).await
}

#[tauri::command]
async fn stop_server(app: AppHandle, name: String) -> Result<()> {
    processes::stop_server(&app, &name).await
}

#[tauri::command]
async fn restart_server(app: AppHandle, name: String) -> Result<()> {
    processes::restart_server(&app, &name).await
}

#[tauri::command]
async fn send_command(app: AppHandle, name: String, cmd: String) -> Result<()> {
    processes::send_command(&app, &name, &cmd).await
}

#[tauri::command]
fn read_log(app: AppHandle, name: String, max_lines: u32) -> Result<Vec<String>> {
    processes::read_log(&app, &name, max_lines)
}

#[tauri::command]
fn get_properties(app: AppHandle, name: String) -> Result<std::collections::HashMap<String, String>> {
    processes::get_properties(&app, &name)
}

#[tauri::command]
fn set_properties(
    app: AppHandle,
    name: String,
    kvs: std::collections::HashMap<String, String>,
) -> Result<()> {
    processes::set_properties(&app, &name, kvs)
}

#[tauri::command]
fn set_ram(app: AppHandle, name: String, ram_mb: u64) -> Result<()> {
    processes::set_ram(&app, &name, ram_mb)
}

#[tauri::command]
fn server_stats(app: AppHandle) -> Result<processes::AllStats> {
    processes::server_stats(&app)
}

#[tauri::command]
fn preflight(app: AppHandle, name: String) -> Result<processes::Preflight> {
    processes::preflight(&app, &name)
}

#[tauri::command]
fn list_log_files(app: AppHandle, name: String) -> Result<Vec<processes::LogFile>> {
    processes::list_log_files(&app, &name)
}

#[tauri::command]
fn read_log_file(app: AppHandle, name: String, file: String, max_lines: u32) -> Result<Vec<String>> {
    processes::read_log_file(&app, &name, &file, max_lines)
}

#[tauri::command]
fn set_icon(app: AppHandle, name: String, data_url: String) -> Result<()> {
    server_manager::set_icon(&app, &name, &data_url)
}

#[tauri::command]
fn get_icon(app: AppHandle, name: String) -> Result<Option<String>> {
    server_manager::get_icon(&app, &name)
}

#[tauri::command]
fn local_ips() -> Vec<String> {
    server_manager::local_ips()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(processes::ProcessState::new())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_servers,
            list_versions,
            create_server,
            delete_server,
            detect_java,
            host_ram_mb,
            required_java,
            start_server,
            stop_server,
            restart_server,
            send_command,
            read_log,
            get_properties,
            set_properties,
            set_ram,
            server_stats,
            preflight,
            list_log_files,
            read_log_file,
            set_icon,
            get_icon,
            local_ips,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
