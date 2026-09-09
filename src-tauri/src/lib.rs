// Comandos Tauri del backend (hito 3: biblioteca + creación + procesos).
// Comandos rápidos / props editables / update-check = hito 4.

pub mod backups;
pub mod diagnose;
pub mod errors;
pub mod java;
pub mod modrinth;
pub mod mojang_api;
pub mod paper_api;
pub mod plugins;
pub mod processes;
pub mod properties;
pub mod runtime;
pub mod server_manager;
pub mod settings;
pub mod tray;
pub mod update;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use errors::{Result, ServerError};
use server_manager::{CreateInput, ImportInput, ServerInfo};

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
    if app.state::<backups::BackupState>().is_busy(name.trim()) {
        return Err(ServerError::Busy(
            "Hay un backup en curso: esperá a que termine para borrar.".to_string(),
        ));
    }
    server_manager::delete_server(&app, &name)
}

#[tauri::command]
async fn rename_server(
    app: AppHandle,
    procs: State<'_, processes::ProcessState>,
    old_name: String,
    new_name: String,
) -> Result<String> {
    if procs.is_running(old_name.trim()) {
        return Err(ServerError::AlreadyRunning(
            "Frená el server antes de renombrarlo.".to_string(),
        ));
    }
    if app.state::<backups::BackupState>().is_busy(old_name.trim()) {
        return Err(ServerError::Busy(
            "Hay un backup en curso: esperá a que termine para renombrar.".to_string(),
        ));
    }
    let final_name = server_manager::rename_server(&app, &old_name, &new_name)?;
    // Los backups acompañan (best-effort).
    if let Ok(data) = app.path().app_data_dir() {
        let src = data.join("backups").join(old_name.trim());
        if src.is_dir() {
            let _ = std::fs::rename(src, data.join("backups").join(&final_name));
        }
    }
    Ok(final_name)
}

#[tauri::command]
fn server_dir_path(app: AppHandle, name: String) -> Result<String> {
    server_manager::server_dir_path(&app, &name)
}

#[tauri::command]
fn detect_java() -> Result<java::JavaInfo> {
    java::detect_java()
}

#[tauri::command]
fn list_backups(app: AppHandle, name: String) -> Result<Vec<backups::BackupInfo>> {
    backups::list_backups(&app, &name)
}

#[tauri::command]
async fn create_backup(app: AppHandle, name: String, scope: String) -> Result<backups::BackupInfo> {
    backups::create_backup(&app, &name, &scope).await
}

#[tauri::command]
async fn restore_backup(app: AppHandle, name: String, file: String) -> Result<()> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || backups::restore_backup(&app2, &name, &file))
        .await
        .map_err(|e| ServerError::Io(format!("Restore interrumpido: {e}")))??;
    Ok(())
}

#[tauri::command]
fn delete_backup(app: AppHandle, name: String, file: String) -> Result<()> {
    backups::delete_backup(&app, &name, &file)
}

#[tauri::command]
fn is_backing_up(app: AppHandle, name: String) -> bool {
    backups::is_backing_up(&app, &name)
}

#[tauri::command]
fn get_backup_config(app: AppHandle, name: String) -> Result<server_manager::BackupConfig> {
    backups::get_backup_config(&app, &name)
}

