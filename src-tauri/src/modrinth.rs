// Modrinth — buscador + instalador in-app de PLUGINS (solo Bukkit/Paper).
// NADA de mods (Fabric/Forge/...) en este hito. Solo Modrinth (no CurseForge/Hangar).
// La gestión de instalados (list/import/delete/toggle) vive en plugins.rs.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::errors::{Result, ServerError};

const BASE: &str = "https://api.modrinth.com/v2";
const USER_AGENT: &str = concat!("digspawn/", env!("CARGO_PKG_VERSION"));

/// Loaders que corre un server Paper.
const PAPER_LOADERS: &[&str] = &["paper", "spigot", "bukkit"];

/// Tope de la cascada de dependencias.
const MAX_DEP_DEPTH: u32 = 3;

fn client() -> std::result::Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder().user_agent(USER_AGENT).build()
}

// ---------------------------------------------------------------------------
// Search.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub project_id: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub downloads: u64,
    pub icon_url: Option<String>,
    /// Game versions que declara (para el badge "compatible con tu X").
    pub game_versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResp {
    hits: Vec<SearchHitRaw>,
}

#[derive(Debug, Deserialize)]
struct SearchHitRaw {
    project_id: String,
    title: String,
    author: String,
    description: String,
    downloads: u64,
    icon_url: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    versions: Vec<String>,
}

/// Busca plugins. Facet por loaders (OR) + verificación cliente.
/// NO se filtra por project_type: populares como LuckPerms figuran como
/// "mod" con loaders paper/bukkit/spigot (caza de Hermes).
/// Query vacía = explorar: devuelve los más descargados (para cuando no
/// sabés qué buscar). `category` = slug de Modrinth (economy, minigame...).
pub async fn search(query: &str, category: Option<&str>) -> Result<Vec<SearchHit>> {
    let q = query.trim();
    // Facets: OR de loaders dentro de un array (AND entre arrays).
    let facets = BriefFacets::for_search(category);
    let resp = client()
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: {e}")))?
        .get(format!("{BASE}/search"))
        .query(&[("query", q), ("limit", "25"), ("facets", &facets), ("index", result_order(q))])
        .send()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: no se pudo buscar: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::VersionsFailed(format!(
            "Modrinth respondió {}",
            resp.status()
        )));
    }
    let body: SearchResp = resp
        .json()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: respuesta inválida: {e}")))?;
    Ok(body
        .hits
        .into_iter()
        .filter(|h| h.categories.iter().any(|c| PAPER_LOADERS.contains(&c.as_str())))
        .map(|h| SearchHit {
            project_id: h.project_id,
            title: h.title,
            author: h.author,
            description: h.description,
            downloads: h.downloads,
            icon_url: h.icon_url,
            game_versions: h.versions,
        })
        .collect())
}

/// Orden de Modrinth: con texto manda relevancia; vacío = más descargados.
fn result_order(query: &str) -> &'static str {
    if query.trim().is_empty() {
        "downloads"
    } else {
        "relevance"
    }
}

// ---------------------------------------------------------------------------
// Detalle de proyecto (modal): descripción larga + galería.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct GalleryItem {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetails {
    pub project_id: String,
    pub title: String,
    pub body: String,
    pub gallery: Vec<GalleryItem>,
}

#[derive(Debug, Deserialize)]
struct ProjectRaw {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    gallery: Vec<GalleryRaw>,
}

#[derive(Debug, Deserialize)]
struct GalleryRaw {
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: String,
}

/// Detalle para el modal. Galería topada (las imágenes pesan; se cargan
/// solo al abrir y se liberan al cerrar).
pub async fn details(project_id: &str) -> Result<ProjectDetails> {
    const MAX_IMAGES: usize = 8;
    let id = project_id.trim();
    if id.is_empty() {
        return Err(ServerError::InvalidName("Proyecto vacío.".to_string()));
    }
    let resp = client()
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: {e}")))?
        .get(format!("{BASE}/project/{id}"))
        .send()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: no se pudo traer el detalle: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::VersionsFailed(format!(
            "Modrinth respondió {}",
            resp.status()
        )));
    }
    let raw: ProjectRaw = resp
        .json()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: respuesta inválida: {e}")))?;
    // La API ya trae las destacadas primero; tope duro (las imágenes pesan
    // y el modal las carga solo al abrir).
    let gallery: Vec<GalleryItem> = raw
        .gallery
        .into_iter()
        .filter(|g| !g.url.is_empty())
        .take(MAX_IMAGES)
        .map(|g| GalleryItem { url: g.url, title: g.title })
        .collect();
    Ok(ProjectDetails {
        project_id: raw.id,
        title: raw.title,
        body: raw.body,
        gallery,
    })
}

