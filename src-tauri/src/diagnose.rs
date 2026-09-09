// Parser heurístico de crashes: causa probable + sugerencia, sin LLM.
// Los "no arranca / se cae" son 6-7 causas repetidas con patrones exactos
// en el log; eso cubre la mayoría sin empaquetar ningún modelo.

use serde::Serialize;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize)]
pub struct Diagnosis {
    /// Qué pasó, en criollo.
    pub cause: String,
    /// Qué hacer, en criollo.
    pub hint: String,
}

/// Class-file major -> Java que lo corre (los que importan para MC).
fn java_for_major(major: u32) -> Option<u32> {
    match major {
        52 => Some(8),
        55 => Some(11),
        59 => Some(15),
        60 => Some(16),
        61 => Some(17),
        62 => Some(18),
        63 => Some(19),
        64 => Some(20),
        65 => Some(21),
        66 => Some(22),
        67 => Some(23),
        69 => Some(25),
        _ => None,
    }
}

/// Saca el "65.0" de "Unsupported major.minor version 65.0".
fn class_major(line: &str) -> Option<u32> {
    let marker = "major.minor version";
    let i = line.find(marker)?;
    line[i + marker.len()..]
        .trim_start()
        .split('.')
        .next()?
        .trim()
        .parse::<u32>()
        .ok()
}

/// Lógica pura: busca patrones de más nuevo a más viejo (la última
/// coincidencia manda, porque el final del log es lo que lo mató).
pub fn diagnose_lines(lines: &[String]) -> Option<Diagnosis> {
    for line in lines.iter().rev() {
        if line.contains("You need to agree to the EULA") {
            return Some(Diagnosis {
                cause: "No aceptaste la EULA de Minecraft.".to_string(),
                hint: "Revisá que exista eula.txt con eula=true en la carpeta del server. \
                    Los servers creados por Digspawn ya la traen firmada."
                    .to_string(),
            });
        }
        if line.contains("FAILED TO BIND TO PORT")
            || line.contains("Perhaps a server is already running on that port")
            || line.contains("Address already in use")
        {
            return Some(Diagnosis {
                cause: "El puerto ya está en uso.".to_string(),
                hint: "Otro programa u otro server está usando ese puerto. \
                    Cambialo en Ajustes o frená el otro server y arrancá de nuevo."
                    .to_string(),
            });
        }
        if line.contains("Unsupported major.minor version")
            || line.contains("compiled by a more recent version of the Java Runtime")
        {
            let java = class_major(line).and_then(java_for_major);
            let cause = match java {
                Some(n) => format!("Java incorrecto: este server necesita Java {n}."),
                None => "Java incorrecto: este server necesita otro Java.".to_string(),
            };
            return Some(Diagnosis {
                cause,
                hint: "Digspawn descarga solo el Java portable correcto al arrancar. \
                    Si ves esto, avisá: es un bug del launcher."
                    .to_string(),
            });
        }
        if line.contains("OutOfMemoryError")
            || line.contains("GC overhead limit exceeded")
            || line.contains("Java heap space")
        {
            return Some(Diagnosis {
                cause: "Se quedó sin memoria (RAM).".to_string(),
                hint: "Subí la RAM del server en Ajustes (con el server frenado) y arrancá de nuevo."
                    .to_string(),
            });
        }
        if line.contains("Failed to start the minecraft server")
            || line.contains("already locked")
            || line.contains("DirectoryLock")
            || (line.contains("session.lock") && line.contains("Exception"))
        {
            return Some(Diagnosis {
                cause: "El mundo ya está abierto por otro proceso.".to_string(),
                hint: "Quedó un java colgado con este mundo (o dos servers apuntan al mismo). \
                    Cerrá todo, fijate que no haya un java viejo corriendo y arrancá de nuevo. \
                    Digspawn limpia los colgados solo al abrir la app."
                    .to_string(),
            });
        }
        if line.contains("Unable to access jarfile") {
            return Some(Diagnosis {
                cause: "Falta o está roto el server.jar.".to_string(),
                hint: "La descarga quedó trunca. Borrá el server y crealo de nuevo.".to_string(),
            });
        }
        if line.contains("Not in GZIP format")
            || line.contains("Failed to load level")
            || line.contains("Exception reading level")
        {
            return Some(Diagnosis {
                cause: "El mundo está corrupto (level.dat ilegible).".to_string(),
                hint: "Si tenés copia del mundo, restaurala desde la pestaña Backups; si no, \
                    borrá la carpeta world para regenerarlo (perdés lo construido)."
                    .to_string(),
            });
        }
    }
    None
}

