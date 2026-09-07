// PropertiesParser — `server.properties` (hito 2: solo escritura de defaults).

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
}
