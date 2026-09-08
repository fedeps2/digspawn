// Update check (Opción A del SPEC): meta-file JSON plano en GitHub,
// patrón Phone Stories. Silent fail sin red: nunca bloquea la app.

use std::cmp::Ordering;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::errors::{Result, ServerError};

/// Raw del meta-file (repo + rama + archivo fijo).
pub const META_URL: &str = "https://raw.githubusercontent.com/fedeps2/digspawn/main/version.json";

#[derive(Debug, Deserialize)]
struct MetaFile {
    latest_public: String,
    url: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: String,
    pub url: String,
    pub notes: String,
    pub required: bool,
    pub available: bool,
    /// false = no se pudo chequear (sin red, etc). La UI no muestra nada.
    pub checked: bool,
}

/// Compara "1.0.1" vs "1.0.0" por segmentos numéricos.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let pa: Vec<u64> = a.split('.').map(|s| leading_num(s)).collect();
    let pb: Vec<u64> = b.split('.').map(|s| leading_num(s)).collect();
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            ord => return ord,
        }
    }
    Ordering::Equal
}

fn leading_num(s: &str) -> u64 {
    s.chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

async fn fetch_meta() -> Result<MetaFile> {
    let client = reqwest::Client::builder()
        .user_agent("digspawn/0.1.0")
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| ServerError::VersionsFailed(format!("update: {e}")))?;
    let resp = client
        .get(META_URL)
        .send()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("update: {e}")))?;
    if !resp.status().is_success() {
        return Err(ServerError::VersionsFailed(format!("update: http {}", resp.status())));
    }
    resp.json()
        .await
        .map_err(|e| ServerError::VersionsFailed(format!("update: meta inválido: {e}")))
}

pub async fn check_update() -> UpdateCheck {
    let current = env!("CARGO_PKG_VERSION").to_string();
    match fetch_meta().await {
        Ok(meta) => {
            let available = compare_versions(&meta.latest_public, &current) == Ordering::Greater;
            UpdateCheck {
                current,
                latest: meta.latest_public,
                url: meta.url,
                notes: meta.notes,
                required: meta.required,
                available,
                checked: true,
            }
        }
        Err(_) => UpdateCheck {
            current,
            latest: String::new(),
            url: String::new(),
            notes: String::new(),
            required: false,
            available: false,
            checked: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver() {
        assert_eq!(compare_versions("1.0.1", "1.0.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.9.9", "0.10.0"), Ordering::Less);
        assert_eq!(compare_versions("0.2", "0.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("2.0.0", "10.0.0"), Ordering::Less);
    }

    /// Requiere red. El meta-file real debe parsear (aunque no haya update).
    #[test]
    #[ignore = "needs-network"]
    fn live_meta_parses() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let c = rt.block_on(check_update());
        assert!(c.checked, "el meta-file debe estar publicado");
        assert!(!c.current.is_empty());
    }
}
