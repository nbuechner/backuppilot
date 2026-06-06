//! Optional verbose diagnostics (`backuppilot --debug`).

use std::cell::Cell;
use std::io::Write;

use backuppilot_core::DBUS_NAME;
use gtk::gio::prelude::ApplicationExt;
use gtk::glib::{Char, OptionArg, OptionFlags};
use libadwaita::Application;
use tracing_subscriber::EnvFilter;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
}

/// Parse CLI flags that must be stripped before `GApplication::run`.
pub fn parse_cli_args(args: impl IntoIterator<Item = impl AsRef<str>>) -> CliArgs {
    let mut background = false;
    let mut debug = false;
    let mut help = false;

    for arg in args {
        match arg.as_ref() {
            "--background" => background = true,
            "--debug" | "-d" => debug = true,
            "--help" | "-h" => help = true,
            _ => {}
        }
    }

    CliArgs {
        background,
        debug,
        help,
    }
}

pub struct CliArgs {
    pub background: bool,
    pub debug: bool,
    pub help: bool,
}

pub fn print_help() {
    let _ = writeln!(std::io::stderr(), "BackupPilot GUI");
    let _ = writeln!(std::io::stderr());
    let _ = writeln!(std::io::stderr(), "Optionen:");
    let _ = writeln!(std::io::stderr(), "  --background    Tray-Modus ohne Fenster (Autostart)");
    let _ = writeln!(std::io::stderr(), "  --debug, -d     Ausführliche Logs auf stderr (UI, D-Bus, Daemon)");
    let _ = writeln!(std::io::stderr(), "  --help, -h      Diese Hilfe");
    let _ = writeln!(std::io::stderr());
    let _ = writeln!(std::io::stderr(), "Beispiele:");
    let _ = writeln!(
        std::io::stderr(),
        "  RUST_BACKTRACE=1 backuppilot --debug"
    );
    let _ = writeln!(
        std::io::stderr(),
        "  RUST_LOG=backuppilot=trace backuppilot --debug"
    );
    let _ = writeln!(std::io::stderr());
    let _ = writeln!(std::io::stderr(), "Hintergrunddienst:");
    let _ = writeln!(
        std::io::stderr(),
        "  systemctl --user start backuppilot-daemon.service"
    );
    let _ = writeln!(
        std::io::stderr(),
        "  busctl --user call org.freedesktop.DBus /org/freedesktop/DBus \\"
    );
    let _ = writeln!(
        std::io::stderr(),
        "    org.freedesktop.DBus NameHasOwner s:{DBUS_NAME}"
    );
}

/// Initialize tracing and panic diagnostics. Call once at process start.
pub fn init(debug: bool) -> Result<(), String> {
    ENABLED.with(|c| c.set(debug));

    let filter = if debug {
        // RUST_LOG overrides defaults when set.
        EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("backuppilot=debug,ipc=info,gtk=info")
        })
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("backuppilot=info"))
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(debug)
        .with_thread_ids(debug)
        .with_thread_names(debug)
        .try_init()
        .map_err(|e| e.to_string())?;

    install_panic_hook(debug);

    if debug {
        tracing::info!(
            pid = std::process::id(),
            dbus_name = DBUS_NAME,
            "debug mode enabled"
        );
        log_environment();
    }

    Ok(())
}

pub fn enabled() -> bool {
    ENABLED.with(|c| c.get())
}

/// Damit GApplication `--debug` / `--background` akzeptiert (auch bei zweiter Instanz).
pub fn register_application_options(app: &Application) {
    app.add_main_option(
        "debug",
        Char::from(b'd'),
        OptionFlags::HIDDEN,
        OptionArg::None,
        "Verbose diagnostic logging",
        None,
    );
    app.add_main_option(
        "background",
        Char::from(b'\0'),
        OptionFlags::HIDDEN,
        OptionArg::None,
        "Tray-only mode (no main window)",
        None,
    );
}

/// UI / navigation event (only logged in debug mode).
pub fn log_ui(event: &'static str, detail: impl std::fmt::Display) {
    if enabled() {
        tracing::debug!(event, %detail, "ui");
    }
}

/// D-Bus task lifecycle (only logged in debug mode).
pub fn log_dbus(phase: &'static str, detail: impl std::fmt::Display) {
    if enabled() {
        tracing::debug!(phase, %detail, "dbus");
    }
}

/// Log daemon reachability after startup checks.
pub fn log_daemon_status(reachable: bool) {
    if enabled() {
        tracing::debug!(reachable, dbus_name = DBUS_NAME, "daemon session bus");
    } else if !reachable {
        tracing::warn!(
            dbus_name = DBUS_NAME,
            "daemon not on session bus — start: systemctl --user start backuppilot-daemon.service"
        );
    }
}

fn install_panic_hook(debug: bool) {
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("backuppilot: PANIC: {info}");
        if debug || std::env::var_os("RUST_BACKTRACE").is_some() {
            eprintln!("backuppilot: Tipp: RUST_BACKTRACE=1 backuppilot --debug");
            eprintln!("{:?}", std::backtrace::Backtrace::force_capture());
        }
    }));
}

fn log_environment() {
    for key in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_SESSION_TYPE",
        "DBUS_SESSION_BUS_ADDRESS",
        "RUST_LOG",
        "RUST_BACKTRACE",
    ] {
        let value = std::env::var(key).unwrap_or_else(|_| "(not set)".into());
        tracing::debug!(var = key, %value, "env");
    }
}
