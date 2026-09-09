// Backups .zip con alcance elegible (mundo o completo).
// - Completo: todo menos regenerable/pesado (logs, crashlogs, cache,
//   versions/, server.jar, session.lock, .log.gz).
// - Mundo: carpetas world* + level.dat* + *.json de jugadores de la raíz.
// Compresión deflate nivel 6: en mundos reales (.mca ya comprimidos por
// dentro) el 9 no achica nada y solo suma ~20% de CPU; el 1 hasta infla.
// Medido en máquina real: 6 = mismo tamaño que 9, más rápido.
// Crear funciona con el server corriendo (antes hace `save-all flush`);
// restaurar exige server frenado.
// Mientras un backup corre, el arranque se inhabilita (lock + guard).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::errors::{Result, ServerError};
use crate::server_manager::{self, SIDECAR};

/// Servers con backup en curso (uno por vez por server).
pub struct BackupState {
    busy: Mutex<HashSet<String>>,
}

impl BackupState {
    pub fn new() -> Self {
        Self { busy: Mutex::new(HashSet::new()) }
    }

    pub fn is_busy(&self, name: &str) -> bool {
        self.busy.lock().expect("lock").contains(name)
    }

    fn set_busy(&self, name: &str) {
        self.busy.lock().expect("lock").insert(name.to_string());
    }

    fn clear_busy(&self, name: &str) {
        self.busy.lock().expect("lock").remove(name);
    }
}

impl Default for BackupState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub file: String,
    pub scope: String, // "full" | "world"
    pub kind: String,  // "manual" | "auto" | "onstart"
    pub size: u64,
    pub modified: u64, // unix timestamp
}

#[derive(Debug, Clone, Serialize)]
struct BackupProgress {
    server: String,
    file: String,
    files_done: usize,
    files_total: usize,
    bytes_done: u64,
    bytes_total: u64,
    pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
struct BackupStateEvent {
    server: String,
    backing_up: bool,
}

fn emit_progress(
    app: &AppHandle,
    server: &str,
    file: &str,
    done_files: usize,
    total_files: usize,
    done_bytes: u64,
    total_bytes: u64,
) {
    let pct = if total_bytes > 0 {
        Some(done_bytes as f64 / total_bytes as f64 * 100.0)
    } else {
        None
    };
    let _ = app.emit(
        "backup-progress",
        BackupProgress {
            server: server.to_string(),
            file: file.to_string(),
            files_done: done_files,
            files_total: total_files,
            bytes_done: done_bytes,
            bytes_total: total_bytes,
            pct,
        },
    );
}

fn emit_state(app: &AppHandle, server: &str, backing_up: bool) {
    let _ = app.emit("backup-state", BackupStateEvent { server: server.to_string(), backing_up });
}

pub fn backups_dir(app: &AppHandle) -> Result<PathBuf> {
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| ServerError::Io(format!("No se pudo resolver el dataDir: {e}")))?;
    Ok(data.join("backups"))
}

/// Misma resolución con ruta explícita (para tests sin Tauri).
#[cfg(test)]
fn backups_dir_at(base: &Path) -> PathBuf {
    base.join("backups")
}

/// Alcance desde el nombre (`..._full[_auto|_start].zip` / `..._world....zip`).
fn scope_of(file_name: &str) -> &str {
    if file_name.contains("_world") {
        "world"
    } else {
        "full"
    }
}