#[tauri::command]
fn set_backup_config(
    app: AppHandle,
    name: String,
    cfg: server_manager::BackupConfig,
) -> Result<server_manager::BackupConfig> {
    backups::set_backup_config(&app, &name, cfg)
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
async fn force_stop(app: AppHandle, name: String) -> Result<()> {
    processes::force_stop(&app, &name).await
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
fn diagnose_crash(app: AppHandle, name: String) -> Option<diagnose::Diagnosis> {
    diagnose::diagnose_crash(&app, &name)
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

/// TEMPORAL (diagnóstico del salto a biblioteca): appendea a /tmp/digspawn-debug.log.
/// Se saca cuando se encuentre la causa.
#[tauri::command]
fn debug_log(msg: String) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/digspawn-debug.log")?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    writeln!(f, "[{ts}] {msg}")?;
    Ok(())
}

#[tauri::command]
fn list_plugins(app: AppHandle, name: String) -> Result<Vec<plugins::PluginInfo>> {
    plugins::list_plugins(&app, &name)
}

#[tauri::command]
fn import_plugin(app: AppHandle, name: String, path: String) -> Result<String> {
    plugins::import_plugin(&app, &name, &path)
}

#[tauri::command]
fn delete_plugin(app: AppHandle, name: String, file: String) -> Result<()> {
    plugins::delete_plugin(&app, &name, &file)
}

#[tauri::command]
fn set_plugin_enabled(app: AppHandle, name: String, file: String, enabled: bool) -> Result<String> {
    plugins::set_plugin_enabled(&app, &name, &file, enabled)
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<settings::Settings> {
    settings::get_settings(&app)
}

#[tauri::command]
fn set_settings(app: AppHandle, settings: settings::Settings) -> Result<settings::Settings> {
    settings::set_settings(&app, settings)
}

#[derive(Debug, Clone, Serialize)]
struct PluginProgress {
    server: String,
    file: String,
    downloaded: u64,
    total: Option<u64>,
    pct: Option<f64>,
}

#[tauri::command]
async fn search_plugins(query: String, category: Option<String>) -> Result<Vec<modrinth::SearchHit>> {
    modrinth::search(&query, category.as_deref()).await
}

#[tauri::command]
async fn plugin_details(project_id: String) -> Result<modrinth::ProjectDetails> {
    modrinth::details(&project_id).await
}

#[tauri::command]
async fn install_plugin(
    app: AppHandle,
    server_name: String,
    project_id: String,
) -> Result<modrinth::InstallReport> {
    use tauri::Emitter;
    let clean = server_manager::validate_name(&server_name)?;
    let dir = plugins::server_dir_of(&app, &clean)?;
    let meta: server_manager::ServerMeta = serde_json::from_str(
        &std::fs::read_to_string(dir.join(server_manager::SIDECAR)).map_err(|_| {
            ServerError::NotFound("Falta digspawn.json: no es un server válido.".to_string())
        })?,
    )
    .map_err(|e| ServerError::Io(format!("digspawn.json inválido: {e}")))?;
    let server_ev = clean.clone();
    let app_emit = app.clone();
    let on_progress = move |file: String, downloaded: u64, total: Option<u64>| {
        let pct = total.filter(|t| *t > 0).map(|t| downloaded as f64 / t as f64 * 100.0);
        let _ = app_emit.emit(
            "plugin-progress",
            PluginProgress { server: server_ev.clone(), file, downloaded, total, pct },
        );
    };
    modrinth::install_at(&dir, &project_id, &meta.version, &on_progress).await
}

#[tauri::command]
async fn check_update() -> update::UpdateCheck {
    update::check_update().await
}

#[tauri::command]
async fn download_update(app: AppHandle) -> Result<update::DownloadReport> {
    update::download_update(&app).await
}

#[tauri::command]
fn apply_update(app: AppHandle) -> Result<()> {
    update::apply_update(&app)
}

#[tauri::command]
fn rollback_available() -> bool {
    update::rollback_available()
}

#[tauri::command]
fn rollback_update(app: AppHandle) -> Result<()> {
    update::rollback_update(&app)
}

#[tauri::command]
fn import_server(app: AppHandle, input: ImportInput) -> Result<ServerInfo> {
    server_manager::import_server(&app, input)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(processes::ProcessState::new())
        .manage(backups::BackupState::new())
        // Una sola instancia: si ya corre, muestra la ventana existente
        // (puede estar escondida en el tray) en vez de levantar un segundo
        // backend que pelee por los servers.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // El tray es best-effort: en Linux sin bandeja (ej. GNOME pelado)
            // puede no existir. Si falla, la app sigue andando sin él.
            if let Err(e) = tray::build(app.handle()) {
                eprintln!("tray no disponible: {e}");
            }
            // Programador de auto-backups (corre aunque la ventana esté en tray).
            {
                let app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    backups::auto_backup_loop(app).await;
                });
            }
            // Huérfanos de sesiones viejas (padre muerto sin cleanup): si no
            // se matan, retienen lock del mundo + puerto y todo arranque muere.
            // En background para no frenar la ventana.
            {
                let app = app.handle().clone();
                std::thread::spawn(move || {
                    for n in processes::reap_orphans(&app) {
                        eprintln!("proceso huérfano de \"{n}\" frenado al arrancar");
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Cerrar con servers corriendo esconde a tray (no mata por accidente).
            // Sin nada corriendo, el cierre es normal.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if !window.state::<processes::ProcessState>().running_names().is_empty() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_servers,
            list_versions,
            create_server,
            delete_server,
            rename_server,
            server_dir_path,
            detect_java,
            host_ram_mb,
            required_java,
            start_server,
            stop_server,
            force_stop,
            restart_server,
            send_command,
            read_log,
            diagnose_crash,
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
            check_update,
            download_update,
            apply_update,
            rollback_available,
            rollback_update,
            import_server,
            debug_log,
            list_backups,
            create_backup,
            restore_backup,
            delete_backup,
            is_backing_up,
            get_backup_config,
            set_backup_config,
            list_plugins,
            import_plugin,
            delete_plugin,
            set_plugin_enabled,
            get_settings,
            set_settings,
            search_plugins,
            plugin_details,
            install_plugin,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
