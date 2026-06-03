//! Login autostart: systemd user service (daemon) + XDG autostart entry (tray, no window).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use backuppilot_core::app_settings::{load_app_settings, save_app_settings};
use backuppilot_core::{find_executable, is_flatpak_runtime, APP_ID, AUTOSTART_DESKTOP, DBUS_NAME, DBUS_SERVICE_FILE, ICON_NAME};
use backuppilot_core::paths::{
    legacy_autostart_desktop_path, legacy_systemd_unit_path, user_autostart_dir,
    user_systemd_user_dir,
};
use crate::daemon_lifecycle::{self, resolve_daemon_binary};

const DAEMON_SYSTEMD_UNIT: &str = "backuppilot-daemon.service";
const GUI_SYSTEMD_UNIT: &str = "backuppilot-gui.service";

pub fn start_at_login_active() -> bool {
    load_app_settings().start_at_login
        && (autostart_desktop_is_valid() || is_user_unit_enabled(GUI_SYSTEMD_UNIT))
}

/// Re-install autostart files when settings say enabled but artifacts are missing or on legacy paths.
pub fn ensure_autostart_from_settings() {
    if is_flatpak_runtime() {
        daemon_lifecycle::remove_stale_user_dbus_service();
    }
    if !load_app_settings().start_at_login {
        return;
    }
    if autostart_needs_refresh() || !is_user_unit_enabled(GUI_SYSTEMD_UNIT) {
        let _ = enable();
        remove_legacy_autostart_files();
    }
}

fn autostart_needs_refresh() -> bool {
    legacy_autostart_desktop_path().is_file()
        || !autostart_desktop_is_valid()
        || gui_systemd_unit_needs_refresh()
}

/// Older units invoked `backuppilot --background` without a graphical session environment.
fn gui_systemd_unit_needs_refresh() -> bool {
    let path = user_systemd_user_dir().join(GUI_SYSTEMD_UNIT);
    if !path.is_file() {
        return true;
    }
    fs::read_to_string(&path)
        .ok()
        .is_some_and(|content| !content.contains("backuppilot-gui-launch"))
}

pub fn set_start_at_login(enabled: bool) -> Result<(), String> {
    if enabled {
        remove_legacy_autostart_files();
        enable()?;
    } else {
        disable()?;
    }
    let mut settings = load_app_settings();
    settings.start_at_login = enabled;
    save_app_settings(&settings).map_err(|e| e.to_string())?;
    Ok(())
}

fn enable() -> Result<(), String> {
    if is_flatpak_runtime() {
        return enable_flatpak();
    }
    let daemon = resolve_daemon_binary()?;
    let gui = resolve_gui_binary();
    let launcher = resolve_gui_launcher(&gui)?;
    install_daemon_systemd_unit(&daemon)?;
    install_gui_systemd_unit(&gui, &launcher)?;
    install_dbus_service_file(&daemon)?;
    install_autostart_desktop()?;
    run_systemctl(&["daemon-reload"])?;

    if daemon_lifecycle::is_daemon_reachable() {
        run_systemctl(&["enable", DAEMON_SYSTEMD_UNIT])?;
    } else {
        run_systemctl(&["enable", "--now", DAEMON_SYSTEMD_UNIT])?;
    }

    // Nur aktivieren, nicht sofort starten: --now würde mit einer manuell gestarteten
    // GUI um die GApplication-D-Bus-Registrierung konkurrieren (Zeitüberschreitung).
    run_systemctl(&["enable", GUI_SYSTEMD_UNIT])?;
    Ok(())
}

fn disable() -> Result<(), String> {
    remove_autostart_artifacts()
}

/// Remove login autostart without changing `app-settings.json` (used before a full data wipe).
pub fn remove_autostart_artifacts() -> Result<(), String> {
    backuppilot_core::reset::remove_autostart_artifacts().map_err(|e| e.to_string())
}

fn install_daemon_systemd_unit(daemon: &Path) -> Result<(), String> {
    let unit_dir = user_systemd_user_dir();
    fs::create_dir_all(&unit_dir).map_err(|e| e.to_string())?;
    let content = format_daemon_systemd_unit(daemon);
    fs::write(unit_dir.join(DAEMON_SYSTEMD_UNIT), content).map_err(|e| e.to_string())?;
    Ok(())
}

fn install_gui_systemd_unit(gui: &Path, launcher: &Path) -> Result<(), String> {
    let unit_dir = user_systemd_user_dir();
    fs::create_dir_all(&unit_dir).map_err(|e| e.to_string())?;
    let content = format_gui_systemd_unit(gui, launcher);
    fs::write(unit_dir.join(GUI_SYSTEMD_UNIT), content).map_err(|e| e.to_string())?;
    Ok(())
}

fn format_daemon_systemd_unit(daemon: &Path) -> String {
    format!(
        r#"[Unit]
Description=BackupPilot background service
After=dbus.service

[Service]
Type=dbus
BusName={dbus}
ExecStart={}
Environment=PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/snap/bin:{home_bin}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        daemon.display(),
        dbus = DBUS_NAME,
        home_bin = home_local_bin().display(),
    )
}

fn format_gui_systemd_unit(gui: &Path, launcher: &Path) -> String {
    format!(
        r#"[Unit]
Description=BackupPilot tray and notifications
After=graphical-session.target dbus.service
PartOf=graphical-session.target
Wants={daemon_unit}

[Service]
Type=simple
ExecStart={launcher}
Environment=BACKUPPILOT_GUI={gui}
Environment=PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/snap/bin:{home_bin}
PassEnvironment=WAYLAND_DISPLAY DISPLAY XAUTHORITY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS
Restart=on-failure
RestartSec=15

[Install]
WantedBy=graphical-session.target
"#,
        launcher = launcher.display(),
        gui = gui.display(),
        daemon_unit = DAEMON_SYSTEMD_UNIT,
        home_bin = home_local_bin().display(),
    )
}

