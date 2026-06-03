mod about;
mod advanced_ui;
mod activity_log;
mod autostart;
mod backup_actions;
mod debug;
mod daemon_lifecycle;
mod dbus_client;
mod dbus_runtime;
mod form_fields;
mod icons;
mod logs;
mod onboarding;
mod pbs_client_setup;
mod profiles;
mod encryption_keys;
mod restore;
mod settings;
mod settings_apply;
mod shell;
mod status_poll;
mod tray;
mod tray_state;
mod updates;
mod util;
mod window;

use std::process::Command;

use gtk::glib;
use gtk::prelude::{ApplicationExt, ApplicationExtManual};

fn main() {
    let cli = debug::parse_cli_args(std::env::args());
    if cli.help {
        debug::print_help();
        return;
    }
    if let Err(err) = debug::init(cli.debug) {
        eprintln!("backuppilot: logging init failed: {err}");
    }

    if let Err(err) = libadwaita::init() {
        eprintln!("backuppilot: Libadwaita/GTK initialization failed: {err}");
        eprintln!(
            "backuppilot: Is a graphical session available (WAYLAND_DISPLAY or DISPLAY)?"
        );
        std::process::exit(1);
    }
    backuppilot_i18n::init();
    settings_apply::apply_loaded();

    let background = cli.background;

    if !background {
        stop_stale_systemd_gui_service();
    }

    let app = libadwaita::Application::builder()
        .application_id(window::application_id())
        .build();

    icons::init();
    icons::apply_to_application(&app);
    debug::register_application_options(&app);

    app.connect_startup(move |app| {
        glib::idle_add_local_once(autostart::ensure_autostart_from_settings);
        window::register_application(app);
        icons::apply_to_application(app);
        icons::refresh_on_main_loop();
        std::thread::spawn(|| {
            daemon_lifecycle::ensure_daemon_running();
            debug::log_daemon_status(daemon_lifecycle::is_daemon_reachable());
        });
        if background && settings_apply::tray_icon_enabled() {
            icons::init_tray_pixmaps();
            glib::timeout_add_seconds_local(5, || {
                tray::spawn();
                glib::ControlFlow::Break
            });
        }
        if backuppilot_core::app_update_checks_enabled() {
            updates::init_scheduler();
        }
    });

    app.connect_activate(move |app| {
        tracing::info!(
            remote = app.is_remote(),
            background,
            debug = debug::enabled(),
            "application activate"
        );
        if !background {
            let app = app.clone();
            glib::idle_add_local_once(move || window::present(&app));
        }
    });

    // Volle argv an GApplication — eigene Optionen sind registriert (siehe debug::register_application_options).
    let run_argv: Vec<String> = std::env::args().collect();
    let run_argv: Vec<&str> = run_argv.iter().map(String::as_str).collect();
    let status = app.run_with_args(&run_argv);
    if status.value() != 0 {
        eprintln!(
            "backuppilot: application exited with status {} (GTK/GApplication failure)",
            status.value()
        );
        eprintln!(
            "backuppilot: Bei „Failed to register“/Zeitüberschreitung: andere backuppilot-Instanz beenden; systemctl --user stop backuppilot-gui.service; erneut starten."
        );
        std::process::exit(status.value());
    }
}

/// Alten systemd-Tray-Dienst stoppen. Kein `pkill backuppilot` — das würde bei einem
/// zweiten Start (Launcher, Desktop-Icon) die laufende GUI beenden („Getötet“).
fn stop_stale_systemd_gui_service() {
    let _ = Command::new("systemctl")
        .args(["--user", "stop", "--quiet", "backuppilot-gui.service"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
