// JavaDetector — detección del java del sistema (hito 2).
// El autoinstall portable Adoptium es hito 3+; acá solo se detecta y se informa.

use serde::Serialize;

use crate::errors::{Result, ServerError};

/// Java detectado en el sistema.
#[derive(Debug, Clone, Serialize)]
pub struct JavaInfo {
    pub path: String,
    pub version: u32,
    pub raw: String,
}

/// Parsea el major de la salida de `java -version` (va por stderr).
/// Ej: `openjdk version "25.0.4.1"...` -> 25 · `version "1.8.0_422"` -> 8.
pub fn parse_java_major(output: &str) -> Option<u32> {
    for line in output.lines() {
        if let Some(q1) = line.find('"') {
            let rest = &line[q1 + 1..];
            if let Some(q2) = rest.find('"') {
                let v = &rest[..q2];
                let mut parts = v.split('.');
                let first: u32 = parts.next()?.parse().ok()?;
                if first == 1 {
                    // Esquema viejo: 1.8.x -> Java 8.
                    return parts.next()?.parse().ok();
                }
                return Some(first);
            }
        }
    }
    None
}

/// Java requerido según la versión de MC (tabla del SPEC).
/// Versiones con esquema nuevo (ej "26.2", sin prefijo 1.) -> fallback 21.
pub fn required_java(mc_version: &str) -> u32 {
    let nums: Vec<u32> = mc_version
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    if nums.first() != Some(&1) || nums.len() < 2 {
        return 21;
    }
    let minor = nums[1];
    if minor < 17 {
        8
    } else if minor == 17 {
        16
    } else if minor < 20 {
        17
    } else if minor == 20 {
        let patch = nums.get(2).copied().unwrap_or(0);
        if patch >= 5 { 21 } else { 17 }
    } else {
        21
    }
}

fn probe(path: &std::path::Path) -> Option<JavaInfo> {
    let out = std::process::Command::new(path).arg("-version").output().ok()?;
    // `java -version` escribe a stderr.
    let raw = String::from_utf8_lossy(&out.stderr).into_owned();
    let version = parse_java_major(&raw)?;
    Some(JavaInfo {
        path: path.to_string_lossy().into_owned(),
        version,
        raw: raw.lines().next().unwrap_or("").to_string(),
    })
}

/// Busca java: `JAVA_HOME/bin/java` primero, luego `java` del PATH.
pub fn detect_java() -> Result<JavaInfo> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let cand = std::path::Path::new(&home).join("bin").join("java");
        if let Some(info) = probe(&cand) {
            return Ok(info);
        }
    }
    if let Some(info) = probe(std::path::Path::new("java")) {
        return Ok(info);
    }
    Err(ServerError::JavaNotFound(
        "No se encontró Java en el sistema (JAVA_HOME ni PATH). Al arrancar, Digspawn descarga un runtime portable.".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_versions() {
        assert_eq!(
            parse_java_major("openjdk version \"25.0.4.1\" 2026-08-18 LTS"),
            Some(25)
        );
        assert_eq!(parse_java_major("openjdk version \"21.0.3\" 2024-04-16"), Some(21));
    }

    #[test]
    fn parses_legacy_1_8() {
        assert_eq!(
            parse_java_major("java version \"1.8.0_422\"\nJava(TM) SE Runtime Environment"),
            Some(8)
        );
    }

    #[test]
    fn required_java_follows_spec_table() {
        assert_eq!(required_java("1.12.2"), 8);
        assert_eq!(required_java("1.16.5"), 8);
        assert_eq!(required_java("1.17.1"), 16);
        assert_eq!(required_java("1.18.2"), 17);
        assert_eq!(required_java("1.20.4"), 17);
        assert_eq!(required_java("1.20.5"), 21);
        assert_eq!(required_java("1.21.1"), 21);
        assert_eq!(required_java("26.2"), 21); // esquema nuevo -> fallback
    }
}