const GUI_LAUNCH_SCRIPT: &str = include_str!("../../../../packaging/backuppilot-gui-launch.sh");

fn resolve_gui_launcher(gui: &Path) -> Result<PathBuf, String> {
    let system = PathBuf::from("/usr/libexec/backuppilot-gui-launch.sh");
    if system.is_file() {
        return Ok(system);
    }
    install_user_gui_launch_script(gui)
}

fn install_user_gui_launch_script(gui: &Path) -> Result<PathBuf, String> {
    let dir = home_libexec_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("backuppilot-gui-launch.sh");
    fs::write(&path, GUI_LAUNCH_SCRIPT).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    // Script reads BACKUPPILOT_GUI from the unit Environment= line.
    let _ = gui;
    Ok(path)
}

fn home_libexec_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/libexec")
}

fn install_dbus_service_file(daemon: &Path) -> Result<(), String> {
    let dir = dbus_services_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let content = format!(
        "[D-BUS Service]\nName={}\nExec={}\n",
        DBUS_NAME,
        daemon.display()
    );
    fs::write(dir.join(DBUS_SERVICE_FILE), content).map_err(|e| e.to_string())?;
    Ok(())
}

fn dbus_services_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/share/dbus-1/services")
}

fn home_local_bin() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/bin")
}

fn install_autostart_desktop() -> Result<(), String> {
    let autostart_dir = autostart_dir();
    fs::create_dir_all(&autostart_dir).map_err(|e| e.to_string())?;
    let path = autostart_dir.join(AUTOSTART_DESKTOP);
    // GNOME Settings often replaces this with a symlink to the launcher .desktop (no --background).
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    let gui = resolve_gui_binary();
    let content = format_autostart_desktop(&gui);
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(())
}

fn format_autostart_desktop(gui: &Path) -> String {
    // Do not set X-GNOME-Autostart-Phase: systemd-xdg-autostart-generator skips such
    // entries entirely on Ubuntu/GNOME (session autostart would never run).
    format!(
        r#"[Desktop Entry]
Type=Application
Version=1.1
Name=BackupPilot
Comment=BackupPilot tray and scheduled backups
Exec={} --background
TryExec={}
Icon={icon}
Terminal=false
Categories=Utility;
StartupNotify=false
Hidden=false
DBusActivatable=false
X-GNOME-Autostart-enabled=true
X-GNOME-Autostart-Delay=3
X-GNOME-Autostart-systemd=backuppilot-gui
"#,
        gui.display(),
        gui.display(),
        icon = ICON_NAME,
    )
}

fn autostart_dir() -> PathBuf {
    user_autostart_dir()
}

fn autostart_desktop_path() -> PathBuf {
    autostart_dir().join(AUTOSTART_DESKTOP)
}

/// Real autostart file with `--background`, not a GNOME symlink to the launcher entry.
fn autostart_desktop_is_valid() -> bool {
    let path = autostart_desktop_path();
    if path.is_symlink() || !path.is_file() {
        return false;
    }
    fs::read_to_string(&path)
        .ok()
        .is_some_and(|content| {
            !content.contains("X-GNOME-Autostart-Phase") && content.contains("--background")
        })
}

fn is_user_unit_enabled(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", unit])
        .output()
        .ok()
        .is_some_and(|o| o.status.success())
}

fn remove_legacy_autostart_files() {
    for path in [legacy_autostart_desktop_path(), legacy_systemd_unit_path()] {
        if path.is_file() {
            let _ = fs::remove_file(&path);
        }
    }
}

fn enable_flatpak() -> Result<(), String> {
    daemon_lifecycle::remove_stale_user_dbus_service();
    install_flatpak_autostart_desktop()?;
    daemon_lifecycle::ensure_daemon_running();
    Ok(())
}

fn install_flatpak_autostart_desktop() -> Result<(), String> {
    let app_id = std::env::var("FLATPAK_ID").unwrap_or_else(|_| APP_ID.to_string());
    let autostart_dir = autostart_dir();
    fs::create_dir_all(&autostart_dir).map_err(|e| e.to_string())?;
    let path = autostart_dir.join(AUTOSTART_DESKTOP);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    let content = format!(
        r#"[Desktop Entry]
Type=Application
Version=1.1
Name=BackupPilot
Comment=BackupPilot tray and scheduled backups
Exec=/usr/bin/flatpak run --user {app_id} --command=backuppilot --background
Icon={app_id}
Terminal=false
X-GNOME-Autostart-enabled=true
"#,
    );
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(())
}

fn resolve_gui_binary() -> PathBuf {
    if is_flatpak_runtime() {
        let flatpak = PathBuf::from("/app/bin/backuppilot");
        if flatpak.is_file() {
            return flatpak;
        }
    }

    if let Some(path) = find_executable("backuppilot") {
        return path;
    }

    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("backuppilot"))
}

fn run_systemctl(args: &[&str]) -> Result<(), String> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|e| tr_fmt_systemctl(&format!("{e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("exit code {}", output.status.code().unwrap_or(-1))
        } else {
            stderr
        };
        Err(tr_fmt_systemctl(&detail))
    }
}

fn tr_fmt_systemctl(detail: &str) -> String {
    backuppilot_i18n::tr_fmt("systemctl failed: {detail}", &[("detail", detail)])
}