struct BriefFacets;
impl BriefFacets {
    fn loaders_inner() -> String {
        // "categories:paper","categories:spigot","categories:bukkit"
        PAPER_LOADERS
            .iter()
            .map(|l| format!("\"categories:{l}\""))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn loaders() -> String {
        // [["categories:paper","categories:spigot","categories:bukkit"]]
        // (OR adentro, AND entre arrays de afuera).
        format!("[[{}]]", Self::loaders_inner())
    }

    /// Facets de búsqueda: loaders AND categoría opcional.
    fn for_search(category: Option<&str>) -> String {
        let cat = category.map(str::trim).filter(|c| !c.is_empty()).unwrap_or("");
        // Whitelist chica de slugs válidos (un slug raro devuelve vacío).
        const VALID: &[&str] = &[
            "adventure",
            "economy",
            "library",
            "magic",
            "management",
            "minigame",
            "social",
            "technology",
            "utility",
            "misc",
        ];
        if VALID.contains(&cat) {
            format!("[[{}],[\"categories:{cat}\"]]", Self::loaders_inner())
        } else {
            Self::loaders()
        }
    }
}

// ---------------------------------------------------------------------------
// Resolve + install.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
struct VersionEntry {
    id: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    files: Vec<VersionFile>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize, Clone)]
struct VersionFile {
    #[serde(default)]
    url: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
struct Dependency {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    dependency_type: String,
}

/// Versión resuelta: la que se baja.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub version_id: String,
    pub version_label: String,
    pub file_url: String,
    pub filename: String,
    pub optional_deps: Vec<String>, // project_ids opcionales (solo se mencionan)
}

async fn fetch_versions(project_id: &str, mc_version: &str) -> Result<Vec<VersionEntry>> {
    let loaders = serde_json::to_string(PAPER_LOADERS)
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: {e}")))?;
    let game_versions = serde_json::to_string(&[mc_version])
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: {e}")))?;
    let resp = client()
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: {e}")))?
        .get(format!("{BASE}/project/{project_id}/version"))
        .query(&[
            ("loaders", loaders.as_str()),
            ("game_versions", game_versions.as_str()),
            ("limit", "20"),
        ])
        .send()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::VersionsFailed(format!(
            "Modrinth respondió {}",
            resp.status()
        )));
    }
    resp.json()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Modrinth: respuesta inválida: {e}")))
}

fn compatible(v: &VersionEntry, mc_version: &str) -> bool {
    v.loaders.iter().any(|l| PAPER_LOADERS.contains(&l.as_str()))
        && v.game_versions.iter().any(|g| g == mc_version)
}

fn primary_file(v: &VersionEntry) -> Option<&VersionFile> {
    v.files.iter().find(|f| f.primary && !f.url.is_empty()).or_else(|| {
        v.files.iter().find(|f| {
            !f.url.is_empty() && f.filename.to_lowercase().ends_with(".jar")
        })
    })
}

/// Resuelve la versión a instalar: compatible + release primero.
/// La compat se exige acá para el raíz Y para cada dep de la cascada.
fn pick_version(versions: &[VersionEntry], mc_version: &str, what: &str) -> Result<VersionEntry> {
    let mut compat: Vec<&VersionEntry> =
        versions.iter().filter(|v| compatible(v, mc_version)).collect();
    if compat.is_empty() {
        return Err(ServerError::DownloadFailed(format!(
            "{what}: no hay versión compatible con {mc_version} (loaders paper/spigot/bukkit)."
        )));
    }
    // Release primero; si no hay, la primera compatible (avisada por version_label).
    compat.sort_by_key(|v| {
        if v.version_type.eq_ignore_ascii_case("release") {
            0
        } else {
            1
        }
    });
    let chosen = compat[0].clone();
    if primary_file(&chosen).is_none() {
        return Err(ServerError::DownloadFailed(format!(
            "{what}: la versión {} no trae .jar descargable.",
            chosen.version_number
        )));
    }
    Ok(chosen)
}

