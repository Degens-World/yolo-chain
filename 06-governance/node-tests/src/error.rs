use thiserror::Error;

pub type Result<T> = std::result::Result<T, NodeError>;

#[derive(Debug, Error)]
pub enum NodeError {
    #[error("transport: {0}")]
    Transport(#[from] ureq::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("node returned status {status}: {body}")]
    BadStatus { status: u16, body: String },

    #[error("timeout waiting for {what} after {waited_ms} ms")]
    Timeout { what: String, waited_ms: u64 },

    #[error("invariant: {0}")]
    Invariant(String),
}
