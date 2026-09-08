// ProcessManager — varios servers corriendo a la vez (decisión hito 3;
// el SPEC describía un único "server activo").
// stdin por pipe, stdout/stderr a eventos `log-line`, estados `server-state`.

use std::collections::{HashMap, VecDeque};
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
    /// Últimas líneas del log (para el crashlog). Compartido por ambos pumps.
    ring: Arc<Mutex<VecDeque<String>>>,
}

/// Tope del ring (y de cada crashlog).
pub const RING_CAP: usize = 300;

/// Agrega una línea al ring, manteniendo el tope.
pub fn push_ring(ring: &Mutex<VecDeque<String>>, line: String) {
    let mut ring = ring.lock().expect("lock");
    ring.push_back(line);
    while ring.len() > RING_CAP {
        ring.pop_front();
    }
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

async fn pump_stream(
    app: AppHandle,
    name: String,
    stream: ChildStdout,
    ring: Arc<Mutex<VecDeque<String>>>,
    ready_tx: tokio::sync::mpsc::Sender<()>,
) {
    let mut lines = tokio::io::BufReader::new(stream).lines();
    let mut ready_sent = false;
    while let Ok(Some(line)) = lines.next_line().await {
        if !ready_sent && line.contains(READY_MARKER) {
            ready_sent = true;
            emit_state(&app, &name, STATE_RUNNING);
            let _ = ready_tx.send(()).await;
        }
        push_ring(&ring, line.clone());
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
    let ring: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));

    {
        let mut running = procs.running.lock().expect("lock");
        running.insert(
            clean.clone(),
            Running {
                child: Arc::new(tokio::sync::Mutex::new(child)),
                stdin: Arc::new(tokio::sync::Mutex::new(stdin)),
                pid: pid.unwrap_or(0),
                ring: ring.clone(),
            },
        );
    }
    emit_state(app, &clean, STATE_STARTING);

    // Lectores (no tocan el mapa).
    let (app_o, name_o) = (app.clone(), clean.clone());
    let ring_o = ring.clone();
    tokio::spawn(pump_stream(app_o, name_o, stdout, ring_o, ready_tx));
    let (app_e, name_e) = (app.clone(), clean.clone());
    let ring_e = ring.clone();
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            push_ring(&ring_e, line.clone());
            emit_line(&app_e, &name_e, &line);
        }
    });

    // Vigilante: espera el exit y publica el estado final (solo quien
    // saque la entrada del mapa emite, para no duplicar con stop()).
    // Si el exit es anormal, vuelca el ring a crashlogs/.
    let (app_w, name_w) = (app.clone(), clean.clone());
    tokio::spawn(async move {
        // Si nunca llegó el "Done", igual se espera al proceso.
        let _ = tokio::time::timeout(Duration::from_secs(240), ready_rx.recv()).await;
        let procs = app_w.state::<ProcessState>();
        let (child, ring) = {
            let running = procs.running.lock().expect("lock");
            match running.get(&name_w) {
                Some(r) => (r.child.clone(), r.ring.clone()),
                None => return, // stop() ya lo cosechó y emitió.
            }
        };
        // wait() sin el lock global tomado (stop() puede estar esperando).
        let status = child.lock().await.wait().await.ok();
        let mut running = procs.running.lock().expect("lock");
        if running.remove(&name_w).is_some() {
            drop(running);
            let crashed = !matches!(status.map(|s| s.success()), Some(true));
            if crashed {
                if let Ok(dir) = server_path(&app_w, &name_w) {
                    let lines: Vec<String> = ring.lock().expect("lock").iter().cloned().collect();
                    match dump_crashlog(&dir, &lines) {
                        Ok(path) => emit_line(
                            &app_w,
                            &name_w,
                            &format!("Crashlog guardado en {}", path.display()),
                        ),
                        Err(e) => {
                            emit_line(&app_w, &name_w, &format!("No se pudo guardar crashlog: {e}"))
                        }
                    }
                }
                emit_state(&app_w, &name_w, STATE_CRASHED);
            } else {
                emit_state(&app_w, &name_w, STATE_STOPPED);
            }
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
    tail_lines(&log, max_lines.clamp(20, 2000) as usize)
}

// ---------------------------------------------------------------------------
// Crashlogs + historial de archivos.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LogFile {
    /// Ruta relativa a la carpeta del server ("crashlogs/crash-....log",
    /// "logs/latest.log", "logs/2026-09-01-1.log.gz").
    pub file: String,
    pub kind: String, // "crash" | "latest" | "rotated"
    pub size: u64,
    pub modified: u64, // unix timestamp
}

/// Vuelca las últimas líneas a `crashlogs/crash-<ts>.log`. Devuelve la ruta.
pub fn dump_crashlog(dir: &Path, lines: &[String]) -> Result<PathBuf> {
    let crashdir = dir.join("crashlogs");
    std::fs::create_dir_all(&crashdir)?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = crashdir.join(format!("crash-{ts}.log"));
    let mut out = format!("# Crashlog de Digspawn ({ts})\n# Últimas {} líneas antes de la caída.\n", lines.len());
    for line in lines.iter().take(RING_CAP) {
        out.push_str(line);
        out.push('\n');
    }
    std::fs::write(&path, out)?;
    Ok(path)
}

/// Resuelve un archivo de log pedido por la UI, sin path traversal:
/// debe existir bajo la carpeta del server y ser .log o .log.gz.
pub fn resolve_log_file(dir: &Path, file: &str) -> Result<PathBuf> {
    if file.contains("..") || file.starts_with('/') || file.starts_with('\\') {
        return Err(ServerError::InvalidName("Archivo de log inválido.".to_string()));
    }
    let base = dir
        .canonicalize()
        .map_err(|_| ServerError::NotFound("Carpeta del server inválida.".to_string()))?;
    let target = base.join(file);
    let canon = target
        .canonicalize()
        .map_err(|_| ServerError::NotFound("Archivo de log inexistente.".to_string()))?;
    if !canon.starts_with(&base) {
        return Err(ServerError::InvalidName("Archivo de log inválido.".to_string()));
    }
    let name = canon.to_string_lossy().into_owned();
    if !(name.ends_with(".log") || name.ends_with(".log.gz")) {
        return Err(ServerError::InvalidName("Solo se pueden leer .log y .log.gz.".to_string()));
    }
    Ok(canon)
}

/// Últimas `n` líneas de un .log o .log.gz.
pub fn tail_lines(path: &Path, n: usize) -> Result<Vec<String>> {
    let raw: String = if path.extension().map(|e| e == "gz").unwrap_or(false) {
        let file = std::fs::File::open(path)?;
        let mut gz = flate2::read::GzDecoder::new(file);
        let mut s = String::new();
        std::io::Read::read_to_string(&mut gz, &mut s)
            .map_err(|e| ServerError::Io(format!("No se pudo descomprimir el log: {e}")))?;
        s
    } else {
        std::fs::read_to_string(path)?
    };
    let lines: Vec<String> = raw.lines().map(|s| s.to_string()).collect();
    let skip = lines.len().saturating_sub(n.max(1));
    Ok(lines.into_iter().skip(skip).collect())
}

fn file_meta(path: &Path) -> Option<(u64, u64)> {
    let m = path.metadata().ok()?;
    let modified = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((m.len(), modified))
}

/// Lista latest.log + crashlogs/*.log + logs/*.log.gz (más nuevos primero).
pub fn list_log_files(app: &AppHandle, name: &str) -> Result<Vec<LogFile>> {
    let clean = server_manager::validate_name(name)?;
    let dir = server_path(app, &clean)?;
    let mut out = vec![];
    let latest = dir.join("logs").join("latest.log");
    if latest.is_file() {
        if let Some((size, modified)) = file_meta(&latest) {
            out.push(LogFile {
                file: "logs/latest.log".to_string(),
                kind: "latest".to_string(),
                size,
                modified,
            });
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("crashlogs")) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().map(|x| x == "log").unwrap_or(false) {
                if let Some((size, modified)) = file_meta(&p) {
                    out.push(LogFile {
                        file: format!("crashlogs/{}", e.file_name().to_string_lossy()),
                        kind: "crash".to_string(),
                        size,
                        modified,
                    });
                }
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("logs")) {
        for e in entries.flatten() {
            let p = e.path();
            let fname = e.file_name().to_string_lossy().into_owned();
            if p.is_file() && fname.ends_with(".log.gz") {
                if let Some((size, modified)) = file_meta(&p) {
                    out.push(LogFile {
                        file: format!("logs/{fname}"),
                        kind: "rotated".to_string(),
                        size,
                        modified,
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(out)
}

pub fn read_log_file(app: &AppHandle, name: &str, file: &str, max_lines: u32) -> Result<Vec<String>> {
    let clean = server_manager::validate_name(name)?;
    let dir = server_path(app, &clean)?;
    let path = resolve_log_file(&dir, file)?;
    tail_lines(&path, max_lines.clamp(20, 2000) as usize)
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
/// OJO perf: `new_all()` + `refresh_all()` cuestan ~500ms porque enumeran
/// TODOS los procesos del equipo. Acá solo se refresca memoria, CPU global
/// y los PIDs propios (~1ms), por eso el poll cada 2s no traba nada.
pub fn server_stats(app: &AppHandle) -> Result<AllStats> {
    let procs: tauri::State<'_, ProcessState> = app.state::<ProcessState>();
    let snapshot: Vec<(String, u32)> = {
        let running = procs.running.lock().expect("lock");
        running.iter().map(|(n, r)| (n.clone(), r.pid)).collect()
    };
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let pids: Vec<sysinfo::Pid> = snapshot.iter().map(|(_, pid)| sysinfo::Pid::from_u32(*pid)).collect();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&pids), true);
    let host = host_stats(&sys);
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
    // `new()` + `refresh_memory()`: ~0.1ms. (`new_all()` tardaba ~400ms.)
    let mut sys = sysinfo::System::new();
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

    #[test]
    fn ring_caps_at_300_keeping_order() {
        let ring = Mutex::new(VecDeque::new());
        for i in 0..350 {
            push_ring(&ring, format!("l{i}"));
        }
        let ring = ring.lock().expect("lock");
        assert_eq!(ring.len(), RING_CAP);
        assert_eq!(ring[0], "l50");
        assert_eq!(ring[RING_CAP - 1], "l349");
    }

    #[test]
    fn crashlog_dumps_to_file() {
        let tmp = std::env::temp_dir().join("digspawn-test-crash");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let lines: Vec<String> = (0..5).map(|i| format!("linea {i}")).collect();
        let path = dump_crashlog(&tmp, &lines).unwrap();
        assert!(path.starts_with(tmp.join("crashlogs")));
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("linea 4"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_rejects_traversal_and_keeps_allowed() {
        let tmp = std::env::temp_dir().join("digspawn-test-resolve");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("crashlogs")).unwrap();
        std::fs::create_dir_all(tmp.join("logs")).unwrap();
        std::fs::write(tmp.join("crashlogs").join("crash-1.log"), "x\n").unwrap();
        assert!(resolve_log_file(&tmp, "crashlogs/crash-1.log").is_ok());
        assert!(resolve_log_file(&tmp, "../fuera.log").is_err());
        assert!(resolve_log_file(&tmp, "crashlogs/../../x.log").is_err());
        assert!(resolve_log_file(&tmp, "server.jar").is_err());
        assert!(resolve_log_file(&tmp, "crashlogs/noexiste.log").is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tail_reads_gz() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join("digspawn-test-gz");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let gz_path = tmp.join("2026-01-01-1.log.gz");
        let f = std::fs::File::create(&gz_path).unwrap();
        let mut enc = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
        for i in 0..10 {
            writeln!(enc, "g{i}").unwrap();
        }
        enc.finish().unwrap();
        let lines = tail_lines(&gz_path, 3).unwrap();
        assert_eq!(lines, vec!["g7".to_string(), "g8".to_string(), "g9".to_string()]);
        let _ = std::fs::remove_dir_all(&tmp);
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

    /// El watcher vuelca el ring al matar el proceso (kill = crash).
    /// Reproduce el camino del watcher: ring con líneas reales → dump.
    #[test]
    #[ignore = "needs-network-heavy"]
    fn live_crash_dump_on_kill() {
        use std::sync::Mutex;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let tmp = std::env::temp_dir().join("digspawn-test-crashlive");
            let _ = std::fs::remove_dir_all(&tmp);
            let servers = tmp.join("servers");
            std::fs::create_dir_all(&servers).unwrap();

            let vs = crate::paper_api::list_versions().await.unwrap();
            let info = crate::server_manager::create_server_at(
                &servers,
                crate::server_manager::CreateInput {
                    name: "Crash Test".to_string(),
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
            let required = crate::runtime::required_java_for(&info.server_type, &info.version).await;
            let java = crate::runtime::ensure_runtime_at(&tmp, required, &|_, _| {})
                .await
                .expect("java");

            let mut child = spawn_process(&java, &dir, 2048).await.expect("spawn");
            let stdout = child.stdout.take().unwrap();
            let mut lines = tokio::io::BufReader::new(stdout).lines();
            let ring = Mutex::new(VecDeque::new());
            // Juntar algunas líneas reales y matar de golpe (como kill -9).
            for _ in 0..5 {
                if let Ok(Some(line)) = lines.next_line().await {
                    push_ring(&ring, line);
                }
            }
            child.start_kill().expect("kill");
            let status = child.wait().await.expect("wait tras kill");
            assert!(!status.success(), "el kill debe dar exit anormal");

            let dumped: Vec<String> = ring.lock().expect("lock").iter().cloned().collect();
            assert!(!dumped.is_empty(), "el ring debe tener líneas reales");
            let path = dump_crashlog(&dir, &dumped).unwrap();
            assert!(path.is_file());
            // Y aparece listado para el Historial.
            let names: Vec<String> = std::fs::read_dir(dir.join("crashlogs"))
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(names.len(), 1);
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
