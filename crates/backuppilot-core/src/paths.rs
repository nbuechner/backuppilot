use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// XDG data directory for BackupPilot (`~/.local/share/backuppilot`).
pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("ch", "backuppilot", "BackupPilot")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share/backuppilot")
        })
}

pub fn database_path() -> PathBuf {
    data_dir().join("backuppilot.db")
}

/// Written by the terminal install wrapper; consumed by the daemon on the next activity refresh.
pub fn pbs_client_install_result_path() -> PathBuf {
    data_dir().join("pbs-client-install-result.json")
}

pub fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("ch", "backuppilot", "BackupPilot")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config/backuppilot")
        })
}

/// `$XDG_CONFIG_HOME` (default `~/.config`) — not the app-specific config subtree.
pub fn xdg_config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".config"))
                .unwrap_or_else(|| PathBuf::from(".config"))
        })
}

/// XDG autostart directory (`~/.config/autostart`).
pub fn user_autostart_dir() -> PathBuf {
    xdg_config_home().join("autostart")
}

/// Per-user systemd unit directory (`~/.config/systemd/user`).
pub fn user_systemd_user_dir() -> PathBuf {
    xdg_config_home().join("systemd/user")
}

/// Legacy paths used before autostart was fixed (under the app config dir).
pub fn legacy_autostart_desktop_path() -> PathBuf {
    config_dir().join("autostart").join(crate::ids::AUTOSTART_DESKTOP)
}

pub fn legacy_systemd_unit_path() -> PathBuf {
    config_dir()
        .join("systemd/user")
        .join("backuppilot-daemon.service")
}

pub fn ensure_data_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(config_dir())?;
    Ok(())
}

static PBS_CLIENT_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Resolved `proxmox-backup-client` executable (cached).
///
/// Lookup order: `BACKUPPILOT_PBS_CLIENT`, Flatpak-Wrapper, `/usr/bin/…`, `/usr/local/bin/…`, `PATH`.
pub fn pbs_client_path() -> &'static Path {
    PBS_CLIENT_PATH
        .get_or_init(resolve_pbs_client_binary)
        .as_path()
}

/// True when running inside a Flatpak sandbox (updates are delivered via Flathub).
pub fn is_flatpak_runtime() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
}

/// Flatpak wrapper that runs `proxmox-backup-client` on the host via `flatpak-spawn`.
pub fn flatpak_pbs_wrapper_path() -> PathBuf {
    PathBuf::from("/app/bin/backuppilot-pbs-client")
}

/// Resolved `backuppilot-cli` on the host (not inside the Flatpak `/app` tree).
pub fn resolve_cli_binary() -> PathBuf {
    if let Ok(custom) = std::env::var("BACKUPPILOT_CLI") {
        let path = PathBuf::from(&custom);
        if path.is_file() {
            return path;
        }
    }
    if let Some(path) = find_executable("backuppilot-cli") {
        return path;
    }
    PathBuf::from("backuppilot-cli")
}

/// Whether `backuppilot-cli` is installed and executable on the current machine.
pub fn cli_available_sync() -> bool {
    let path = resolve_cli_binary();
    std::process::Command::new(&path)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Async availability check (includes Flatpak host probe).
pub async fn cli_available() -> bool {
    if cli_available_sync() {
        return true;
    }
    if is_flatpak_runtime() {
        return probe_flatpak_host_cli().await;
    }
    false
}

async fn probe_flatpak_host_cli() -> bool {
    use std::process::Stdio;
    tokio::process::Command::new("flatpak-spawn")
        .args(["--host", "backuppilot-cli", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Points config/data lookups at another user's home (`getent passwd`).
pub fn apply_config_owner(username: &str) -> Result<(), String> {
    let home = passwd_home_dir(username)?;
    std::env::set_var("HOME", &home);
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_STATE_HOME");
    Ok(())
}

fn passwd_home_dir(username: &str) -> Result<String, String> {
    let output = std::process::Command::new("getent")
        .args(["passwd", username])
        .output()
        .map_err(|e| format!("getent failed: {e}"))?;
    if !output.status.success() {
        return Err(format!("user not found: {username}"));
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let line = line.lines().next().unwrap_or("").trim();
    let home = line
        .split(':')
        .nth(5)
        .filter(|h| !h.is_empty())
        .ok_or_else(|| format!("could not parse home directory for {username}"))?;
    Ok(home.to_string())
}

/// Resolves the PBS client binary without using a stale cache (for availability checks).
pub fn resolve_pbs_client_binary() -> PathBuf {
    if is_flatpak_runtime() {
        let wrapper = flatpak_pbs_wrapper_path();
        if wrapper.is_file() {
            return wrapper;
        }
    }

    if let Ok(custom) = std::env::var("BACKUPPILOT_PBS_CLIENT") {
        let path = PathBuf::from(&custom);
        if path.is_file() {
            return path;
        }
    }

    if let Some(path) = find_executable("proxmox-backup-client") {
        return path;
    }

    PathBuf::from("proxmox-backup-client")
}

/// Locate an executable on standard system paths and `$PATH`.
pub fn find_executable(name: &str) -> Option<PathBuf> {
    for candidate in ["/usr/bin", "/usr/local/bin"] {
        let path = PathBuf::from(candidate).join(name);
        if path.is_file() {
            return Some(path);
        }
    }

    let Ok(path_var) = std::env::var("PATH") else {
        return None;
    };
    for dir in path_var.split(':').filter(|d| !d.is_empty()) {
        let path = PathBuf::from(dir).join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

pub fn backup_client_config_path() -> PathBuf {
    config_dir().join("backup-client.json")
}

/// Runtime directory for per-profile PBS client config snippets.
pub fn runtime_profile_config_dir(profile_id: i64) -> PathBuf {
    data_dir().join("profiles").join(profile_id.to_string())
}

pub fn profile_backup_client_config(profile_id: i64) -> PathBuf {
    runtime_profile_config_dir(profile_id).join("backup-client.json")
}

pub fn parent_exists(path: &Path) -> bool {
    path.parent().is_some_and(|p| p.exists())
}

/// Version reported by `proxmox-backup-client version` (e.g. `4.2.0`), if available.
pub fn pbs_client_version() -> Option<String> {
    let path = resolve_pbs_client_binary();
    let output = std::process::Command::new(&path)
        .arg("version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_pbs_client_version_stdout(&String::from_utf8_lossy(&output.stdout))
}

fn parse_pbs_client_version_stdout(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("client version:") {
            let version = rest.trim();
            if !version.is_empty() {
                return Some(version.to_string());
            }
        }
    }
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_client_version_line() {
        assert_eq!(
            parse_pbs_client_version_stdout("client version: 4.2.0\n"),
            Some("4.2.0".into())
        );
    }

    #[test]
    fn resolves_pbs_client_when_installed() {
        let path = resolve_pbs_client_binary();
        if path == PathBuf::from("proxmox-backup-client") && !path.is_file() {
            return;
        }
        assert!(
            path.is_file(),
            "expected executable at {}, set BACKUPPILOT_PBS_CLIENT if needed",
            path.display()
        );
    }
}