/// Lee la cola del latest.log y diagnostica. `None` = sin log o causa
/// desconocida (la UI muestra el mensaje genérico en ese caso).
pub fn diagnose_crash(app: &AppHandle, name: &str) -> Option<Diagnosis> {
    let clean = crate::server_manager::validate_name(name).ok()?;
    let dir = crate::server_manager::servers_dir(app).ok()?.join(clean);
    let latest = dir.join("logs").join("latest.log");
    if !latest.is_file() {
        return None;
    }
    let lines = crate::processes::tail_lines(&latest, 300).ok()?;
    diagnose_lines(&lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn detects_port_in_use() {
        let l = lines(&[
            "[Server] Starting minecraft server version 1.21",
            "[Server] **** FAILED TO BIND TO PORT!",
            "[Server] Perhaps a server is already running on that port?",
        ]);
        let d = diagnose_lines(&l).expect("debería diagnosticar");
        assert!(d.cause.contains("puerto"), "causa: {}", d.cause);
    }

    #[test]
    fn detects_wrong_java_with_version() {
        let l = lines(&["Exception: Unsupported major.minor version 65.0"]);
        let d = diagnose_lines(&l).expect("debería diagnosticar");
        assert!(d.cause.contains("Java 21"), "causa: {}", d.cause);
    }

    #[test]
    fn detects_oom() {
        let l = lines(&["java.lang.OutOfMemoryError: Java heap space"]);
        let d = diagnose_lines(&l).expect("debería diagnosticar");
        assert!(d.cause.contains("memoria"), "causa: {}", d.cause);
    }

    #[test]
    fn detects_eula() {
        let l = lines(&["You need to agree to the EULA in order to run the server."]);
        let d = diagnose_lines(&l).expect("debería diagnosticar");
        assert!(d.cause.contains("EULA"), "causa: {}", d.cause);
    }

    #[test]
    fn detects_corrupt_world() {
        let l = lines(&["Caused by: java.util.zip.ZipException: Not in GZIP format"]);
        let d = diagnose_lines(&l).expect("debería diagnosticar");
        assert!(d.cause.contains("corrupto"), "causa: {}", d.cause);
    }

    #[test]
    fn detects_world_lock_held() {
        let l = lines(&[
            "[ServerMain/ERROR]: Failed to start the minecraft server",
            "net.minecraft.util.DirectoryLock$LockException: ./world/session.lock: already locked (possibly by other Minecraft instance)",
        ]);
        let d = diagnose_lines(&l).expect("debería diagnosticar");
        assert!(d.cause.contains("ya está abierto"), "causa: {}", d.cause);
    }

    #[test]
    fn latest_match_wins() {
        // El puerto falló primero pero lo que lo mató al final fue la RAM.
        let l = lines(&[
            "FAILED TO BIND TO PORT!",
            "server restarted by user",
            "java.lang.OutOfMemoryError: GC overhead limit exceeded",
        ]);
        let d = diagnose_lines(&l).expect("debería diagnosticar");
        assert!(d.cause.contains("memoria"), "causa: {}", d.cause);
    }

    #[test]
    fn unknown_returns_none() {
        let l = lines(&["[Server] Done (1.23s)!", "[Server] Stopping server"]);
        assert!(diagnose_lines(&l).is_none());
    }
}
