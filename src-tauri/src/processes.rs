// ProcessManager — varios servers corriendo a la vez (decisión hito 3;
// el SPEC describía un único "server activo").
// stdin por pipe, stdout/stderr a eventos `log-line`, estados `server-state`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, ChildStdin, ChildStdout};

use crate::errors::{Result, ServerError};
use crate::server_manager::{self, ServerMeta, SIDECAR};

pub const STATE_STARTING: &str = "starting";
pub const STATE_RUNNING: &str = "running";
pub const STATE_STOPPING: &str = "stopping";
pub const STATE_STOPPED: &str = "stopped";
pub const STATE_CRASHED: &str = "crashed";

const STOP_TIMEOUT: Duration = Duration::from_secs(20);
const READY_MARKER: &str = "Done (";

/// Aikar's flags (G1GC) del SPEC — el launcher las mete solo, el pana no las ve.
const AIKAR_FLAGS: &[&str] = &[
    "-XX:+UseG1GC",
    "-XX:+ParallelRefProcEnabled",
    "-XX:MaxGCPauseMillis=200",
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+DisableExplicitGC",
    "-XX:+AlwaysPreTouch",
    "-XX:G1NewSizePercent=30",
    "-XX:G1MaxNewSizePercent=40",
    "-XX:G1HeapRegionSize=8M",
    "-XX:G1ReservePercent=20",
    "-XX:G1HeapWastePercent=5",
    "-XX:G1MixedGCCountTarget=4",
    "-XX:InitiatingHeapOccupancyPercent=15",
    "-XX:G1MixedGCLiveThresholdPercent=90",
    "-XX:G1RSetUpdatingPauseTimePercent=5",
    "-XX:SurvivorRatio=32",
    "-XX:+PerfDisableSharedMem",
    "-XX:MaxTenuringThreshold=1",
];

struct Running {
    child: Arc<tokio::sync::Mutex<Child>>,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pid: u32,
}

/// Estado global (uno por app): qué servers están corriendo.
pub struct ProcessState {
    running: Mutex<HashMap<String, Running>>,
}

impl ProcessState {
    pub fn new() -> Self {
        Self { running: Mutex::new(HashMap::new()) }
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.running.lock().expect("lock").contains_key(name)
    }
}

impl Default for ProcessState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
struct StateEvent {
    server: String,
    state: String,
}

#[derive(Debug, Clone, Serialize)]
struct LogLine {
    server: String,
    line: String,
}

#[derive(Debug, Clone, Serialize)]
struct RuntimeProgress {
    server: String,
    version: u32,
    downloaded: u64,
    total: Option<u64>,
    pct: Option<f64>,
}

fn emit_state(app: &AppHandle, server: &str, state: &str) {
    let _ = app.emit(
        "server-state",
        StateEvent { server: server.to_string(), state: state.to_string() },
    );
}

fn emit_line(app: &AppHandle, server: &str, line: &str) {
    let _ = app.emit(
        "log-line",
        LogLine { server: server.to_string(), line: line.to_string() },
    );
}

fn server_path(app: &AppHandle, name: &str) -> Result<PathBuf> {
    let clean = server_manager::validate_name(name)?;
    Ok(server_manager::servers_dir(app)?.join(clean))
}

fn read_meta(dir: &Path) -> Result<ServerMeta> {
    let raw = std::fs::read_to_string(dir.join(SIDECAR))
        .map_err(|_| ServerError::NotFound("Falta digspawn.json: no es un server válido.".to_string()))?;
    serde_json::from_str(&raw).map_err(|e| ServerError::Io(format!("digspawn.json inválido: {e}")))
}

/// Argumentos de arranque (testeable sin proceso).
/// java_path queda aparte porque lo resuelve el runtime manager.
pub fn launch_args(ram_mb: u64) -> Vec<String> {
    let mut args = vec![
        format!("-Xms{ram_mb}M"),
        format!("-Xmx{ram_mb}M"),
    ];
    args.extend(AIKAR_FLAGS.iter().map(|s| s.to_string()));
    args.push("-jar".to_string());
    args.push("server.jar".to_string());
    args.push("--nogui".to_string());
    args
}

