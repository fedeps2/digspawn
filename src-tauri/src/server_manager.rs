// ServerManager — biblioteca en disco + creación (hito 2).
// Sin start/stop/consola: eso es hito 3+.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::errors::{Result, ServerError};

/// Sidecar con los metadatos que el disco no revela (tipo/versión/RAM).
pub const SIDECAR: &str = "digspawn.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerMeta {
    #[serde(rename = "type")]
    pub server_type: String,
    pub version: String,
    pub ram_mb: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub server_type: String,
    pub version: String,
    pub ram_mb: u64,
    /// En hito 2 todo está parado (sin gestión de procesos aún).
    pub state: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateInput {
    pub name: String,
    pub server_type: String,
    pub version: String,
    pub ram_mb: u64,
    /// Consentimiento explícito de la EULA de Mojang (casilla del wizard).
    pub accept_eula: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadProgress {
    server: String,
    downloaded: u64,
    total: Option<u64>,
    pct: Option<f64>,
}

/// Valida el nombre como carpeta segura. Devuelve el nombre recortado.
pub fn validate_name(name: &str) -> Result<String> {
    let clean = name.trim();
    if clean.is_empty() {
        return Err(ServerError::InvalidName("El nombre no puede estar vacío.".to_string()));
    }
    if clean.len() > 64 {
        return Err(ServerError::InvalidName(
            "El nombre no puede superar 64 caracteres.".to_string(),
        ));
    }
    if clean == "." || clean == ".." {
        return Err(ServerError::InvalidName("Nombre reservado.".to_string()));
    }
    let ok = clean.chars().all(|c| {
        c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.')
    });
    if !ok {
        return Err(ServerError::InvalidName(
            "Usá solo letras, números, espacios, guiones y puntos.".to_string(),
        ));
    }
    Ok(clean.to_string())
}

pub fn servers_dir(app: &AppHandle) -> Result<std::path::PathBuf> {
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| ServerError::Io(format!("No se pudo resolver el dataDir: {e}")))?;
    Ok(data.join("servers"))
}

/// Misma resolución con ruta explícita (para tests sin Tauri).
#[cfg(test)]
pub fn servers_dir_at(base: &std::path::Path) -> std::path::PathBuf {
    base.join("servers")
}

pub fn list_servers_at(dir: &std::path::Path) -> Result<Vec<ServerInfo>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    let entries = std::fs::read_dir(dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta_path = path.join(SIDECAR);
        if !meta_path.is_file() {
            continue; // Sin sidecar: no es un server de Digspawn (import = hito posterior).
        }
        let raw = std::fs::read_to_string(&meta_path)?;
        let meta: ServerMeta = serde_json::from_str(&raw)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        out.push(ServerInfo {
            name,
            server_type: meta.server_type,
            version: meta.version,
            ram_mb: meta.ram_mb,
            state: "stopped".to_string(),
        });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

pub fn list_servers(app: &AppHandle) -> Result<Vec<ServerInfo>> {
    let dir = servers_dir(app)?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    list_servers_at(&dir)
}

pub fn delete_server(app: &AppHandle, name: &str) -> Result<()> {
    let clean = validate_name(name)?;
    let dir = servers_dir(app)?;
    let target = dir.join(&clean);
    if !target.is_dir() || target.join(SIDECAR).is_file() == false {
        return Err(ServerError::NotFound(format!("No existe el server \"{clean}\".")));
    }
    std::fs::remove_dir_all(&target)?;
    Ok(())
}

/// RAM física del host en MB (guard rail del slider).
/// OJO: `System::new_all()` tarda ~400ms (enumera todos los procesos);
/// acá solo nos interesa la memoria, así que se usa `new()` + `refresh_memory()`.
pub fn host_ram_mb() -> Result<u64> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let mb = sys.total_memory() / 1024 / 1024;
    if mb == 0 {
        return Err(ServerError::Io("No se pudo leer la RAM del host.".to_string()));
    }
    Ok(mb)
}

