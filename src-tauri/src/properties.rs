// PropertiesParser — lectura/escritura de `server.properties` con round-trip:
// se preservan comentarios, orden y claves desconocidas; solo se tocan las
// claves editadas (más las que falten, que se agregan al final).

use std::collections::HashMap;
use std::path::Path;

use crate::errors::{Result, ServerError};

/// Claves editables del hito 4 (set del SPEC) con su validación.
pub fn validate(key: &str, value: &str) -> Result<String> {
    let v = value.trim().to_string();
    match key {
        "server-port" => {
            let p: u16 = v
                .parse()
                .map_err(|_| ServerError::InvalidName("Puerto inválido (1–65535).".to_string()))?;
            if p == 0 {
                return Err(ServerError::InvalidName("Puerto inválido (1–65535).".to_string()));
            }
            Ok(p.to_string())
        }
        "max-players" => {
            let n: u32 = v
                .parse()
                .map_err(|_| ServerError::InvalidName("max-players debe ser un número ≥ 1.".to_string()))?;
            if n < 1 || n > 1000 {
                return Err(ServerError::InvalidName("max-players debe estar entre 1 y 1000.".to_string()));
            }
            Ok(n.to_string())
        }
        "view-distance" => {
            let n: i32 = v
                .parse()
                .map_err(|_| ServerError::InvalidName("view-distance debe ser un número (2–32).".to_string()))?;
            if n < 2 || n > 32 {
                return Err(ServerError::InvalidName("view-distance debe estar entre 2 y 32.".to_string()));
            }
            Ok(n.to_string())
        }
        "difficulty" => match v.to_lowercase().as_str() {
            "peaceful" | "easy" | "normal" | "hard" => Ok(v.to_lowercase()),
            _ => Err(ServerError::InvalidName("difficulty: peaceful, easy, normal o hard.".to_string())),
        },
        "gamemode" => match v.to_lowercase().as_str() {
            "survival" | "creative" | "adventure" | "spectator" => Ok(v.to_lowercase()),
            _ => Err(ServerError::InvalidName(
                "gamemode: survival, creative, adventure o spectator.".to_string(),
            )),
        },
        "online-mode" | "pvp" | "white-list" => match v.to_lowercase().as_str() {
            "true" | "false" => Ok(v.to_lowercase()),
            _ => Err(ServerError::InvalidName(format!("{key} debe ser true o false."))),
        },
        "motd" => {
            if v.is_empty() || v.len() > 200 {
                return Err(ServerError::InvalidName("motd: entre 1 y 200 caracteres.".to_string()));
            }
            Ok(v)
        }
        _ => Err(ServerError::InvalidName(format!("Propiedad no editable: {key}."))),
    }
}

/// Defaults del SPEC al crear un server.
pub fn defaults() -> String {
    let pairs: &[(&str, &str)] = &[
        ("online-mode", "true"),
        ("difficulty", "normal"),
        ("gamemode", "survival"),
        ("pvp", "true"),
        ("max-players", "10"),
        ("motd", "Nuestro server!"),
        ("view-distance", "10"),
        ("server-port", "25565"),
    ];
    let mut out = String::from("#Minecraft server properties (generado por Digspawn)\n");
    for (k, v) in pairs {
        out.push_str(k);
        out.push('=');
        out.push_str(v);
        out.push('\n');
    }
    out
}

/// Lee el archivo a mapa (ignora comentarios y líneas sin `=`).
pub fn read_map(path: &Path) -> Result<HashMap<String, String>> {
    let raw = std::fs::read_to_string(path)?;
    let mut map = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok(map)
}

/// Aplica cambios preservando el resto del archivo tal cual.
pub fn apply_changes(path: &Path, changes: &HashMap<String, String>) -> Result<()> {
    // Validar todo antes de tocar el disco.
    let mut clean: HashMap<String, String> = HashMap::new();
    for (k, v) in changes {
        clean.insert(k.clone(), validate(k, v)?);
    }
    let raw = std::fs::read_to_string(path)?;
    let mut pending = clean.clone();
    let mut out = String::new();
    for line in raw.lines() {
        let t = line.trim();
        if let Some((k, _)) = t.split_once('=') {
            let k = k.trim();
            if !t.starts_with('#') {
                if let Some(nv) = pending.remove(k) {
                    out.push_str(k);
                    out.push('=');
                    out.push_str(&nv);
                    out.push('\n');
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    // Claves validadas que no estaban: se agregan al final.
    let mut rest: Vec<_> = pending.into_iter().collect();
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in rest {
        out.push_str(&k);
        out.push('=');
        out.push_str(&v);
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_spec_defaults() {
        let p = defaults();
        for key in [
            "online-mode=true",
            "difficulty=normal",
            "gamemode=survival",
            "pvp=true",
            "max-players=10",
            "motd=Nuestro server!",
            "view-distance=10",
        ] {
            assert!(p.contains(key), "falta {key}");
        }
    }

    #[test]
    fn roundtrip_preserves_comments_and_unknown_keys() {
        let dir = std::env::temp_dir().join("digspawn-test-props");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("server.properties");
        std::fs::write(&f, "# comentario\nmotd=Viejo\nclave-rara=123\n").unwrap();
        let mut ch = HashMap::new();
        ch.insert("motd".to_string(), "Nuevo".to_string());
        ch.insert("max-players".to_string(), "20".to_string());
        apply_changes(&f, &ch).unwrap();
        let raw = std::fs::read_to_string(&f).unwrap();
        assert!(raw.contains("# comentario"), "comentario: {raw}");
        assert!(raw.contains("motd=Nuevo"), "motd: {raw}");
        assert!(raw.contains("clave-rara=123"), "rara: {raw}");
        assert!(raw.contains("max-players=20"), "nueva: {raw}");
        let map = read_map(&f).unwrap();
        assert_eq!(map["motd"], "Nuevo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(validate("server-port", "0").is_err());
        assert!(validate("server-port", "abc").is_err());
        assert!(validate("server-port", "25565").is_ok());
        assert!(validate("difficulty", "extrema").is_err());
        assert!(validate("difficulty", "HARD").is_ok());
        assert!(validate("online-mode", "si").is_err());
        assert!(validate("white-list", "true").is_ok());
        assert!(validate("max-players", "0").is_err());
        assert!(validate("view-distance", "64").is_err());
        assert!(validate("motd", "").is_err());
        assert!(validate("seed-cualquiera", "x").is_err());
    }
}
