use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("profile not found: {0}")]
    ProfileNotFound(i64),

    #[error("backup already running for profile {0}")]
    BackupInProgress(i64),

    #[error("pbs client not found in PATH; install proxmox-backup-client")]
    PbsClientMissing,

    #[error("pbs command failed: {0}")]
    PbsCommand(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
