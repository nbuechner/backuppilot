//! Windows setup status checks: WSL, proxmox-backup-client, fuse3.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WslSetupStatus {
    /// False on non-Windows platforms (not applicable).
    pub applicable: bool,
    pub wsl_available: bool,
    pub pbs_client_available: bool,
    pub pbs_client_version: Option<String>,
    pub fuse_available: bool,
    pub fuse_can_install: bool,
}

#[cfg(not(windows))]
pub fn check_windows_setup() -> WslSetupStatus {
    WslSetupStatus {
        applicable: false,
        wsl_available: true,
        pbs_client_available: true,
        pbs_client_version: None,
        fuse_available: true,
        fuse_can_install: false,
    }
}

#[cfg(windows)]
pub fn check_windows_setup() -> WslSetupStatus {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let wsl_available = std::process::Command::new("wsl")
        .args(["--", "true"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let (pbs_client_available, pbs_client_version) = if wsl_available {
        match std::process::Command::new("wsl")
            .args(["--", "proxmox-backup-client", "version"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            Ok(out) if out.status.success() => {
                let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
                // e.g. "proxmox-backup-client 3.3.1"
                let version = raw.split_whitespace().nth(1).map(str::to_string);
                (true, version)
            }
            _ => (false, None),
        }
    } else {
        (false, None)
    };

    let fuse_check = crate::pbs_mount::check_fuse();

    WslSetupStatus {
        applicable: true,
        wsl_available,
        pbs_client_available,
        pbs_client_version,
        fuse_available: fuse_check.available,
        fuse_can_install: fuse_check.can_install,
    }
}
