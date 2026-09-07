// PaperAPIClient — Fill v3 (`fill.papermc.io`).
// La API v2 (`api.papermc.io/v2`) está sunset desde jul-2026 (devuelve
// `{"ok":false,"error":"sunset"}`), por eso se usa v3 directo (AGENTS.md 6).
// Fill exige un `User-Agent` válido que identifique el software.

use serde::Deserialize;

use crate::errors::{Result, ServerError};

const BASE: &str = "https://fill.papermc.io/v3/projects/paper";
const USER_AGENT: &str = "digspawn/0.1.0";

fn client() -> std::result::Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder().user_agent(USER_AGENT).build()
}

#[derive(Debug, Clone)]
pub struct PaperVersion {
    pub id: String,
    /// Java mínimo que informa Fill para esta versión (si viene).
    pub min_java: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct VersionsResp {
    versions: Vec<VersionEntry>,
}

#[derive(Debug, Deserialize)]
struct VersionEntry {
    version: VersionId,
}

#[derive(Debug, Deserialize)]
struct VersionId {
    id: String,
    #[serde(default)]
    java: Option<JavaMeta>,
}

#[derive(Debug, Deserialize)]
struct JavaMeta {
    #[serde(default)]
    version: Option<JavaVer>,
}

#[derive(Debug, Deserialize)]
struct JavaVer {
    #[serde(default)]
    minimum: Option<u32>,
}

/// Lista de versiones (newest-first, como la devuelve Fill).
pub async fn list_versions() -> Result<Vec<PaperVersion>> {
    let resp = client()
        .map_err(|e| ServerError::VersionsFailed(format!("Paper: no se pudo crear cliente HTTP: {e}")))?
        .get(format!("{BASE}/versions"))
        .send()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Paper: no se pudo listar versiones: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::VersionsFailed(format!(
            "Paper: el servidor respondió {}",
            resp.status()
        )));
    }
    let body: VersionsResp = resp
        .json()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Paper: respuesta inválida: {e}")))?;
    Ok(body
        .versions
        .into_iter()
        .map(|e| PaperVersion {
            id: e.version.id,
            min_java: e.version.java.and_then(|j| j.version).and_then(|v| v.minimum),
        })
        .collect())
}

/// Jar del último build STABLE de una versión (URL embebida, no construida).
pub struct PaperDownload {
    pub url: String,
    pub size: Option<u64>,
    pub build: u64,
}

#[derive(Debug, Deserialize)]
struct BuildEntry {
    id: u64,
    channel: String,
    downloads: std::collections::HashMap<String, BuildDownload>,
}

#[derive(Debug, Deserialize)]
struct BuildDownload {
    url: String,
    #[serde(default)]
    size: Option<u64>,
}

pub async fn stable_download(version: &str) -> Result<PaperDownload> {
    let resp = client()
        .map_err(|e| ServerError::DownloadFailed(format!("Paper: no se pudo crear cliente HTTP: {e}")))?
        .get(format!("{BASE}/versions/{version}/builds"))
        .send()
        .await
        .map_err(|e| ServerError::DownloadFailed(format!("Paper: no se pudo listar builds de {version}: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::DownloadFailed(format!(
            "Paper: el servidor respondió {} para la versión {version}",
            resp.status()
        )));
    }
    let builds: Vec<BuildEntry> = resp
        .json()
        .await
        .map_err(|e| ServerError::DownloadFailed(format!("Paper: respuesta inválida: {e}")))?;
    builds
        .into_iter()
        .filter(|b| b.channel == "STABLE")
        .filter_map(|b| {
            b.downloads.get("server:default").map(|d| PaperDownload {
                url: d.url.clone(),
                size: d.size,
                build: b.id,
            })
        })
        .max_by_key(|d| d.build)
        .ok_or_else(|| {
            ServerError::DownloadFailed(format!("Paper: la versión {version} no tiene builds STABLE"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requiere red. Verifica contra la API real (Fill v3).
    #[test]
    #[ignore = "needs-network"]
    fn live_lists_versions_newest_first() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let vs = rt.block_on(list_versions()).expect("Fill v3 debe responder");
        assert!(!vs.is_empty(), "Fill no devolvió versiones");
        assert!(
            vs[0].id >= vs[vs.len() - 1].id,
            "se espera newest-first: primero={} último={}",
            vs[0].id,
            vs[vs.len() - 1].id
        );
    }

    /// Requiere red. Resuelve el jar STABLE de la última versión.
    #[test]
    #[ignore = "needs-network"]
    fn live_resolves_stable_jar_url() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let vs = rt.block_on(list_versions()).expect("Fill v3 debe responder");
        let dl = rt
            .block_on(stable_download(&vs[0].id))
            .expect("debe haber build STABLE");
        assert!(dl.url.starts_with("https://"), "URL inesperada: {}", dl.url);
        assert!(dl.build > 0);
    }
}
