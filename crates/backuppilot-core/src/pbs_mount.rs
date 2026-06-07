//! FUSE mount of PBS `.pxar` archives via `proxmox-backup-client mount`.
//!
//! On Linux the mount runs natively. On Windows, `proxmox-backup-client` runs
//! inside WSL, so all FUSE checks and mount-point operations go through WSL.

use std::path::{Path, PathBuf};
use std::process::Stdio;
#[cfg(windows)]
use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::encryption::{apply_encryption_to_command, EncryptionCliMode};
use crate::error::{CoreError, Result};
#[cfg(not(windows))]
use crate::paths::ensure_data_dirs;
use crate::paths::pbs_client_path;
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
    /// WSL/Linux path to the mount point.
    pub mount_point: String,
    pub started_at: DateTime<Utc>,
    /// Windows UNC path (`\\wsl.localhost\distro\...`) for opening in Explorer.
    /// Only set on Windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountSnapshotResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<ActiveMount>,
    /// Set when the failure is specifically due to fuse3 not being installed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_fuse3: bool,
}

/// Result of [`check_fuse`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuseCheckResult {
    pub available: bool,
    /// Human-readable reason when not available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether automatic fuse3 install is offered on this system.
    pub can_install: bool,
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

// ── Mount point path ─────────────────────────────────────────────────────────

/// On Linux: a path under the app data dir.
/// On Windows: a path inside WSL (`/tmp/backuppilot-mounts/...`) because
/// `proxmox-backup-client mount` runs in WSL and needs a Linux mount point.
#[cfg(not(windows))]
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

#[cfg(windows)]
pub fn mount_point_for(profile_id: i64, snapshot: &str, archive_name: &str) -> PathBuf {
    let snap_slug = snapshot.replace('/', "_");
    let arch_slug = archive_name
        .strip_suffix(".pxar")
        .unwrap_or(archive_name)
        .replace('/', "_");
    // Forward-slash path — valid inside WSL, passes through the PowerShell wrapper unchanged.
    PathBuf::from(format!(
        "/tmp/backuppilot-mounts/{profile_id}/{snap_slug}/{arch_slug}"
    ))
}

// ── WSL helpers (Windows only) ───────────────────────────────────────────────

#[cfg(windows)]
fn wsl_run_status(args: &[&str]) -> bool {
    std::process::Command::new("wsl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Returns the WSL default distro name (e.g. `"Ubuntu"`) via `$WSL_DISTRO_NAME`.
/// Cached after the first successful call.
#[cfg(windows)]
pub fn wsl_distro_name() -> Option<&'static str> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let out = std::process::Command::new("wsl")
            .args(["-e", "sh", "-c", "printf '%s' \"$WSL_DISTRO_NAME\""])
            .output()
            .ok()?;
        let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if name.is_empty() { None } else { Some(name) }
    }).as_deref()
}

/// Converts a WSL Linux path to a Windows UNC path for Explorer.
/// `/tmp/foo` -> `\\wsl.localhost\Ubuntu\tmp\foo`
#[cfg(windows)]
pub fn wsl_path_to_unc(wsl_path: &Path) -> Option<String> {
    let distro = wsl_distro_name()?;
    let path_str = wsl_path.to_string_lossy();
    let without_slash = path_str.trim_start_matches('/');
    let win_suffix = without_slash.replace('/', "\\");
    Some(format!(r"\\wsl.localhost\{distro}\{win_suffix}"))
}

// ── FUSE availability ────────────────────────────────────────────────────────

#[cfg(not(windows))]
pub fn fuse_available() -> bool {
    Path::new("/dev/fuse").exists()
        && (which_exists("fusermount") || which_exists("fusermount3") || which_exists("umount"))
}

#[cfg(windows)]
pub fn fuse_available() -> bool {
    wsl_run_status(&[
        "--", "sh", "-c",
        "test -e /dev/fuse && { command -v fusermount3 >/dev/null 2>&1 || command -v umount >/dev/null 2>&1; }",
    ])
}

