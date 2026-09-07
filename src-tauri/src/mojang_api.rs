// MojangAPI — manifest oficial para Vanilla.
// `version_manifest_v2.json` -> version-json -> `downloads.server.url`.

use serde::Deserialize;

use crate::errors::{Result, ServerError};

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone)]
pub struct VanillaVersion {
    pub id: String,
    /// "release" o "snapshot" (u otros tipos que informe Mojang).
    pub kind: String,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    versions: Vec<ManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct VersionJson {
    downloads: VersionJsonDownloads,
}

#[derive(Debug, Deserialize)]
struct VersionJsonDownloads {
    server: ServerArtifact,
}

#[derive(Debug, Deserialize)]
struct ServerArtifact {
    url: String,
    #[serde(default)]
    size: Option<u64>,
}

fn client() -> std::result::Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder().user_agent("digspawn/0.1.0").build()
}

async fn fetch_manifest() -> Result<Manifest> {
    let resp = client()
        .map_err(|e| ServerError::VersionsFailed(format!("Mojang: no se pudo crear cliente HTTP: {e}")))?
        .get(MANIFEST)
        .send()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Mojang: no se pudo leer el manifest: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::VersionsFailed(format!(
            "Mojang: el servidor respondió {}",
            resp.status()
        )));
    }
    resp.json()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("Mojang: respuesta inválida: {e}")))
}

/// Versiones Vanilla (el manifest ya viene newest-first).
/// `include_snapshots=false` -> solo `release` (default del wizard).
pub async fn list_versions(include_snapshots: bool) -> Result<Vec<VanillaVersion>> {
    let m = fetch_manifest().await?;
    Ok(m.versions
        .into_iter()
        .filter(|e| include_snapshots || e.kind == "release")
        .map(|e| VanillaVersion { id: e.id, kind: e.kind })
        .collect())
}

/// URL del `server.jar` oficial para una versión (más su tamaño si viene).
pub struct VanillaDownload {
    pub url: String,
    pub size: Option<u64>,
}

pub async fn server_jar_url(version: &str) -> Result<VanillaDownload> {
    let m = fetch_manifest().await?;
    let entry = m
        .versions
        .iter()
        .find(|e| e.id == version)
        .ok_or_else(|| ServerError::DownloadFailed(format!("Mojang: versión desconocida: {version}")))?;
    let resp = client()
        .map_err(|e| ServerError::DownloadFailed(format!("Mojang: no se pudo crear cliente HTTP: {e}")))?
        .get(&entry.url)
        .send()
        .await
        .map_err(|e| {
            ServerError::DownloadFailed(format!("Mojang: no se pudo leer la versión {version}: {e}"))
        })?;
    if !resp.status().is_success() {
        return Err(ServerError::DownloadFailed(format!(
            "Mojang: el servidor respondió {} para la versión {version}",
            resp.status()
        )));
    }
    let vj: VersionJson = resp
        .json()
        .await
        .map_err(|e| ServerError::DownloadFailed(format!("Mojang: respuesta inválida: {e}")))?;
    Ok(VanillaDownload {
        url: vj.downloads.server.url,
        size: vj.downloads.server.size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    /// Requiere red. El manifest debe traer releases.
    #[test]
    #[ignore = "needs-network"]
    fn live_lists_releases() {
        let vs = rt().block_on(list_versions(false)).expect("manifest debe responder");
        assert!(!vs.is_empty());
        assert!(vs.iter().all(|v| v.kind == "release"));
    }

    /// Requiere red. Resuelve el server.jar del último release.
    #[test]
    #[ignore = "needs-network"]
    fn live_resolves_server_jar() {
        let vs = rt().block_on(list_versions(false)).expect("manifest debe responder");
        let dl = rt()
            .block_on(server_jar_url(&vs[0].id))
            .expect("debe resolver server.jar");
        assert!(dl.url.starts_with("https://"), "URL inesperada: {}", dl.url);
    }
}
