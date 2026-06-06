//! Ensure the BackupPilot daemon is running before the GUI starts.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

const REACHABILITY_WAIT: Duration = Duration::from_millis(250);
const REACHABILITY_ATTEMPTS: u32 = 40;

fn is_daemon_reachable() -> bool {
    thread::spawn(|| {
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

fn find_daemon_binary() -> Option<PathBuf> {
    // 1. Same directory as this executable (installed together)
    if let Ok(mut exe) = std::env::current_exe() {
        exe.pop();
        let candidate = exe.join(if cfg!(windows) {
            "backuppilot-daemon.exe"
        } else {
            "backuppilot-daemon"
        });
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // 2. ~/.local/bin (Linux/macOS user install)
    if let Ok(home) = std::env::var("HOME") {
        let candidate = PathBuf::from(home).join(".local/bin/backuppilot-daemon");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // 3. PATH
    backuppilot_core::find_executable("backuppilot-daemon")
}

fn wait_for_daemon() {
    for _ in 0..REACHABILITY_ATTEMPTS {
        if is_daemon_reachable() {
            return;
        }
        thread::sleep(REACHABILITY_WAIT);
    }
}

pub fn ensure_daemon_running() {
    if is_daemon_reachable() {
        return;
    }

    if let Some(daemon) = find_daemon_binary() {
        let _ = std::process::Command::new(&daemon)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        wait_for_daemon();
    }
}