#[cfg(not(windows))]
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Probes FUSE availability in detail for the UI.
#[cfg(not(windows))]
pub fn check_fuse() -> FuseCheckResult {
    let dev_fuse = Path::new("/dev/fuse").exists();
    let has_mount = which_exists("fusermount")
        || which_exists("fusermount3")
        || which_exists("umount");

    if dev_fuse && has_mount {
        return FuseCheckResult { available: true, reason: None, can_install: false };
    }

    let reason = if !dev_fuse {
        "fuse3 is not installed (/dev/fuse not found).".into()
    } else {
        "fusermount3 not found (install the fuse3 package).".into()
    };

    FuseCheckResult { available: false, reason: Some(reason), can_install: true }
}

#[cfg(windows)]
pub fn check_fuse() -> FuseCheckResult {
    // First confirm WSL itself is usable.
    if !wsl_run_status(&["--", "true"]) {
        return FuseCheckResult {
            available: false,
            reason: Some("WSL is not available or not configured.".into()),
            can_install: false,
        };
    }

    let dev_fuse = wsl_run_status(&["--", "test", "-e", "/dev/fuse"]);
    let has_fusermount = wsl_run_status(&[
        "--", "sh", "-c",
        "command -v fusermount3 >/dev/null 2>&1 || command -v fusermount >/dev/null 2>&1",
    ]);

    if dev_fuse && has_fusermount {
        return FuseCheckResult { available: true, reason: None, can_install: false };
    }

    let reason = if !dev_fuse {
        "fuse3 is not installed in WSL (/dev/fuse not found).".into()
    } else {
        "fusermount3 not found in WSL (install the fuse3 package).".into()
    };

    FuseCheckResult { available: false, reason: Some(reason), can_install: true }
}

// ── Mount-point checks ───────────────────────────────────────────────────────

#[cfg(not(windows))]
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

#[cfg(windows)]
pub fn is_mountpoint_active(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    wsl_run_status(&["--", "mountpoint", "-q", path_str.as_ref()])
}

// ── Unmount ──────────────────────────────────────────────────────────────────

/// Unmount a FUSE mount point; kills `proxmox-backup-client mount` if needed.
#[cfg(not(windows))]
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
            "Mount {} could not be unmounted (process still active?). {}",
            path.display(),
            err.trim()
        )));
    }
    Ok(())
}

#[cfg(windows)]
pub fn force_unmount(path: &Path) -> Result<()> {
    if !is_mountpoint_active(path) {
        return Ok(());
    }
    let p = path.to_string_lossy();

    for args in [
        vec!["--", "fusermount3", "-uz", p.as_ref()],
        vec!["--", "fusermount3", "-u", p.as_ref()],
        vec!["--", "umount", "-l", p.as_ref()],
        vec!["--", "umount", p.as_ref()],
    ] {
        wsl_run_status(&args);
        if !is_mountpoint_active(path) {
            return Ok(());
        }
    }

    if is_mountpoint_active(path) {
        return Err(CoreError::PbsCommand(format!(
            "Could not unmount WSL FUSE mount at {}. Try: wsl -- fusermount3 -uz {}",
            path.display(), p
        )));
    }
    Ok(())
}

pub fn unmount_directory(path: &Path) -> Result<()> {
    force_unmount(path)
}

#[cfg(not(windows))]
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

// ── Spawn mount process ───────────────────────────────────────────────────────

pub async fn spawn_mount_process(
    profile: &BackupProfile,
    snapshot: &str,
    archive_name: &str,
    mount_point: &Path,
    encryption_key_id: Option<i64>,
) -> Result<tokio::process::Child> {
    if !fuse_available() {
        return Err(CoreError::PbsCommand(
            "FUSE is not available. Install fuse3 and ensure /dev/fuse exists.".into(),
        ));
    }

    // Create the mount directory — via WSL on Windows, directly on Linux.
    #[cfg(windows)]
    {
        let mp = mount_point.to_string_lossy();
        let status = tokio::process::Command::new("wsl")
            .args(["--", "mkdir", "-p", mp.as_ref()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(CoreError::Io)?;
        if !status.success() {
            return Err(CoreError::PbsCommand(format!(
                "Could not create WSL mount directory: {}", mp
            )));
        }
    }
    #[cfg(not(windows))]
    {
        ensure_data_dirs().map_err(CoreError::Io)?;
        std::fs::create_dir_all(mount_point).map_err(CoreError::Io)?;
    }

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