/// Núcleo testeable: spawnea el proceso (sin eventos ni estado global).
pub async fn spawn_process(java: &Path, dir: &Path, ram_mb: u64) -> Result<Child> {
    if !dir.join("server.jar").is_file() {
        return Err(ServerError::NotFound("Falta server.jar en la carpeta del server.".to_string()));
    }
    tokio::process::Command::new(java)
        .args(launch_args(ram_mb))
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ServerError::Io(format!("No se pudo arrancar java: {e}")))
}

async fn pump_stream(app: AppHandle, name: String, stream: ChildStdout, ready_tx: tokio::sync::mpsc::Sender<()>) {
    let mut lines = tokio::io::BufReader::new(stream).lines();
    let mut ready_sent = false;
    while let Ok(Some(line)) = lines.next_line().await {
        if !ready_sent && line.contains(READY_MARKER) {
            ready_sent = true;
            emit_state(&app, &name, STATE_RUNNING);
            let _ = ready_tx.send(()).await;
        }
        emit_line(&app, &name, &line);
    }
}

/// Arranca un server (con autoinstall de Java si falta). Emite starting →
/// running (+ log-line) o crashed.
pub async fn start_server(app: &AppHandle, name: &str) -> Result<()> {
    let clean = server_manager::validate_name(name)?;
    let procs = app.state::<ProcessState>();
    if procs.is_running(&clean) {
        return Err(ServerError::AlreadyRunning(format!("\"{clean}\" ya está corriendo.")));
    }
    let dir = server_path(app, &clean)?;
    let meta = read_meta(&dir)?;
    // Fuente primaria: Fill/Mojang (conocen versionados nuevos como 26.x).
    let required = crate::runtime::required_java_for(&meta.server_type, &meta.version).await;

    // Java (bundled → sistema compatible → descarga Adoptium con progreso).
    let server_ev = clean.clone();
    let app_emit = app.clone();
    let on_progress = move |downloaded: u64, total: Option<u64>| {
        let pct = total.filter(|t| *t > 0).map(|t| downloaded as f64 / t as f64 * 100.0);
        let _ = app_emit.emit(
            "runtime-progress",
            RuntimeProgress {
                server: server_ev.clone(),
                version: required,
                downloaded,
                total,
                pct,
            },
        );
    };
    let java = crate::runtime::ensure_runtime(app, required, &on_progress).await?;

    let mut child = spawn_process(&java, &dir, meta.ram_mb).await?;
    let pid = child.id();
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stdin = child.stdin.take().expect("stdin piped");
    // stderr se le trata como stdout más (MC loguea casi todo por stdout).
    let (ready_tx, mut ready_rx) = tokio::sync::mpsc::channel::<()>(1);

    {
        let mut running = procs.running.lock().expect("lock");
        running.insert(
            clean.clone(),
            Running {
                child: Arc::new(tokio::sync::Mutex::new(child)),
                stdin: Arc::new(tokio::sync::Mutex::new(stdin)),
                pid: pid.unwrap_or(0),
            },
        );
    }
    emit_state(app, &clean, STATE_STARTING);

    // Lectores (no tocan el mapa).
    let (app_o, name_o) = (app.clone(), clean.clone());
    tokio::spawn(pump_stream(app_o, name_o, stdout, ready_tx));
    let (app_e, name_e) = (app.clone(), clean.clone());
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            emit_line(&app_e, &name_e, &line);
        }
    });

    // Vigilante: espera el exit y publica el estado final (solo quien
    // saque la entrada del mapa emite, para no duplicar con stop()).
    let (app_w, name_w) = (app.clone(), clean.clone());
    tokio::spawn(async move {
        // Si nunca llegó el "Done", igual se espera al proceso.
        let _ = tokio::time::timeout(Duration::from_secs(240), ready_rx.recv()).await;
        let procs = app_w.state::<ProcessState>();
        let status = {
            let entry = {
                let running = procs.running.lock().expect("lock");
                running.get(&name_w).map(|r| r.child.clone())
            };
            match entry {
                Some(child) => child.lock().await.wait().await.ok(),
                None => return, // stop() ya lo cosechó y emitió.
            }
        };
        let mut running = procs.running.lock().expect("lock");
        if running.remove(&name_w).is_some() {
            let crashed = !matches!(status.map(|s| s.success()), Some(true));
            emit_state(&app_w, &name_w, if crashed { STATE_CRASHED } else { STATE_STOPPED });
        }
    });
    Ok(())
}

