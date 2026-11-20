use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid catalog: {0}")]
    Catalog(String),
    #[error("Unknown app: {0}")]
    UnknownApp(String),
    #[error("Unknown tweak or debloat item: {0}")]
    UnknownItem(String),
    #[error("{0} isn't available on this system")]
    Unavailable(String),
    #[error("WSL: {0}")]
    Wsl(String),
    #[error("Profiles: {0}")]
    Profile(String),
    #[error("Backup: {0}")]
    Backup(String),
    #[error("File dialog: {0}")]
    Dialog(String),
    #[error("Another run is already in progress")]
    AlreadyRunning,
    #[error("Background task failed: {0}")]
    Join(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Errors reach the frontend as plain messages.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
