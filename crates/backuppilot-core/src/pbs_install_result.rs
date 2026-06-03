//! Result file from the GUI terminal install wrapper → activity log entry.

use serde::{Deserialize, Serialize};

use crate::paths::{ensure_data_dirs, pbs_client_install_result_path};
use crate::profile::RunStatus;
use crate::Result;

pub const PBS_CLIENT_INSTALL_KIND: &str = "pbs_client_install";
pub const PBS_CLIENT_INSTALL_DISPLAY: &str = "Proxmox Backup Client";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PbsInstallResultStatus {
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbsInstallResult {
    pub status: PbsInstallResultStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl PbsInstallResult {
    pub fn run_status(&self) -> RunStatus {
        match self.status {
            PbsInstallResultStatus::Success => RunStatus::Success,
            PbsInstallResultStatus::Failed => RunStatus::Failed,
        }
    }
}

/// Reads and removes the install result file if present (at most one activity entry per file).
pub fn consume_install_result_file() -> Result<Option<PbsInstallResult>> {
    let path = pbs_client_install_result_path();
    if !path.is_file() {
        return Ok(None);
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            return Ok(None);
        }
    };
    let _ = std::fs::remove_file(&path);
    match serde_json::from_str::<PbsInstallResult>(&raw) {
        Ok(result) => Ok(Some(result)),
        Err(_) => Ok(None),
    }
}

/// Path for the wrapper script (bash), escaped for embedding.
pub fn install_result_path_for_shell() -> Result<String> {
    ensure_data_dirs().map_err(crate::CoreError::Io)?;
    Ok(shell_escape_single(
        &pbs_client_install_result_path().display().to_string(),
    ))
}

fn shell_escape_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