async fn write_stdin(app: &AppHandle, name: &str, payload: &str) -> Result<()> {
    let procs = app.state::<ProcessState>();
    let stdin = {
        let running = procs.running.lock().expect("lock");
        running
            .get(name)
            .map(|r| r.stdin.clone())
            .ok_or_else(|| ServerError::NotRunning(format!("\"{name}\" no está corriendo.")))?
    };
    let mut stdin = stdin.lock().await;
    use tokio::io::AsyncWriteExt;
    stdin
        .write_all(payload.as_bytes())
        .await
        .map_err(|e| ServerError::Io(format!("No se pudo escribir al server: {e}")))?;
    stdin
        .flush()
        .await
        .map_err(|e| ServerError::Io(format!("No se pudo escribir al server: {e}")))?;
    Ok(())
}

/// Frena con `stop` graceful (timeout 20s) y `kill` si cuelga.
pub async fn stop_server(app: &AppHandle, name: &str) -> Result<()> {
    let clean = server_manager::validate_name(name)?;
    let procs = app.state::<ProcessState>();
    if !procs.is_running(&clean) {
        return Err(ServerError::NotRunning(format!("\"{clean}\" no está corriendo.")));
    }
    emit_state(app, &clean, STATE_STOPPING);
    write_stdin(app, &clean, "stop\n").await.ok();
    let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
    loop {
        let exited = {
            let entry = {
                let running = procs.running.lock().expect("lock");
                running.get(&clean).map(|r| r.child.clone())
            };
            match entry {
                None => {
                    // El vigilante ya lo cosechó y emitió el estado final.
                    return Ok(());
                }
                Some(child) => {
                    let mut child = child.lock().await;
                    match child.try_wait() {
                        Ok(Some(_)) => true,
                        Ok(None) => false,
                        Err(_) => true,
                    }
                }
            }
        };
        if exited {
            let mut running = procs.running.lock().expect("lock");
            if running.remove(&clean).is_some() {
                emit_state(app, &clean, STATE_STOPPED);
            }
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            let entry = {
                let running = procs.running.lock().expect("lock");
                running.get(&clean).map(|r| r.child.clone())
            };
            if let Some(child) = entry {
                let _ = child.lock().await.start_kill();
            }
            let mut running = procs.running.lock().expect("lock");
            if running.remove(&clean).is_some() {
                emit_state(app, &clean, STATE_STOPPED);
            }
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

pub async fn restart_server(app: &AppHandle, name: &str) -> Result<()> {
    // Si no estaba corriendo, stop falla con NotRunning: se ignora y se arranca.
    match stop_server(app, name).await {
        Ok(()) | Err(ServerError::NotRunning(_)) => start_server(app, name).await,
        Err(e) => Err(e),
    }
}

/// Envía un comando/linea a stdin (una sola línea).
pub async fn send_command(app: &AppHandle, name: &str, cmd: &str) -> Result<()> {
    let clean = server_manager::validate_name(name)?;
    let line = cmd.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Err(ServerError::InvalidName("Comando vacío.".to_string()));
    }
    write_stdin(app, &clean, &format!("{line}\n")).await
}

/// Cola de `logs/latest.log` (historial al abrir la consola).
pub fn read_log(app: &AppHandle, name: &str, max_lines: u32) -> Result<Vec<String>> {
    let clean = server_manager::validate_name(name)?;
    let log = server_path(app, &clean)?.join("logs").join("latest.log");
    if !log.is_file() {
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&log)?;
    let n = max_lines.clamp(20, 500) as usize;
    let lines: Vec<String> = raw.lines().map(|s| s.to_string()).collect();
    let skip = lines.len().saturating_sub(n);
    Ok(lines.into_iter().skip(skip).collect())
}

pub fn state_of(app: &AppHandle, name: &str) -> String {
    let procs: tauri::State<'_, ProcessState> = app.state::<ProcessState>();
    if procs.is_running(name) {
        STATE_RUNNING.to_string()
    } else {
        STATE_STOPPED.to_string()
    }
}

// ---------------------------------------------------------------------------
// Propiedades (solo con el server parado; aplican al arrancar).
// ---------------------------------------------------------------------------

fn require_stopped(app: &AppHandle, name: &str) -> Result<()> {
    let procs: tauri::State<'_, ProcessState> = app.state::<ProcessState>();
    if procs.is_running(name) {
        return Err(ServerError::AlreadyRunning(
            "Frená el server para editar (aplica al arrancar).".to_string(),
        ));
    }
    Ok(())
}

pub fn get_properties(app: &AppHandle, name: &str) -> Result<std::collections::HashMap<String, String>> {
    let clean = server_manager::validate_name(name)?;
    crate::properties::read_map(&server_path(app, &clean)?.join("server.properties"))
}

pub fn set_properties(
    app: &AppHandle,
    name: &str,
    kvs: std::collections::HashMap<String, String>,
) -> Result<()> {
    let clean = server_manager::validate_name(name)?;
    require_stopped(app, &clean)?;
    crate::properties::apply_changes(&server_path(app, &clean)?.join("server.properties"), &kvs)
}

/// RAM asignada (vive en el sidecar, no en server.properties).
pub fn set_ram(app: &AppHandle, name: &str, ram_mb: u64) -> Result<()> {
    let clean = server_manager::validate_name(name)?;
    require_stopped(app, &clean)?;
    let host = server_manager::host_ram_mb()?;
    if ram_mb < 512 || ram_mb > host {
        return Err(ServerError::InvalidName(format!(
            "RAM entre 512 MB y {host} MB (tu equipo)."
        )));
    }
    let dir = server_path(app, &clean)?;
    let mut meta = read_meta(&dir)?;
    meta.ram_mb = ram_mb;
    std::fs::write(dir.join(SIDECAR), serde_json::to_string_pretty(&meta)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Monitoreo + preflight.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct HostStats {
    pub total_mb: u64,
    pub used_mb: u64,
    pub cpu_pct: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerStat {
    pub name: String,
    pub pid: u32,
    pub ram_mb: u64,
    pub cpu_pct: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllStats {
    pub host: HostStats,
    pub servers: Vec<ServerStat>,
}

fn host_stats(sys: &sysinfo::System) -> HostStats {
    let cpus = sys.cpus();
    let cpu_pct = if cpus.is_empty() {
        0.0
    } else {
        cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpus.len() as f32
    };
    HostStats {
        total_mb: sys.total_memory() / 1024 / 1024,
        used_mb: sys.used_memory() / 1024 / 1024,
        cpu_pct,
    }
}

/// Foto de host + cada server corriendo (por PID).
pub fn server_stats(app: &AppHandle) -> Result<AllStats> {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let host = host_stats(&sys);
    let procs: tauri::State<'_, ProcessState> = app.state::<ProcessState>();
    let snapshot: Vec<(String, u32)> = {
        let running = procs.running.lock().expect("lock");
        running.iter().map(|(n, r)| (n.clone(), r.pid)).collect()
    };
    let mut servers = vec![];
    for (name, pid) in snapshot {
        let (ram_mb, cpu_pct) = sys
            .process(sysinfo::Pid::from_u32(pid))
            .map(|p| (p.memory() / 1024 / 1024, p.cpu_usage()))
            .unwrap_or((0, 0.0));
        servers.push(ServerStat { name, pid, ram_mb, cpu_pct });
    }
    servers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(AllStats { host, servers })
}

#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    pub can_start: bool,
    pub free_mb: u64,
    pub needed_mb: u64,
    pub warnings: Vec<String>,
}

/// Lógica pura del preflight (testeable con números inyectados).
pub fn evaluate_preflight(
    total_mb: u64,
    used_mb: u64,
    assigned_running_mb: u64,
    needed_mb: u64,
) -> Preflight {
    let free_mb = total_mb.saturating_sub(used_mb).saturating_sub(assigned_running_mb);
    let mut warnings = vec![];
    if needed_mb > free_mb {
        let falta = needed_mb - free_mb;
        warnings.push(format!(
            "Este server pide {needed_mb} MB pero quedan ~{free_mb} MB libres (faltan ~{falta} MB). \
            Si lo arrancás igual puede andar a los tirones o crashear. \
            Conviene frenar otro server o bajarle la RAM en Ajustes."
        ));
    }
    Preflight { can_start: true, free_mb, needed_mb, warnings }
}

/// Chequeo antes de arrancar: ¿alcanza la memoria?
/// No bloquea (el aviso con confirmación vive en la UI); solo informa.
pub fn preflight(app: &AppHandle, name: &str) -> Result<Preflight> {
    let clean = server_manager::validate_name(name)?;
    let dir = server_path(app, &clean)?;
    let meta = read_meta(&dir)?;
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    let total_mb = sys.total_memory() / 1024 / 1024;
    let used_mb = sys.used_memory() / 1024 / 1024;
    // RAM ya comprometida por otros servers corriendo (por sidecar).
    let procs: tauri::State<'_, ProcessState> = app.state::<ProcessState>();
    let others: Vec<String> = {
        let running = procs.running.lock().expect("lock");
        running.keys().filter(|n| *n != &clean).cloned().collect()
    };
    let mut assigned_running_mb: u64 = 0;
    let base = server_manager::servers_dir(app)?;
    for other in others {
        let raw = std::fs::read_to_string(base.join(&other).join(SIDECAR));
        if let Ok(raw) = raw {
            if let Ok(m) = serde_json::from_str::<ServerMeta>(&raw) {
                assigned_running_mb += m.ram_mb;
            }
        }
    }
    Ok(evaluate_preflight(total_mb, used_mb, assigned_running_mb, meta.ram_mb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_args_carry_ram_and_aikar() {
        let a = launch_args(2048);
        assert_eq!(a[0], "-Xms2048M");
        assert_eq!(a[1], "-Xmx2048M");
        assert!(a.contains(&"-XX:+UseG1GC".to_string()));
        assert!(a.contains(&"-XX:MaxGCPauseMillis=200".to_string()));
        assert_eq!(a[a.len() - 3..], ["-jar".to_string(), "server.jar".to_string(), "--nogui".to_string()]);
    }

    #[test]
    fn launch_args_min_ram() {
        let a = launch_args(512);
        assert_eq!(a[0], "-Xms512M");
    }

    #[test]
    fn preflight_ok_when_memory_fits() {
        let p = evaluate_preflight(16000, 6000, 2048, 2048);
        assert!(p.can_start);
        assert!(p.warnings.is_empty());
        assert_eq!(p.free_mb, 16000 - 6000 - 2048);
        assert_eq!(p.needed_mb, 2048);
    }

    #[test]
    fn preflight_warns_when_short() {
        let p = evaluate_preflight(8000, 7000, 0, 2048);
        assert!(p.can_start); // informa, no bloquea
        assert_eq!(p.warnings.len(), 1);
        assert!(p.warnings[0].contains("faltan"));
    }

    #[test]
    fn preflight_counts_running_servers() {
        // Otro server corriendo con 4GB asignados deja sin lugar al nuevo.
        let p = evaluate_preflight(8000, 2000, 4096, 2048);
        assert_eq!(p.warnings.len(), 1);
    }

    #[test]
    fn sysinfo_reads_host_and_self() {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();
        let host = host_stats(&sys);
        assert!(host.total_mb >= 512, "total: {}", host.total_mb);
        assert!(host.used_mb <= host.total_mb);
        assert!((0.0..=100.0).contains(&host.cpu_pct), "cpu: {}", host.cpu_pct);
        // El propio proceso de test debe aparecer por PID.
        let me = sys
            .process(sysinfo::Pid::from_u32(std::process::id()))
            .expect("el proceso propio debe existir");
        assert!(me.memory() > 0);
    }

    /// Boot real hasta "Done" + `say` + freno graceful con `stop`.
    /// Núcleo compartido por los smokes de Paper y Vanilla.
    async fn boot_and_stop(java: &Path, dir: &Path) {
        let mut child = spawn_process(java, dir, 2048).await.expect("spawn");
        let stdout = child.stdout.take().unwrap();
        let stdin = child.stdin.take().unwrap();
        let mut lines = tokio::io::BufReader::new(stdout).lines();

        // Esperar "Done (" (primer boot genera mundo: hasta 4 min).
        // Si el proceso muere antes, fallar con su código en vez de esperar.
        let mut done = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(240);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(status)) = child.try_wait() {
                panic!("el proceso murió antes de Done (exit: {status})");
            }
            let line = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap_or_default();
            if line.contains(READY_MARKER) {
                done = true;
                break;
            }
        }
        assert!(done, "el server nunca mostró Done");

        // `say` por stdin y freno graceful.
        {
            use tokio::io::AsyncWriteExt;
            let mut stdin = stdin;
            stdin.write_all(b"say hola pana\n").await.unwrap();
            stdin.write_all(b"stop\n").await.unwrap();
            stdin.flush().await.unwrap();
        }
        let status = tokio::time::timeout(Duration::from_secs(60), child.wait())
            .await
            .expect("el stop debe frenar el server")
            .unwrap();
        assert!(status.success(), "exit limpio tras stop");
    }

    /// Requiere red + Java del sistema. Boot real de Paper hasta "Done",
    /// comando `say`, freno graceful con `stop`. Pesado (~1-3 min).
    #[test]
    #[ignore = "needs-network-heavy"]
    fn live_boot_paper_until_done() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let tmp = std::env::temp_dir().join("digspawn-test-boot");
            let _ = std::fs::remove_dir_all(&tmp);
            let servers = tmp.join("servers");
            std::fs::create_dir_all(&servers).unwrap();

            // Server Paper real (reusa Fill v3).
            let vs = crate::paper_api::list_versions().await.unwrap();
            let info = crate::server_manager::create_server_at(
                &servers,
                crate::server_manager::CreateInput {
                    name: "Boot Test".to_string(),
                    server_type: "paper".to_string(),
                    version: vs[0].id.clone(),
                    ram_mb: 2048,
                    accept_eula: true,
                },
                &|_, _| {},
            )
            .await
            .expect("crear server");
            let dir = servers.join(&info.name);

            // Java del sistema si coincide, si no el portable.
            let required = crate::runtime::required_java_for(&info.server_type, &info.version).await;
            let java = crate::runtime::ensure_runtime_at(&tmp, required, &|_, _| {})
                .await
                .expect("java");

            boot_and_stop(&java, &dir).await;
            let _ = std::fs::remove_dir_all(&tmp);
        });
    }

    /// Smoke de Vanilla: crear + boot hasta "Done" + stop. Pesado (~1-3 min).
    #[test]
    #[ignore = "needs-network-heavy"]
    fn live_boot_vanilla_until_done() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let tmp = std::env::temp_dir().join("digspawn-test-boot-vanilla");
            let _ = std::fs::remove_dir_all(&tmp);
            let servers = tmp.join("servers");
            std::fs::create_dir_all(&servers).unwrap();

            let vs = crate::mojang_api::list_versions(false).await.unwrap();
            let info = crate::server_manager::create_server_at(
                &servers,
                crate::server_manager::CreateInput {
                    name: "Vanilla Test".to_string(),
                    server_type: "vanilla".to_string(),
                    version: vs[0].id.clone(),
                    ram_mb: 2048,
                    accept_eula: true,
                },
                &|_, _| {},
            )
            .await
            .expect("crear server vanilla");
            let dir = servers.join(&info.name);

            let required = crate::runtime::required_java_for(&info.server_type, &info.version).await;
            let java = crate::runtime::ensure_runtime_at(&tmp, required, &|_, _| {})
                .await
                .expect("java");

            boot_and_stop(&java, &dir).await;
            let _ = std::fs::remove_dir_all(&tmp);
        });
    }
}