pub async fn resolve(project_id: &str, mc_version: &str) -> Result<Resolved> {
    let versions = fetch_versions(project_id, mc_version).await?;
    let chosen = pick_version(&versions, mc_version, &format!("El proyecto {project_id}"))?;
    let file = primary_file(&chosen).expect("chequeado");
    let optional_deps = chosen
        .dependencies
        .iter()
        .filter(|d| d.dependency_type == "optional")
        .filter_map(|d| d.project_id.clone())
        .collect();
    Ok(Resolved {
        version_id: chosen.id.clone(),
        version_label: if chosen.version_number.is_empty() {
            chosen.id.clone()
        } else {
            chosen.version_number.clone()
        },
        file_url: file.url.clone(),
        filename: file.filename.clone(),
        optional_deps,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallReport {
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
    pub optional_deps: Vec<String>,
    pub version_label: String,
}

/// Instala un proyecto + sus dependencias required en cascada.
/// `mc_version` = versión MC exacta del server (filtro duro).
pub async fn install_at(
    server_dir: &Path,
    project_id: &str,
    mc_version: &str,
    on_progress: &(dyn Fn(String, u64, Option<u64>) + Send + Sync),
) -> Result<InstallReport> {
    let mut installed = vec![];
    let mut skipped = vec![];
    let mut seen: HashSet<String> = HashSet::new();
    let mut optional_all: HashSet<String> = HashSet::new();
    let mut queue: Vec<(String, u32)> = vec![(project_id.to_string(), 0)];
    let mut root_label = String::new();

    while let Some((pid, depth)) = queue.pop() {
        if !seen.insert(pid.clone()) {
            continue; // anti-ciclos
        }
        if depth > MAX_DEP_DEPTH {
            return Err(ServerError::DownloadFailed(format!(
                "Dependencias muy profundas para {pid} (tope {MAX_DEP_DEPTH})."
            )));
        }
        let versions = fetch_versions(&pid, mc_version).await?;
        let chosen = pick_version(&versions, mc_version, &format!("La dependencia {pid}"))?;
        let file = primary_file(&chosen).expect("chequeado").clone();
        if root_label.is_empty() && depth == 0 {
            root_label = if chosen.version_number.is_empty() {
                chosen.id.clone()
            } else {
                chosen.version_number.clone()
            };
        }
        for d in &chosen.dependencies {
            match d.dependency_type.as_str() {
                "required" => {
                    if let Some(dep_id) = &d.project_id {
                        queue.push((dep_id.clone(), depth + 1));
                    }
                }
                "optional" => {
                    if let Some(dep_id) = &d.project_id {
                        optional_all.insert(dep_id.clone());
                    }
                }
                _ => {}
            }
        }
        // Descargar (saltear si el archivo ya está).
        let dest = server_dir.join("plugins").join(&file.filename);
        if dest.is_file() {
            skipped.push(file.filename.clone());
            continue;
        }
        download_jar(&file.url, &file.filename, &dest, &|dl, total| {
            on_progress(file.filename.clone(), dl, total)
        })
        .await?;
        installed.push(file.filename.clone());
    }

    Ok(InstallReport {
        installed,
        skipped,
        optional_deps: optional_all.into_iter().collect(),
        version_label: root_label,
    })
}

async fn download_jar(
    url: &str,
    filename: &str,
    dest: &Path,
    on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<()> {
    let client = client()
        .map_err(|e| ServerError::DownloadFailed(format!("Modrinth: {e}")))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| ServerError::DownloadFailed(format!("No se pudo bajar {filename}: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::DownloadFailed(format!(
            "No se pudo bajar {filename}: http {}",
            resp.status()
        )));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let total = resp.content_length();
    let tmp = dest.with_extension("jar.tmp");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|e| ServerError::DownloadFailed(format!("Descarga interrumpida ({filename}): {e}")))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    drop(file);
    // Validar ZIP magic antes de mover a plugins/.
    let head = std::fs::read(&tmp)?;
    if head.len() < 4 || &head[..4] != b"PK\x03\x04" {
        let _ = std::fs::remove_file(&tmp);
        return Err(ServerError::DownloadFailed(format!(
            "{filename}: lo bajado no es un .jar válido."
        )));
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_VERSIONS: &str = r#"[
        {"id":"v-beta","version_number":"2.0-beta","version_type":"beta",
         "game_versions":["26.2"],"loaders":["paper"],
         "files":[{"url":"https://cdn.example/a.jar","filename":"a-2.0.jar","primary":true}],
         "dependencies":[]},
        {"id":"v-rel","version_number":"1.9","version_type":"release",
         "game_versions":["26.2"],"loaders":["spigot"],
         "files":[{"url":"https://cdn.example/a.jar","filename":"a-1.9.jar","primary":true}],
         "dependencies":[{"project_id":"dep1","dependency_type":"required"},{"project_id":"opt1","dependency_type":"optional"}]},
        {"id":"v-old","version_number":"1.0","version_type":"release",
         "game_versions":["1.20.4"],"loaders":["paper"],
         "files":[{"url":"https://cdn.example/a.jar","filename":"a-1.0.jar","primary":true}],
         "dependencies":[]},
        {"id":"v-fabric","version_number":"1.0f","version_type":"release",
         "game_versions":["26.2"],"loaders":["fabric"],
         "files":[{"url":"https://cdn.example/a.jar","filename":"a-f.jar","primary":true}],
         "dependencies":[]}
    ]"#;

    fn fixture() -> Vec<VersionEntry> {
        serde_json::from_str(FIXTURE_VERSIONS).unwrap()
    }

    #[test]
    fn picks_release_over_beta() {
        let chosen = pick_version(&fixture(), "26.2", "test").unwrap();
        assert_eq!(chosen.id, "v-rel");
    }

    #[test]
    fn rejects_wrong_game_version_and_fabric() {
        // 1.20.4: solo v-old califica.
        let chosen = pick_version(&fixture(), "1.20.4", "test").unwrap();
        assert_eq!(chosen.id, "v-old");
        // Sin match: error rico.
        assert!(pick_version(&fixture(), "1.19.4", "test").is_err());
    }

    #[test]
    fn facets_use_loader_or() {
        let f = BriefFacets::loaders();
        assert!(f.contains("categories:paper"));
        assert!(f.contains("categories:spigot"));
        assert!(f.contains("categories:bukkit"));
        assert!(f.starts_with("[[") && f.ends_with("]]"));
    }

    #[test]
    fn empty_query_sorts_by_downloads() {
        assert_eq!(result_order(""), "downloads");
        assert_eq!(result_order("   "), "downloads");
        assert_eq!(result_order("essentials"), "relevance");
    }

    #[test]
    fn category_facet_ands_with_loaders() {
        let f = BriefFacets::for_search(Some("economy"));
        assert!(f.contains("categories:paper"));
        assert!(f.contains("\"categories:economy\""));
        // Sin categoría o con slug inválido: solo loaders.
        assert_eq!(BriefFacets::for_search(None), BriefFacets::loaders());
        assert_eq!(BriefFacets::for_search(Some("no-existe")), BriefFacets::loaders());
        assert_eq!(BriefFacets::for_search(Some("  ")), BriefFacets::loaders());
    }

    #[test]
    fn details_parses_body_and_gallery() {
        let raw: ProjectRaw = serde_json::from_str(
            r##"{"id":"abc","title":"Demo","body":"# Hola","gallery":[
                {"url":"https://x/1.png","title":"uno"},
                {"url":"","title":"rota"},
                {"url":"https://x/2.png","title":""}
            ]}"##,
        )
        .unwrap();
        assert_eq!(raw.gallery.len(), 3);
        // El filtrado de vacías vive en details(); acá se chequea el parse.
        assert_eq!(raw.gallery[0].title, "uno");
    }

    /// Requiere red. Busca un plugin real.
    #[test]
    #[ignore = "needs-network"]
    fn live_searches_luckperms() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let hits = rt.block_on(search("luckperms", None)).expect("Modrinth debe responder");
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|h| h.title.to_lowercase().contains("luckperms")));
    }

    /// Requiere red. Resuelve versión paper/26.2 real.
    #[test]
    #[ignore = "needs-network"]
    fn live_resolves_paper_26_2() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let r = rt
            .block_on(resolve("Vebnzrzj", "26.2"))
            .expect("debe resolver LuckPerms paper 26.2");
        assert!(r.filename.to_lowercase().ends_with(".jar"));
        assert!(r.file_url.starts_with("https://"));
    }

    /// Requiere red. Instala LuckPerms de verdad en dir temporal (~13MB).
    #[test]
    #[ignore = "needs-network-heavy"]
    fn live_installs_to_temp_plugins() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let tmp = std::env::temp_dir().join("digspawn-test-modrinth");
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).unwrap();
            let rep = install_at(&tmp, "Vebnzrzj", "26.2", &|_, _, _| {})
                .await
                .expect("instalar LuckPerms");
            assert_eq!(rep.installed.len(), 1);
            let jar = tmp.join("plugins").join(&rep.installed[0]);
            assert!(jar.is_file());
            assert!(jar.metadata().unwrap().len() > 1_000_000);
            let _ = std::fs::remove_dir_all(&tmp);
        });
    }

    /// Playbook del hito: Paper real + LuckPerms instalado + boot hasta ver
    /// que el plugin carga + stop limpio. Pesado (~2-4 min).
    #[test]
    #[ignore = "needs-network-heavy"]
    fn live_plugin_boot_loads() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            use tokio::io::AsyncBufReadExt;
            let tmp = std::env::temp_dir().join("digspawn-test-pluginboot");
            let _ = std::fs::remove_dir_all(&tmp);
            let servers = tmp.join("servers");
            std::fs::create_dir_all(&servers).unwrap();

            let vs = crate::paper_api::list_versions().await.unwrap();
            let info = crate::server_manager::create_server_at(
                &servers,
                crate::server_manager::CreateInput {
                    name: "Plugin Test".to_string(),
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

            let rep = install_at(&dir, "Vebnzrzj", &info.version, &|_, _, _| {})
                .await
                .expect("instalar LuckPerms");
            assert!(!rep.installed.is_empty());

            let required =
                crate::runtime::required_java_for(&info.server_type, &info.version).await;
            let java = crate::runtime::ensure_runtime_at(&tmp, required, &|_, _| {})
                .await
                .expect("java");
            let mut child = crate::processes::spawn_process(&java, &dir, 2048)
                .await
                .expect("spawn");
            let stdout = child.stdout.take().unwrap();
            let stdin = child.stdin.take().unwrap();
            let mut lines = tokio::io::BufReader::new(stdout).lines();

            // Esperar Done + evidencia de que LuckPerms cargó.
            let mut done = false;
            let mut lp = false;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
            while tokio::time::Instant::now() < deadline && !(done && lp) {
                if let Ok(Some(status)) = child.try_wait() {
                    panic!("el proceso murió antes de tiempo (exit: {status})");
                }
                let line = tokio::time::timeout(std::time::Duration::from_secs(10), lines.next_line())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap_or_default();
                if line.contains("Done (") {
                    done = true;
                }
                if line.contains("LuckPerms") && line.contains("Enabling") {
                    lp = true;
                }
            }
            assert!(done, "nunca mostró Done");
            assert!(lp, "LuckPerms no mostró Enabling en el log");

            {
                use tokio::io::AsyncWriteExt;
                let mut stdin = stdin;
                stdin.write_all(b"stop\n").await.unwrap();
                stdin.flush().await.unwrap();
            }
            let status = tokio::time::timeout(std::time::Duration::from_secs(60), child.wait())
                .await
                .expect("stop")
                .unwrap();
            assert!(status.success());
            let _ = std::fs::remove_dir_all(&tmp);
        });
    }
}
