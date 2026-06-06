//! Daemon start and reachability (native install and Flatpak).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use backuppilot_core::{find_executable, is_flatpak_runtime, DBUS_SERVICE_FILE};
use backuppilot_i18n::tr;

const DAEMON_SYSTEMD_UNIT: &str = "backuppilot-daemon.service";
const REACHABILITY_WAIT: Duration = Duration::from_millis(250);
const REACHABILITY_ATTEMPTS: u32 = 40;

/// Host session D-Bus must not use a user `.service` file with sandbox-only paths.
pub fn remove_stale_user_dbus_service() {
    if !is_flatpak_runtime() {
        return;
    }
    let path = user_dbus_services_dir().join(DBUS_SERVICE_FILE);
    if !path.is_file() {
        return;
    }
    let Ok(content) = fs::read_to_string(&path) else {
        return;
    };
    if content.contains("/app/bin")
        || content.contains("/usr/bin/backuppilot-daemon")
        || content.contains("/.local/bin/backuppilot-daemon")
    {
        let _ = fs::remove_file(&path);
    }
}

pub fn resolve_daemon_binary() -> Result<PathBuf, String> {
    if is_flatpak_runtime() {
        let flatpak = PathBuf::from("/app/bin/backuppilot-daemon");
        if flatpak.is_file() {
            return Ok(flatpak);
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let local = PathBuf::from(home).join(".local/bin/backuppilot-daemon");
        if local.is_file() {
            return Ok(local);
        }
    }

    if let Some(path) = find_executable("backuppilot-daemon") {
        return Ok(path);
    }

    Err(tr(
        "backuppilot-daemon was not found. Install it to ~/.local/bin or /usr/bin first.",
    ))
}

pub fn is_daemon_reachable() -> bool {
    std::thread::spawn(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
            .map(|rt| {
                rt.block_on(async {
                    backuppilot_ipc::IpcClient::call("version", serde_json::Value::Null)
                        .await
                        .is_ok()
                })
            })
            .unwrap_or(false)
    })
    .join()
    .unwrap_or(false)
}

fn spawn_daemon_process(daemon: &Path) {
    let _ = Command::new(daemon)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn wait_for_daemon() {
    for _ in 0..REACHABILITY_ATTEMPTS {
        if is_daemon_reachable() {
            return;
        }
        thread::sleep(REACHABILITY_WAIT);
    }
}

/// Start the daemon if it is not on the session bus (spawn in-process, not D-Bus activation).
pub fn ensure_daemon_running() {
    remove_stale_user_dbus_service();
    if is_daemon_reachable() {
        return;
    }

    if let Ok(daemon) = resolve_daemon_binary() {
        spawn_daemon_process(&daemon);
        wait_for_daemon();
        if is_daemon_reachable() {
            return;
        }
    }

    if !is_flatpak_runtime() {
        let _ = Command::new("systemctl")
            .args(["--user", "start", DAEMON_SYSTEMD_UNIT])
            .status();
        wait_for_daemon();
    }
}

fn user_dbus_services_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/share/dbus-1/services")
}