/// Origen desde el nombre: manual | auto (periódico) | onstart (al arrancar).
fn kind_of(file_name: &str) -> &str {
    if file_name.ends_with("_auto.zip") {
        "auto"
    } else if file_name.ends_with("_start.zip") {
        "onstart"
    } else {
        "manual"
    }
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

/// Espacio libre del disco donde vive `path` (mejor esfuerzo: si no se
/// puede averiguar, devuelve None y el pre-chequeo se saltea).
fn free_bytes_for(path: &Path) -> Option<u64> {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let mut best: Option<u64> = None;
    let mut best_len = 0usize;
    for d in disks.list() {
        let mount = d.mount_point();
        if path.starts_with(mount) {
            let len = mount.as_os_str().len();
            if len >= best_len {
                best_len = len;
                best = Some(d.available_space());
            }
        }
    }
    best
}

fn fmt_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if bytes >= 1024 * 1024 {
        format!("{:.0} MB", bytes as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

/// ¿Este archivo (ruta relativa al server) entra en el backup?
/// `scope`: "full" | "world".
pub fn should_include(rel: &Path, scope: &str) -> bool {
    let mut comps = rel.components().peekable();
    let first = comps.peek().and_then(|c| c.as_os_str().to_str()).unwrap_or("");
    // Directorios regenerables o puro ruido: afuera siempre.
    for c in rel.components() {
        let s = c.as_os_str().to_string_lossy();
        if matches!(s.as_ref(), "logs" | "crashlogs" | "cache" | "versions") {
            return false;
        }
    }
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // Pesados/regenerables o locks: afuera siempre.
    if name == "server.jar" || name == "session.lock" || name.ends_with(".log.gz") {
        return false;
    }
    // Zips sueltos en la raíz (restos de copias manuales): afuera.
    if first == name && name.ends_with(".zip") {
        return false;
    }
    if scope == "world" {
        // Carpetas world* (world, world_nether, world_the_end...) + datos de
        // nivel/jugadores que Vanilla guarda en la raíz.
        if first.starts_with("world") {
            return true;
        }
        if rel.parent().map(|p| p.as_os_str().is_empty()).unwrap_or(false)
            && (name == "level.dat"
                || name == "level.dat_old"
                || name.ends_with(".json"))
        {
            return true;
        }
        return false;
    }
    true
}

/// Junta (relativa, absoluta, tamaño) ordenado. Solo archivos (los dirs
/// se recrean al extraer).
pub fn collect_files(server_dir: &Path, scope: &str) -> Result<Vec<(PathBuf, PathBuf, u64)>> {
    let mut out = vec![];
    let mut stack = vec![server_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)?;
        for e in entries.flatten() {
            let abs = e.path();
            let rel = abs.strip_prefix(server_dir).map(|p| p.to_path_buf()).unwrap_or_default();
            if abs.is_dir() {
                // Podar ramas enteras excluidas (logs, cache...).
                let mut skip = false;
                for c in rel.components() {
                    if matches!(
                        c.as_os_str().to_string_lossy().as_ref(),
                        "logs" | "crashlogs" | "cache" | "versions"
                    ) {
                        skip = true;
                        break;
                    }
                }
                if !skip {
                    stack.push(abs);
                }
            } else if should_include(&rel, scope) {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push((rel, abs, size));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Escribe el zip (bloqueante: llamar desde spawn_blocking).
/// `on_progress(files_done, files_total, bytes_done, bytes_total)`.
pub fn write_zip(
    files: &[(PathBuf, PathBuf, u64)],
    out_file: &Path,
    mut on_progress: impl FnMut(usize, usize, u64, u64),
) -> Result<()> {
    let f = std::fs::File::create(out_file)?;
    let mut zip = zip::ZipWriter::new(f);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6))
        .unix_permissions(0o644);
    let total_files = files.len();
    let total_bytes: u64 = files.iter().map(|(_, _, s)| s).sum();
    let mut done_bytes = 0u64;
    // Dirs explícitos para que el restore recree vacíos también.
    let mut dirs = HashSet::new();
    for (rel, _, _) in files {
        let mut cur = PathBuf::new();
        for c in rel.parent().unwrap_or(Path::new("")).components() {
            cur.push(c);
            dirs.insert(cur.clone());
        }
    }
    let mut dir_list: Vec<_> = dirs.into_iter().collect();
    dir_list.sort();
    for d in dir_list {
        let name = format!("{}/", d.to_string_lossy().replace('\\', "/"));
        zip.add_directory(name, options)?;
    }
    for (i, (rel, abs, size)) in files.iter().enumerate() {
        let name = rel.to_string_lossy().replace('\\', "/");
        zip.start_file(name, options)?;
        let mut src = std::fs::File::open(abs)?;
        std::io::copy(&mut src, &mut zip)?;
        done_bytes += size;
        on_progress(i + 1, total_files, done_bytes, total_bytes);
    }
    zip.finish()?;
    Ok(())
}

/// Reabre el zip y chequea que tenga todos los archivos (caza truncados
/// que igual abren bien pero están incompletos).
fn verify_zip(path: &Path, expected_files: usize) -> Result<()> {
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    let mut count = 0usize;
    for i in 0..zip.len() {
        if !zip.by_index(i)?.is_dir() {
            count += 1;
        }
    }
    if count != expected_files {
        return Err(ServerError::Io(format!(
            "El backup quedó incompleto ({count}/{expected_files} archivos)."
        )));
    }
    Ok(())
}

fn resolve_backup_file(app: &AppHandle, clean: &str, file: &str) -> Result<PathBuf> {
    let base = backups_dir(app)?.join(clean);
    let target = base.join(file);
    let canon = target
        .canonicalize()
        .map_err(|_| ServerError::NotFound("Backup inexistente.".to_string()))?;
    if !canon.starts_with(&base) {
        return Err(ServerError::InvalidName("Backup inválido.".to_string()));
    }
    if canon.extension().map(|e| e != "zip").unwrap_or(true) {
        return Err(ServerError::InvalidName("Solo se restauran .zip de backup.".to_string()));
    }
    Ok(canon)
}

pub fn list_backups(app: &AppHandle, name: &str) -> Result<Vec<BackupInfo>> {
    let clean = server_manager::validate_name(name)?;
    let dir = backups_dir(app)?.join(&clean);
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for e in std::fs::read_dir(&dir)?.flatten() {
        let p = e.path();
        if !p.is_file() || p.extension().map(|x| x != "zip").unwrap_or(true) {
            continue;
        }
        let fname = e.file_name().to_string_lossy().into_owned();
        if let Some((size, modified)) = file_meta(&p) {
            out.push(BackupInfo {
                file: fname.clone(),
                scope: scope_of(&fname).to_string(),
                kind: kind_of(&fname).to_string(),
                size,
                modified,
            });
        }
    }
    out.sort_by(|a, b| b.file.cmp(&a.file)); // más nuevos primero
    Ok(out)
}

pub fn is_backing_up(app: &AppHandle, name: &str) -> bool {
    let clean = match server_manager::validate_name(name) {
        Ok(c) => c,
        Err(_) => return false,
    };
    app.state::<BackupState>().is_busy(&clean)
}

fn backup_filename(scope: &str, kind: &str) -> String {
    let tag = match kind {
        "auto" => format!("{scope}_auto"),
        "onstart" => format!("{scope}_start"),
        _ => scope.to_string(),
    };
    format!("{}_{}.zip", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"), tag)
}

/// Crea un backup con progreso. Funciona con el server corriendo (antes
/// flushea con `save-all`); el lock + `backup-state` avisan a la UI para
/// inhabilitar el arranque hasta terminar.
pub async fn create_backup(app: &AppHandle, name: &str, scope: &str) -> Result<BackupInfo> {
    create_backup_with_kind(app, name, scope, "manual").await
}

/// Crea un backup con progreso. Funciona con el server corriendo (antes
/// flushea con `save-all`); el lock + `backup-state` avisan a la UI para
/// inhabilitar el arranque hasta terminar. Al final poda según retención.
/// `kind`: "manual" | "auto" (periódico) | "onstart" (al arrancar).
pub async fn create_backup_with_kind(
    app: &AppHandle,
    name: &str,
    scope: &str,
    kind: &str,
) -> Result<BackupInfo> {
    if scope != "full" && scope != "world" {
        return Err(ServerError::InvalidName("Alcance: full o world.".to_string()));
    }
    if !matches!(kind, "manual" | "auto" | "onstart") {
        return Err(ServerError::InvalidName("Origen: manual, auto u onstart.".to_string()));
    }
    let clean = server_manager::validate_name(name)?;
    let state: tauri::State<'_, BackupState> = app.state();
    if state.is_busy(&clean) {
        return Err(ServerError::Busy("Ya hay un backup en curso para este server.".to_string()));
    }
    let server_dir = server_manager::servers_dir(app)?.join(&clean);
    if !server_dir.join(SIDECAR).is_file() {
        return Err(ServerError::NotFound(format!("No existe el server \"{clean}\".")));
    }
    let out_dir = backups_dir(app)?.join(&clean);
    std::fs::create_dir_all(&out_dir)?;
    state.set_busy(&clean);
    emit_state(app, &clean, true);
    let res = create_backup_inner(app, &clean, &server_dir, &out_dir, scope, kind).await;
    state.clear_busy(&clean);
    emit_state(app, &clean, false);
    let info = res?;
    // Retención por pool (los manuales no se tocan solos).
    if let Ok(cfg) = read_backup_config(&server_dir) {
        let _ = prune_backups_at(&out_dir, cfg.keep_count, cfg.keep_gb, "auto");
        let _ = prune_backups_at(
            &out_dir,
            cfg.onstart_keep_count,
            cfg.onstart_keep_gb,
            "onstart",
        );
    }
    Ok(info)
}

async fn create_backup_inner(
    app: &AppHandle,
    clean: &str,
    server_dir: &Path,
    out_dir: &Path,
    scope: &str,
    kind: &str,
) -> Result<BackupInfo> {
    // Con el server corriendo, pedir flush para que la copia salga consistente.
    if app.state::<crate::processes::ProcessState>().is_running(clean) {
        let _ = crate::processes::send_command(app, clean, "save-all flush").await;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    let files = collect_files(server_dir, scope)?;
    if files.is_empty() {
        return Err(ServerError::Io("Nada para respaldar (¿carpeta vacía?).".to_string()));
    }
    let total_bytes: u64 = files.iter().map(|(_, _, s)| s).sum();
    // Pre-chequeo conservador: se exige lugar para el total SIN comprimir
    // (lo comprimido siempre ocupa menos, salvo overhead mínimo).
    if let Some(free) = free_bytes_for(out_dir) {
        if free < total_bytes {
            return Err(ServerError::Io(format!(
                "No hay espacio suficiente: el backup necesita ~{} y quedan {} libres.",
                fmt_size(total_bytes),
                fmt_size(free)
            )));
        }
    }
    let mut out_file = out_dir.join(backup_filename(scope, kind));
    let mut n = 1u32;
    while out_file.exists() {
        let tag = match kind {
            "auto" => format!("{scope}_auto"),
            "onstart" => format!("{scope}_start"),
            _ => scope.to_string(),
        };
        out_file = out_dir.join(format!(
            "{}_{}_{}.zip",
            chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"),
            tag,
            n
        ));
        n += 1;
    }
    let fname = out_file.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let file_count = files.len();
    // Zip en thread bloqueante (puede tardar con mundos grandes).
    let app_emit = app.clone();
    let server = clean.to_string();
    let file_label = fname.clone();
    let joined = tokio::task::spawn_blocking(move || {
        write_zip(&files, &out_file, |done_f, total_f, done_b, total_b| {
            emit_progress(&app_emit, &server, &file_label, done_f, total_f, done_b, total_b);
        })
    })
    .await
    .map_err(|e| ServerError::Io(format!("Backup interrumpido: {e}")))?;
    // Si falló a mitad de camino (disco lleno igual, corte...), el .zip
    // trunco NO debe quedar como si fuera válido: se borra.
    if let Err(e) = joined {
        let _ = std::fs::remove_file(out_dir.join(&fname));
        return Err(e);
    }
    // El archivo final se verifica reabriéndolo antes de darlo por bueno.
    if let Err(e) = verify_zip(&out_dir.join(&fname), file_count) {
        let _ = std::fs::remove_file(out_dir.join(&fname));
        return Err(e);
    }
    let (size, modified) = file_meta(&out_dir.join(&fname)).unwrap_or((0, 0));
    Ok(BackupInfo { file: fname.clone(), scope: scope.to_string(), kind: kind.to_string(), size, modified })
}

/// Restaura un backup encima del server (bloqueante: llamar desde
/// spawn_blocking). Exige server frenado y sin backup en curso.
pub fn restore_backup(app: &AppHandle, name: &str, file: &str) -> Result<()> {
    let clean = server_manager::validate_name(name)?;
    if app.state::<BackupState>().is_busy(&clean) {
        return Err(ServerError::Busy("Hay un backup en curso: esperá a que termine.".to_string()));
    }
    if app.state::<crate::processes::ProcessState>().is_running(&clean) {
        return Err(ServerError::Busy("Frená el server para restaurar un backup.".to_string()));
    }
    let src = resolve_backup_file(app, &clean, file)?;
    let server_dir = server_manager::servers_dir(app)?.join(&clean);
    if !server_dir.is_dir() {
        return Err(ServerError::NotFound(format!("No existe el server \"{clean}\".")));
    }
    let f = std::fs::File::open(&src)?;
    let mut zip = zip::ZipArchive::new(f)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // Entradas con .. o absolutas se saltean (zip ajeno/malicioso).
        let rel = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };
        let target = server_dir.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&target)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    Ok(())
}

pub fn delete_backup(app: &AppHandle, name: &str, file: &str) -> Result<()> {
    let clean = server_manager::validate_name(name)?;
    let target = resolve_backup_file(app, &clean, file)?;
    std::fs::remove_file(&target)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Config (sidecar) + retención + programador automático.
// ---------------------------------------------------------------------------

use crate::server_manager::BackupConfig;

fn read_backup_config(server_dir: &Path) -> Result<BackupConfig> {
    let raw = std::fs::read_to_string(server_dir.join(SIDECAR))
        .map_err(|_| ServerError::NotFound("Falta digspawn.json.".to_string()))?;
    let meta: server_manager::ServerMeta = serde_json::from_str(&raw)
        .map_err(|e| ServerError::Io(format!("digspawn.json inválido: {e}")))?;
    Ok(meta.backup)
}

pub fn get_backup_config(app: &AppHandle, name: &str) -> Result<BackupConfig> {
    let clean = server_manager::validate_name(name)?;
    let dir = server_manager::servers_dir(app)?.join(&clean);
    read_backup_config(&dir)
}

pub fn set_backup_config(app: &AppHandle, name: &str, cfg: BackupConfig) -> Result<BackupConfig> {
    if cfg.auto_scope != "full" && cfg.auto_scope != "world" {
        return Err(ServerError::InvalidName("Alcance auto: full o world.".to_string()));
    }
    if cfg.on_start_scope != "full" && cfg.on_start_scope != "world" {
        return Err(ServerError::InvalidName("Alcance al arrancar: full o world.".to_string()));
    }
    if cfg.onstart_keep_count > 10000 {
        return Err(ServerError::InvalidName("Conservar al arrancar: como mucho 10000 backups.".to_string()));
    }
    if !(0.0..=100000.0).contains(&cfg.onstart_keep_gb) {
        return Err(ServerError::InvalidName("Tope al arrancar en GB: entre 0 (sin tope) y 100000.".to_string()));
    }
    if cfg.auto_hours < 1 || cfg.auto_hours > 720 {
        return Err(ServerError::InvalidName("Cada cuántas horas: entre 1 y 720.".to_string()));
    }
    if cfg.keep_count > 10000 {
        return Err(ServerError::InvalidName("Conservar: como mucho 10000 backups.".to_string()));
    }
    if !(0.0..=100000.0).contains(&cfg.keep_gb) {
        return Err(ServerError::InvalidName("Tope en GB: entre 0 (sin tope) y 100000.".to_string()));
    }
    let clean = server_manager::validate_name(name)?;
    let dir = server_manager::servers_dir(app)?.join(&clean);
    let raw = std::fs::read_to_string(dir.join(SIDECAR))
        .map_err(|_| ServerError::NotFound(format!("No existe el server \"{clean}\".")))?;
    let mut meta: server_manager::ServerMeta = serde_json::from_str(&raw)
        .map_err(|e| ServerError::Io(format!("digspawn.json inválido: {e}")))?;
    meta.backup = cfg.clone();
    std::fs::write(dir.join(SIDECAR), serde_json::to_string_pretty(&meta)?)?;
    Ok(cfg)
}

/// Lógica pura de retención: dados los backups AUTO (file, tamaño, modificado),
/// devuelve cuáles borrar (los más viejos) para cumplir cantidad y/o GB.
/// 0 = sin límite en esa dimensión.
pub fn pick_prune(autos: &[(String, u64, u64)], keep_count: u32, keep_gb: f64) -> Vec<String> {
    let mut sorted = autos.to_vec();
    sorted.sort_by(|a, b| b.2.cmp(&a.2).then(b.0.cmp(&a.0))); // nuevos primero
    let mut drop = vec![];
    let mut kept_count = 0u32;
    let mut kept_bytes = 0u64;
    let cap_bytes = (keep_gb * 1024.0 * 1024.0 * 1024.0) as u64;
    for (file, size, _) in sorted {
        kept_count += 1;
        kept_bytes += size;
        let over_count = keep_count > 0 && kept_count > keep_count;
        let over_gb = keep_gb > 0.0 && kept_bytes > cap_bytes;
        if over_count || over_gb {
            drop.push(file);
        }
    }
    drop
}

/// Borra backups viejos de un pool (`auto` u `onstart`) en `out_dir` según
/// retención. Devuelve cuántos borró. Los manuales nunca se tocan solos.
pub fn prune_backups_at(out_dir: &Path, keep_count: u32, keep_gb: f64, kind: &str) -> Result<usize> {
    if keep_count == 0 && keep_gb <= 0.0 {
        return Ok(0);
    }
    let mut autos = vec![];
    let entries = match std::fs::read_dir(out_dir) {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_file() || p.extension().map(|x| x != "zip").unwrap_or(true) {
            continue;
        }
        let fname = e.file_name().to_string_lossy().into_owned();
        if kind_of(&fname) != kind {
            continue;
        }
        if let Some((size, modified)) = file_meta(&p) {
            autos.push((fname, size, modified));
        }
    }
    let mut n = 0;
    for f in pick_prune(&autos, keep_count, keep_gb) {
        if std::fs::remove_file(out_dir.join(&f)).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

/// Un tick del programador: auto-backups vencidos de servers corriendo.
async fn auto_backup_tick(app: &AppHandle) {
    let servers = match server_manager::list_servers(app) {
        Ok(s) => s,
        Err(_) => return,
    };
    for s in servers {
        let dir = match server_manager::servers_dir(app) {
            Ok(d) => d.join(&s.name),
            Err(_) => continue,
        };
        let cfg = match read_backup_config(&dir) {
            Ok(c) if c.auto_enabled => c,
            _ => continue,
        };
        if !app.state::<crate::processes::ProcessState>().is_running(&s.name) {
            continue; // solo con el server corriendo
        }
        // Vencido si no hay periódicos o el más nuevo es más viejo que el intervalo.
        // (Los de arranque no reinician el temporizador: son otro pool.)
        let due = match list_backups(app, &s.name) {
            Ok(list) => {
                let newest = list.iter().filter(|b| b.kind == "auto").map(|b| b.modified).max();
                match newest {
                    None => true,
                    Some(ts) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(ts);
                        now.saturating_sub(ts) >= cfg.auto_hours * 3600
                    }
                }
            }
            Err(_) => false,
        };
        if !due {
            continue;
        }
        // create respeta el lock (si hay uno manual en curso, falla con Busy y se ignora).
        let _ = create_backup_with_kind(app, &s.name, &cfg.auto_scope, "auto").await;
    }
}

/// Loop del programador automático (vive mientras la app corre, incluso en tray).
pub async fn auto_backup_loop(app: AppHandle) {
    // Primer tick con demora corta para no pelear con el arranque.
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    loop {
        auto_backup_tick(&app).await;
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("digspawn-test-backup-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        let srv = dir.join("srv");
        for d in ["world/region", "world_nether/DIM-1/region", "plugins", "logs", "cache", "versions"] {
            std::fs::create_dir_all(srv.join(d)).unwrap();
        }
        std::fs::write(srv.join("world/level.dat"), b"LEVEL").unwrap();
        std::fs::write(srv.join("world_nether/DIM-1/region/r.0.0.mca"), b"NETHER").unwrap();
        std::fs::write(srv.join("world/region/r.0.0.mca"), b"REGION").unwrap();
        std::fs::write(srv.join("level.dat"), b"ROOTLEVEL").unwrap();
        std::fs::write(srv.join("whitelist.json"), b"[]").unwrap();
        std::fs::write(srv.join("server.properties"), b"motd=x").unwrap();
        std::fs::write(srv.join("server.jar"), b"JARJAR").unwrap();
        std::fs::write(srv.join("session.lock"), b"1").unwrap();
        std::fs::write(srv.join("logs/latest.log"), b"log").unwrap();
        std::fs::write(srv.join("logs/viejo.log.gz"), b"gz").unwrap();
        std::fs::write(srv.join("cache/mojang.dat"), b"cache").unwrap();
        std::fs::write(srv.join("plugins/un.jar"), b"plug").unwrap();
        std::fs::write(srv.join("ops.json"), b"[]").unwrap();
        dir
    }

    #[test]
    fn scope_world_picks_world_stuff_only() {
        let dir = setup("world");
        let srv = dir.join("srv");
        let files = collect_files(&srv, "world").unwrap();
        let rels: Vec<String> =
            files.iter().map(|(r, _, _)| r.to_string_lossy().replace('\\', "/")).collect();
        assert!(rels.contains(&"world/level.dat".to_string()), "{rels:?}");
        assert!(rels.contains(&"level.dat".to_string()), "{rels:?}");
        assert!(rels.contains(&"whitelist.json".to_string()), "{rels:?}");
        assert!(rels.iter().any(|r| r.starts_with("world_nether/")), "{rels:?}");
        assert!(!rels.iter().any(|r| r.contains("plugins")), "{rels:?}");
        assert!(!rels.iter().any(|r| r.contains("server.jar")), "{rels:?}");
        assert!(!rels.iter().any(|r| r.contains("logs")), "{rels:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scope_full_includes_configs_but_not_junk() {
        let dir = setup("full");
        let srv = dir.join("srv");
        let files = collect_files(&srv, "full").unwrap();
        let rels: Vec<String> =
            files.iter().map(|(r, _, _)| r.to_string_lossy().replace('\\', "/")).collect();
        assert!(rels.contains(&"server.properties".to_string()), "{rels:?}");
        assert!(rels.contains(&"plugins/un.jar".to_string()), "{rels:?}");
        assert!(!rels.iter().any(|r| r.contains("server.jar") && !r.contains("plugins")), "{rels:?}");
        assert!(!rels.iter().any(|r| r.starts_with("logs")), "{rels:?}");
        assert!(!rels.iter().any(|r| r.starts_with("cache")), "{rels:?}");
        assert!(!rels.iter().any(|r| r.starts_with("versions") || r.contains("versions")), "{rels:?}");
        assert!(!rels.iter().any(|r| r.ends_with(".log.gz")), "{rels:?}");
        assert!(!rels.contains(&"session.lock".to_string()), "{rels:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zip_roundtrip_restores_files() {
        let dir = setup("roundtrip");
        let srv = dir.join("srv");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let files = collect_files(&srv, "world").unwrap();
        let zip_path = out.join("b_world.zip");
        let mut seen = (0usize, 0u64);
        write_zip(&files, &zip_path, |df, _, db, _| {
            seen = (df, db);
        })
        .unwrap();
        assert_eq!(seen.0, files.len());
        assert!(seen.1 > 0);
        // Borrar original y extraer como haría restore.
        std::fs::remove_file(srv.join("world/level.dat")).unwrap();
        let f = std::fs::File::open(&zip_path).unwrap();
        let mut zip = zip::ZipArchive::new(f).unwrap();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            let rel = entry.enclosed_name().unwrap().to_path_buf();
            let target = srv.join(&rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&target).unwrap();
            } else {
                if let Some(p) = target.parent() {
                    std::fs::create_dir_all(p).unwrap();
                }
                let mut o = std::fs::File::create(&target).unwrap();
                std::io::copy(&mut entry, &mut o).unwrap();
            }
        }
        assert_eq!(std::fs::read(srv.join("world/level.dat")).unwrap(), b"LEVEL");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scope_and_kind_parsed_from_filename() {
        assert_eq!(scope_of("2026-09-08_18-00-00_full.zip"), "full");
        assert_eq!(scope_of("2026-09-08_18-00-00_world.zip"), "world");
        assert_eq!(scope_of("2026-09-08_18-00-00_full_auto.zip"), "full");
        assert_eq!(scope_of("2026-09-08_18-00-00_world_auto.zip"), "world");
        assert_eq!(scope_of("2026-09-08_18-00-00_full_start.zip"), "full");
        assert_eq!(scope_of("raro.zip"), "full");
        assert_eq!(kind_of("2026-09-08_18-00-00_full_auto.zip"), "auto");
        assert_eq!(kind_of("2026-09-08_18-00-00_full_start.zip"), "onstart");
        assert_eq!(kind_of("2026-09-08_18-00-00_full.zip"), "manual");
    }

    #[test]
    fn prune_keeps_newest_within_limits() {
        let autos = vec![
            ("a.zip".to_string(), 100u64, 100u64),
            ("b.zip".to_string(), 100u64, 200u64),
            ("c.zip".to_string(), 100u64, 300u64),
        ];
        // Sin límites: nada se borra.
        assert!(pick_prune(&autos, 0, 0.0).is_empty());
        // Tope de 2: se borra el más viejo.
        assert_eq!(pick_prune(&autos, 2, 0.0), vec!["a.zip".to_string()]);
        // Tope por GB (~250 bytes): se borra el más viejo.
        let gb = 250.0 / 1024.0 / 1024.0 / 1024.0;
        assert_eq!(pick_prune(&autos, 0, gb), vec!["a.zip".to_string()]);
    }

    #[test]
    fn old_sidecar_without_backup_parses_with_defaults() {
        let meta: server_manager::ServerMeta =
            serde_json::from_str(r#"{"type":"paper","version":"1.21","ram_mb":2048}"#).unwrap();
        assert!(!meta.backup.auto_enabled);
        assert_eq!(meta.backup.auto_hours, 12);
        assert!(!meta.backup.on_start_enabled);
        assert_eq!(meta.backup.on_start_scope, "full");
        assert_eq!(meta.backup.onstart_keep_count, 5);
        assert_eq!(meta.backup.onstart_keep_gb, 0.0);
        let cfg = BackupConfig::default();
        assert!(set_cfg_ok(&cfg));
    }

    fn set_cfg_ok(cfg: &BackupConfig) -> bool {
        cfg.auto_scope == "full"
            && (1..=720).contains(&cfg.auto_hours)
            && cfg.keep_count <= 10000
            && (0.0..=100000.0).contains(&cfg.keep_gb)
    }

    #[test]
    fn verify_zip_catches_truncated() {
        let dir = setup("verify");
        let srv = dir.join("srv");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let files = collect_files(&srv, "world").unwrap();
        let zip_path = out.join("b_world.zip");
        write_zip(&files, &zip_path, |_, _, _, _| {}).unwrap();
        assert!(verify_zip(&zip_path, files.len()).is_ok());
        // Si falta un archivo, el zip se declara incompleto.
        assert!(verify_zip(&zip_path, files.len() + 1).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn free_space_probe_works() {
        let free = free_bytes_for(&std::env::temp_dir());
        assert!(free.is_some_and(|f| f > 0));
        assert_eq!(fmt_size(512), "0 KB");
        assert_eq!(fmt_size(2 * 1024 * 1024), "2 MB");
        assert_eq!(fmt_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
