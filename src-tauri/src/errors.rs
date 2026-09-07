// Errores ricos del backend: cada fallo llega al frontend con `kind` + `message`
// para mostrarlo en el wizard (el pana tiene que saber qué pasó).

use serde::Serialize;

/// Error serializable como union discriminada `{ kind, message }`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum ServerError {
    InvalidName(String),
    AlreadyExists(String),
    NotFound(String),
    EulaNotAccepted(String),
    DownloadFailed(String),
    VersionsFailed(String),
    JavaNotFound(String),
    Io(String),
}

pub type Result<T> = std::result::Result<T, ServerError>;

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, message) = match self {
            ServerError::InvalidName(m) => ("InvalidName", m),
            ServerError::AlreadyExists(m) => ("AlreadyExists", m),
            ServerError::NotFound(m) => ("NotFound", m),
            ServerError::EulaNotAccepted(m) => ("EulaNotAccepted", m),
            ServerError::DownloadFailed(m) => ("DownloadFailed", m),
            ServerError::VersionsFailed(m) => ("VersionsFailed", m),
            ServerError::JavaNotFound(m) => ("JavaNotFound", m),
            ServerError::Io(m) => ("Io", m),
        };
        write!(f, "{kind}: {message}")
    }
}

impl From<std::io::Error> for ServerError {
    fn from(e: std::io::Error) -> Self {
        ServerError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for ServerError {
    fn from(e: serde_json::Error) -> Self {
        ServerError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_tagged_union() {
        let e = ServerError::DownloadFailed("red caída".to_string());
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "DownloadFailed");
        assert_eq!(v["message"], "red caída");
    }
}
