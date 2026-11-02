#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("transcript unreadable: {0}")]
    TranscriptUnreadable(String),
    #[error("invalid event: {0}")]
    InvalidEvent(String),
    #[error("git error: {0}")]
    Git(String),
    #[error("import error: {0}")]
    Import(String),
    /// Codes the UI translates: setup wizard failures ("invalid file", "CLI missing") and
    /// settings/launch ones ("invalid template", "executable not found", "app not found",
    /// "invalid ongoing").
    #[error("{0}")]
    Installer(String),
}

pub type Result<T> = std::result::Result<T, Error>;
