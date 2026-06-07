//! FUSE mount of PBS `.pxar` archives via `proxmox-backup-client mount`.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::encryption::{apply_encryption_to_command, EncryptionCliMode};
use crate::error::{CoreError, Result};
use crate::paths::{ensure_data_dirs, pbs_client_path};
use crate::pbs::{apply_pbs_client_env, PbsClient};
use crate::pbs_repository::PbsRepositoryParts;
use crate::profile::BackupProfile;

/// Request to mount one archive from a snapshot read-only via FUSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountSnapshotRequest {
    pub profile_id: i64,
    pub snapshot: String,
    pub archive_name: String,
    /// Human-readable path label (e.g. `/home`) for UI messages.
    #[serde(default)]
    pub source_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveMount {
    pub id: String,
    pub profile_id: i64,
    pub profile_name: String,
    pub snapshot: String,
    pub archive_name: String,
    pub source_label: String,
    pub mount_point: String,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountSnapshotResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<ActiveMount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmountSnapshotResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn mounts_base_dir() -> PathBuf {
    crate::paths::data_dir().join("mounts")
}

pub fn mount_session_id(profile_id: i64, snapshot: &str, archive_name: &str) -> String {
    format!("{profile_id}|{snapshot}|{archive_name}")
}

pub fn mount_point_for(profile_id: i64, snapshot: &str, archive_name: &str) -> PathBuf {
    let snap_slug = snapshot.replace('/', "_");
    let arch_slug = archive_name
        .strip_suffix(".pxar")
        .unwrap_or(archive_name)
        .replace('/', "_");
    mounts_base_dir()
        .join(profile_id.to_string())
        .join(snap_slug)
        .join(arch_slug)
}

pub fn fuse_available() -> bool {
    Path::new("/dev/fuse").exists()
        && (which_exists("fusermount") || which_exists("umount"))
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn is_mountpoint_active(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    std::process::Command::new("mountpoint")
        .arg("-q")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Unmount a FUSE mount point; kills `proxmox-backup-client mount` if needed.
pub fn force_unmount(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if !is_mountpoint_active(path) {
        return Ok(());
    }

    let path_arg = path.as_os_str();

    for bin in ["fusermount", "fusermount3"] {
        for flag in ["-uz", "-u"] {
            let _ = std::process::Command::new(bin)
                .arg(flag)
                .arg(path_arg)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output();
            if !is_mountpoint_active(path) {
                return Ok(());
            }
        }
    }

    kill_mount_holder_processes(path);

    let lazy = std::process::Command::new("umount")
        .args(["-l", path.to_string_lossy().as_ref()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(CoreError::Io)?;
    if lazy.status.success() && !is_mountpoint_active(path) {
        return Ok(());
    }

    let umount = std::process::Command::new("umount")
        .arg(path_arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(CoreError::Io)?;
    if umount.status.success() && !is_mountpoint_active(path) {
        return Ok(());
    }

    if is_mountpoint_active(path) {
        let err = String::from_utf8_lossy(&umount.stderr);
        return Err(CoreError::PbsCommand(format!(
            "Mount {} konnte nicht getrennt werden (Prozess noch aktiv?). {}",
            path.display(),
            err.trim()
        )));
    }
    Ok(())
}

pub fn unmount_directory(path: &Path) -> Result<()> {
    force_unmount(path)
}

fn kill_mount_holder_processes(path: &Path) {
    let path_str = path.to_string_lossy();

    let _ = std::process::Command::new("fuser")
        .args(["-km", path_str.as_ref()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    std::thread::sleep(std::time::Duration::from_millis(400));

    if !is_mountpoint_active(path) {
        return;
    }

    let Ok(output) = std::process::Command::new("pgrep")
        .args(["-af", "proxmox-backup-client"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if !line.contains(" mount ") || !line.contains(path_str.as_ref()) {
            continue;
        }
        let Some(pid) = line.split_whitespace().next() else {
            continue;
        };
        let _ = std::process::Command::new("kill")
            .arg(pid)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    std::thread::sleep(std::time::Duration::from_millis(300));
}

pub async fn spawn_mount_process(
    profile: &BackupProfile,
    snapshot: &str,
    archive_name: &str,
    mount_point: &Path,
    encryption_key_id: Option<i64>,
) -> Result<tokio::process::Child> {
    if !fuse_available() {
        return Err(CoreError::PbsCommand(
            "FUSE is not available on this system (install fuse3 and ensure /dev/fuse exists)."
                .into(),
        ));
    }

    ensure_data_dirs().map_err(CoreError::Io)?;
    std::fs::create_dir_all(mount_point).map_err(CoreError::Io)?;

    if is_mountpoint_active(mount_point) {
        return Err(CoreError::PbsCommand(format!(
            "mount point {} is already in use",
            mount_point.display()
        )));
    }

    let parts = PbsRepositoryParts::parse(&profile.repository)
        .map_err(|e| CoreError::PbsCommand(e.to_string()))?;
    let auth_id = parts
        .pbs_auth_id()
        .ok_or_else(|| CoreError::PbsCommand("missing API token".into()))?;
    let repo_arg = parts.pbs_repository_bash_style(&auth_id);
    let password = parts.api_token_secret();

    let _ = PbsClient::write_profile_config(profile);

    let mut cmd = crate::pbs::spawn_pbs_command(pbs_client_path());
    apply_pbs_client_env(
        &mut cmd,
        &parts,
        &repo_arg,
        &password,
        profile.server_fingerprint.as_deref(),
    );
    cmd.arg("mount")
        .arg(snapshot)
        .arg(archive_name)
        .arg(mount_point);
    cmd.arg(format!("--repository={repo_arg}"));
    if let Some(ns) = profile.namespace.as_ref().filter(|s| !s.is_empty()) {
        cmd.arg(format!("--ns={ns}"));
    }
    let key_id = encryption_key_id.or(profile.encryption_key_id);
    apply_encryption_to_command(&mut cmd, key_id, EncryptionCliMode::Decrypt)?;
    cmd.arg("--verbose=true");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    cmd.spawn().map_err(CoreError::Io)
}

pub fn wait_mount_ready(mount_point: &Path, attempts: u32) -> bool {
    for _ in 0..attempts {
        if is_mountpoint_active(mount_point) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}
