use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("IPC connection failed: {0}")]
    Connect(String),
    #[error("IPC I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("IPC protocol error: {0}")]
    Protocol(String),
    #[error("{0}")]
    Failure(String),
}

impl IpcError {
    pub fn failure(msg: impl std::fmt::Display) -> Self {
        Self::Failure(msg.to_string())
    }
}