/// IPs locales IPv4 (sin loopback) para mostrarle al pana cuál pasarle
/// a sus amigos (LAN o la de ZeroTier/Radmin).
pub fn local_ips() -> Vec<String> {
    let nets = sysinfo::Networks::new_with_refreshed_list();
    let mut out = vec![];
    for (_iface, net) in &nets {
        for ipnet in net.ip_networks() {
            if let std::net::IpAddr::V4(v4) = ipnet.addr {
                if !v4.is_loopback() {
                    let s = v4.to_string();
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Icono custom (icon.png del layout del SPEC).
// ---------------------------------------------------------------------------

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const MAX_ICON_BYTES: usize = 1024 * 1024;

/// Valida y guarda el icon.png de un server. `data_url` es lo que da el
/// `<input type=file>` del frontend ("data:image/png;base64,...").
pub fn set_icon_at(dir: &std::path::Path, data_url: &str) -> Result<()> {
    let b64 = data_url
        .split_once(',')
        .map(|(_, b)| b)
        .unwrap_or(data_url);
    if !data_url.starts_with("data:image/png") && !looks_like_base64(b64) {
        return Err(ServerError::InvalidName("El icono tiene que ser un PNG.".to_string()));
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| ServerError::InvalidName("El icono no es un PNG válido.".to_string()))?;
    if bytes.len() > MAX_ICON_BYTES {
        return Err(ServerError::InvalidName("El icono no puede superar 1 MB.".to_string()));
    }
    if bytes.len() < PNG_MAGIC.len() || &bytes[..PNG_MAGIC.len()] != PNG_MAGIC {
        return Err(ServerError::InvalidName("El icono tiene que ser un PNG.".to_string()));
    }
    std::fs::write(dir.join("icon.png"), bytes)?;
    Ok(())
}

fn looks_like_base64(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && s.len() % 4 == 0
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
}

/// Devuelve el icon.png como data URL, o None si usa el default.
pub fn get_icon_at(dir: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(dir.join("icon.png")).ok()?;
    if bytes.len() < PNG_MAGIC.len() || &bytes[..PNG_MAGIC.len()] != PNG_MAGIC {
        return None;
    }
    use base64::Engine;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

pub fn set_icon(app: &AppHandle, name: &str, data_url: &str) -> Result<()> {
    let clean = validate_name(name)?;
    let target = servers_dir(app)?.join(&clean);
    if !target.is_dir() {
        return Err(ServerError::NotFound(format!("No existe el server \"{clean}\".")));
    }
    set_icon_at(&target, data_url)
}

pub fn get_icon(app: &AppHandle, name: &str) -> Result<Option<String>> {
    let clean = validate_name(name)?;
    Ok(get_icon_at(&servers_dir(app)?.join(&clean)))
}

async fn download_to(
    url: &str,
    server: &str,
    dest: &std::path::Path,
    on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("digspawn/0.1.0")
        .build()
        .map_err(|e| ServerError::DownloadFailed(format!("No se pudo crear cliente HTTP: {e}")))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| ServerError::DownloadFailed(format!("No se pudo bajar el jar ({server}): {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::DownloadFailed(format!(
            "No se pudo bajar el jar ({server}): el servidor respondió {}",
            resp.status()
        )));
    }
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| ServerError::DownloadFailed(format!("Descarga interrumpida ({server}): {e}")))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    Ok(())
}

/// Crea un server: valida, baja el jar con progreso, firma eula, genera
/// properties + sidecar. Si algo falla a mitad de camino, limpia la carpeta.
pub async fn create_server_at(
    servers: &std::path::Path,
    input: CreateInput,
    on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<ServerInfo> {
    let name = validate_name(&input.name)?;
    let server_type = input.server_type.to_lowercase();
    if server_type != "paper" && server_type != "vanilla" {
        return Err(ServerError::InvalidName(format!(
            "Tipo desconocido: \"{}\". Válidos: paper, vanilla.",
            input.server_type
        )));
    }
    if input.version.trim().is_empty() {
        return Err(ServerError::InvalidName("Elegí una versión.".to_string()));
    }
    if !input.accept_eula {
        return Err(ServerError::EulaNotAccepted(
            "Tenés que aceptar la EULA de Minecraft para crear el server.".to_string(),
        ));
    }
    if input.ram_mb < 512 {
        return Err(ServerError::InvalidName("La RAM mínima es 512 MB.".to_string()));
    }

    let dir = servers.to_path_buf();
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(&name);
    if target.exists() {
        return Err(ServerError::AlreadyExists(format!("Ya existe un server llamado \"{name}\".")));
    }
    std::fs::create_dir_all(&target)?;

    // Resolver URL del jar ANTES de dejar basura: si falla, se limpia.
    let jar_resolved = async {
        if server_type == "paper" {
            let dl = crate::paper_api::stable_download(input.version.trim()).await?;
            Ok::<_, ServerError>((dl.url, dl.size))
        } else {
            let dl = crate::mojang_api::server_jar_url(input.version.trim()).await?;
            Ok((dl.url, dl.size))
        }
    }
    .await;
    let (url, _size) = match jar_resolved {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&target);
            return Err(e);
        }
    };

    if let Err(e) = download_to(&url, &name, &target.join("server.jar"), on_progress).await {
        let _ = std::fs::remove_dir_all(&target);
        return Err(e);
    }

    // eula + properties + sidecar (fallos acá también limpian).
    let setup = (|| -> Result<()> {
        std::fs::write(target.join("eula.txt"), "eula=true\n")?;
        std::fs::write(target.join("server.properties"), crate::properties::defaults())?;
        let meta = ServerMeta {
            server_type: server_type.clone(),
            version: input.version.trim().to_string(),
            ram_mb: input.ram_mb,
        };
        std::fs::write(target.join(SIDECAR), serde_json::to_string_pretty(&meta)?)?;
        Ok(())
    })();
    if let Err(e) = setup {
        let _ = std::fs::remove_dir_all(&target);
        return Err(e);
    }

    Ok(ServerInfo {
        name,
        server_type,
        version: input.version.trim().to_string(),
        ram_mb: input.ram_mb,
        state: "stopped".to_string(),
    })
}

// ---------------------------------------------------------------------------
// Importar server existente: copia jar + config + mundos (sin logs) y
// escribe el sidecar. El original queda intacto.
// ---------------------------------------------------------------------------

/// Carpetas que no se copian al importar (logs propios + caches).
const IMPORT_SKIP: &[&str] = &["logs", "crashlogs", "cache"];

#[derive(Debug, Clone, Deserialize)]
pub struct ImportInput {
    /// Carpeta origen (la que tiene el server.jar).
    pub path: String,
    pub name: String,
    pub server_type: String,
    pub version: String,
    pub ram_mb: u64,
    pub accept_eula: bool,
}

fn copy_dir_filtered(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            if IMPORT_SKIP.contains(&name.as_str()) {
                continue;
            }
            copy_dir_filtered(&from, &to)?;
        } else {
            if name == SIDECAR {
                continue; // el sidecar se regenera abajo
            }
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Núcleo testeable con dirs explícitos.
pub fn import_server_at(
    servers: &std::path::Path,
    input: ImportInput,
) -> Result<ServerInfo> {
    let src = std::path::Path::new(&input.path);
    if !src.is_dir() {
        return Err(ServerError::NotFound("Esa carpeta no existe.".to_string()));
    }
    if !src.join("server.jar").is_file() {
        return Err(ServerError::NotFound(
            "Ahí no hay un server.jar. Elegí la carpeta del server.".to_string(),
        ));
    }
    let name = validate_name(&input.name)?;
    let server_type = input.server_type.to_lowercase();
    if server_type != "paper" && server_type != "vanilla" {
        return Err(ServerError::InvalidName(
            "Tipo desconocido. Válidos: paper, vanilla.".to_string(),
        ));
    }
    if input.version.trim().is_empty() {
        return Err(ServerError::InvalidName("Elegí una versión.".to_string()));
    }
    if input.ram_mb < 512 {
        return Err(ServerError::InvalidName("La RAM mínima es 512 MB.".to_string()));
    }
    let target = servers.join(&name);
    if target.exists() {
        return Err(ServerError::AlreadyExists(format!("Ya existe un server llamado \"{name}\".")));
    }
    // EULA: si el importado no la trae firmada, pedir consentimiento.
    let eula_ok = std::fs::read_to_string(src.join("eula.txt"))
        .map(|c| c.lines().any(|l| l.trim() == "eula=true"))
        .unwrap_or(false);
    if !eula_ok && !input.accept_eula {
        return Err(ServerError::EulaNotAccepted(
            "Ese server no trae la EULA firmada: aceptala para importarlo.".to_string(),
        ));
    }

    std::fs::create_dir_all(&target)?;
    let setup = (|| -> Result<()> {
        copy_dir_filtered(src, &target)?;
        if !target.join("server.properties").is_file() {
            std::fs::write(target.join("server.properties"), crate::properties::defaults())?;
        }
        if !eula_ok {
            std::fs::write(target.join("eula.txt"), "eula=true\n")?;
        }
        let meta = ServerMeta {
            server_type: server_type.clone(),
            version: input.version.trim().to_string(),
            ram_mb: input.ram_mb,
        };
        std::fs::write(target.join(SIDECAR), serde_json::to_string_pretty(&meta)?)?;
        Ok(())
    })();
    if let Err(e) = setup {
        let _ = std::fs::remove_dir_all(&target);
        return Err(e);
    }
    Ok(ServerInfo {
        name,
        server_type,
        version: input.version.trim().to_string(),
        ram_mb: input.ram_mb,
        state: "stopped".to_string(),
    })
}

pub fn import_server(app: &AppHandle, input: ImportInput) -> Result<ServerInfo> {
    let dir = servers_dir(app)?;
    std::fs::create_dir_all(&dir)?;
    import_server_at(&dir, input)
}

/// Wrapper Tauri: resuelve el dir real y emite `download-progress`.
pub async fn create_server(app: &AppHandle, input: CreateInput) -> Result<ServerInfo> {
    let dir = servers_dir(app)?;
    let name_for_event = input.name.trim().to_string();
    let app_emit = app.clone();
    let on_progress = move |downloaded: u64, total: Option<u64>| {
        let pct = total.filter(|t| *t > 0).map(|t| downloaded as f64 / t as f64 * 100.0);
        let _ = app_emit.emit(
            "download-progress",
            DownloadProgress {
                server: name_for_event.clone(),
                downloaded,
                total,
                pct,
            },
        );
    };
    create_server_at(&dir, input, &on_progress).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png_data_url() -> String {
        // PNG de 1x1 (válido): magic + IHDR mínimo.
        let mut bytes = PNG_MAGIC.to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 13, 73, 72, 68, 82]);
        use base64::Engine;
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        )
    }

    #[test]
    fn icon_roundtrip_and_rejections() {
        let tmp = std::env::temp_dir().join("digspawn-test-icon");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(get_icon_at(&tmp).is_none());
        set_icon_at(&tmp, &tiny_png_data_url()).unwrap();
        let back = get_icon_at(&tmp).expect("debe volver como data URL");
        assert!(back.starts_with("data:image/png;base64,"));
        assert!(set_icon_at(&tmp, "data:image/png;base64,!!!no-base64!!!").is_err());
        assert!(set_icon_at(&tmp, "hola").is_err());
        // JPEG con prefijo png: falla por magic.
        use base64::Engine;
        let fake_jpg = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b"\xFF\xD8\xFF not png")
        );
        assert!(set_icon_at(&tmp, &fake_jpg).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn accepts_sane_names() {
        assert_eq!(validate_name("  Mi Server 1 ").unwrap(), "Mi Server 1");
        assert_eq!(validate_name("server-test_v2.0").unwrap(), "server-test_v2.0");
    }

    #[test]
    fn local_ips_are_valid_v4_no_loopback() {
        for ip in local_ips() {
            let addr: std::net::IpAddr = ip.parse().expect("IP válida");
            assert!(addr.is_ipv4(), "{ip} no es v4");
            assert!(!addr.is_loopback(), "{ip} es loopback");
        }
    }

    #[test]
    fn import_copies_all_but_logs_and_signs_eula() {
        let tmp = std::env::temp_dir().join("digspawn-test-import");
        let _ = std::fs::remove_dir_all(&tmp);
        let src = tmp.join("origen");
        std::fs::create_dir_all(src.join("world")).unwrap();
        std::fs::create_dir_all(src.join("logs")).unwrap();
        std::fs::write(src.join("server.jar"), b"fake-jar").unwrap();
        std::fs::write(src.join("server.properties"), "motd=Viejo\n").unwrap();
        std::fs::write(src.join("eula.txt"), "eula=false\n").unwrap();
        std::fs::write(src.join("world").join("level.dat"), b"data").unwrap();
        std::fs::write(src.join("logs").join("latest.log"), b"basura").unwrap();
        let servers = tmp.join("servers");

        // Sin aceptar eula y sin eula firmada: falla y no deja nada.
        let err = import_server_at(
            &servers,
            ImportInput {
                path: src.to_string_lossy().into_owned(),
                name: "Importado".to_string(),
                server_type: "paper".to_string(),
                version: "26.2".to_string(),
                ram_mb: 2048,
                accept_eula: false,
            },
        )
        .expect_err("debe pedir eula");
        assert!(matches!(err, ServerError::EulaNotAccepted(_)));
        assert!(!servers.join("Importado").exists());

        // Aceptando: copia todo menos logs, firma eula, escribe sidecar.
        let info = import_server_at(
            &servers,
            ImportInput {
                path: src.to_string_lossy().into_owned(),
                name: "Importado".to_string(),
                server_type: "paper".to_string(),
                version: "26.2".to_string(),
                ram_mb: 1024,
                accept_eula: true,
            },
        )
        .expect("importar");
        assert_eq!(info.name, "Importado");
        let t = servers.join("Importado");
        assert!(t.join("server.jar").is_file());
        assert!(t.join("world").join("level.dat").is_file());
        assert!(!t.join("logs").exists(), "los logs no se copian");
        assert_eq!(std::fs::read_to_string(t.join("eula.txt")).unwrap(), "eula=true\n");
        assert!(std::fs::read_to_string(t.join("server.properties")).unwrap().contains("motd=Viejo"));
        let listed = list_servers_at(&servers).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].ram_mb, 1024);

        // Sin server.jar no es importable.
        let err = import_server_at(
            &servers,
            ImportInput {
                path: tmp.to_string_lossy().into_owned(),
                name: "Otro".to_string(),
                server_type: "vanilla".to_string(),
                version: "26.2".to_string(),
                ram_mb: 2048,
                accept_eula: true,
            },
        )
        .expect_err("sin jar no importa");
        assert!(matches!(err, ServerError::NotFound(_)));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rejects_bad_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("a\\b").is_err());
        assert!(validate_name("con/slash").is_err());
        assert!(validate_name(&"x".repeat(65)).is_err());
    }

    #[test]
    fn empty_dir_lists_empty() {
        let tmp = std::env::temp_dir().join("digspawn-test-empty");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let servers = list_servers_at(&servers_dir_at(&tmp)).unwrap();
        assert!(servers.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn lists_sidecar_servers_sorted() {
        let tmp = std::env::temp_dir().join("digspawn-test-list");
        let _ = std::fs::remove_dir_all(&tmp);
        let dir = servers_dir_at(&tmp);
        for (n, t, v) in [("zeta", "paper", "26.2"), ("alpha", "vanilla", "26.2")] {
            let d = dir.join(n);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join(SIDECAR),
                serde_json::to_string(&ServerMeta {
                    server_type: t.into(),
                    version: v.into(),
                    ram_mb: 2048,
                })
                .unwrap(),
            )
            .unwrap();
        }
        // Carpeta sin sidecar: se ignora.
        std::fs::create_dir_all(dir.join("suelto")).unwrap();
        let servers = list_servers_at(&dir).unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "alpha");
        assert_eq!(servers[1].name, "zeta");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn host_ram_is_sane() {
        let mb = host_ram_mb().unwrap();
        assert!(mb >= 512, "RAM sospechosa: {mb} MB");
    }

    /// Requiere red. Crea un server Paper REAL en un dir temporal (~65MB).
    /// Es la verificación punta a punta del hito 2.
    #[test]
    #[ignore = "needs-network-heavy"]
    fn live_creates_real_paper_server() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let tmp = std::env::temp_dir().join("digspawn-test-create");
        let _ = std::fs::remove_dir_all(&tmp);
        let servers = servers_dir_at(&tmp);
        std::fs::create_dir_all(&servers).unwrap();

        let vs = rt.block_on(crate::paper_api::list_versions()).unwrap();
        let info = rt
            .block_on(create_server_at(
                &servers,
                CreateInput {
                    name: "Pana Test".to_string(),
                    server_type: "paper".to_string(),
                    version: vs[0].id.clone(),
                    ram_mb: 2048,
                    accept_eula: true,
                },
                &|_, _| {},
            ))
            .expect("crear server Paper real");
        assert_eq!(info.name, "Pana Test");

        let target = servers.join("Pana Test");
        assert!(target.join("server.jar").is_file(), "falta server.jar");
        assert!(
            target.join("server.jar").metadata().unwrap().len() > 10_000_000,
            "jar sospechosamente chico"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("eula.txt")).unwrap(),
            "eula=true\n"
        );
        assert!(target.join("server.properties").is_file());
        assert!(target.join(SIDECAR).is_file());

        let listed = list_servers_at(&servers).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].server_type, "paper");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Sin EULA aceptada no se crea nada.
    #[test]
    fn create_requires_eula() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let tmp = std::env::temp_dir().join("digspawn-test-noeula");
        let _ = std::fs::remove_dir_all(&tmp);
        let servers = servers_dir_at(&tmp);
        std::fs::create_dir_all(&servers).unwrap();
        let err = rt
            .block_on(create_server_at(
                &servers,
                CreateInput {
                    name: "Sin Eula".to_string(),
                    server_type: "paper".to_string(),
                    version: "26.2".to_string(),
                    ram_mb: 2048,
                    accept_eula: false,
                },
                &|_, _| {},
            ))
            .expect_err("debe fallar sin EULA");
        assert!(matches!(err, ServerError::EulaNotAccepted(_)));
        assert!(!servers.join("Sin Eula").exists(), "no debe dejar basura");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
