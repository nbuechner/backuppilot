//! Wipe all local BackupPilot user data (database, settings, profile configs, caches).

use std::fs;
use std::path::PathBuf;

use crate::paths::{
    config_dir, data_dir, ensure_data_dirs, legacy_autostart_desktop_path,
    legacy_systemd_unit_path, user_autostart_dir, user_systemd_user_dir,
};
use crate::restore::clear_all_catalog_cache;

use crate::ids::{AUTOSTART_DESKTOP, DBUS_SERVICE_FILE};

const DAEMON_SYSTEMD_UNIT: &str = "backuppilot-daemon.service";
const GUI_SYSTEMD_UNIT: &str = "backuppilot-gui.service";

/// Delete settings, profiles, backup history, runtime configs and autostart artifacts.
pub fn wipe_all_local_data() -> std::io::Result<()> {
    clear_all_catalog_cache();
    remove_autostart_artifacts()?;

    let data = data_dir();
    if data.exists() {
        fs::remove_dir_all(&data)?;
    }

    let config = config_dir();
    if config.exists() {
        fs::remove_dir_all(&config)?;
    }

    ensure_data_dirs()?;
    // Regenerate the WSL wrapper scripts — they live in data_dir() which was
    // just deleted, and PbsClient::is_available() would return false without them.
    #[cfg(windows)]
    {
        let _ = crate::paths::ensure_wsl_pbs_wrapper();
    }
    Ok(())
}

/// Disable login autostart and remove installed unit/desktop/dbus files under the home directory.
pub fn remove_autostart_artifacts() -> std::io::Result<()> {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", DAEMON_SYSTEMD_UNIT])
        .output();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", GUI_SYSTEMD_UNIT])
        .output();

    for path in [
        user_autostart_dir().join(AUTOSTART_DESKTOP),
        user_systemd_user_dir().join(DAEMON_SYSTEMD_UNIT),
        user_systemd_user_dir().join(GUI_SYSTEMD_UNIT),
        legacy_autostart_desktop_path(),
        legacy_systemd_unit_path(),
        dbus_service_path(),
    ] {
        if path.is_file() {
            let _ = fs::remove_file(&path);
        }
    }

    Ok(())
}

fn dbus_service_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/share/dbus-1/services")
        .join(DBUS_SERVICE_FILE)
}
