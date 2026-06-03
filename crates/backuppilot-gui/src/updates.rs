//! GitLab release checks and guided update confirmation (install or release page).

use std::cell::RefCell;

use backuppilot_core::app_settings::{load_app_settings, UpdateChannel};
use backuppilot_core::notify::send_desktop_notification;
use backuppilot_core::paths::is_flatpak_runtime;
use backuppilot_core::updates::{
    can_install_update_packages, dismiss_available_update, download_package, install_package,
    is_update_newer_than_installed, load_update_state, should_notify_user, UpdateAvailability,
    UpdateCheckOutcome, GITLAB_PROJECT_URL,
};
use backuppilot_core::{installed_version, UpdateError};
use backuppilot_i18n::{tr, tr_fmt};
use gtk::glib;
use gtk::prelude::*;
use libadwaita::prelude::*;
use libadwaita::ApplicationWindow;
use libadwaita::Toast;

use crate::util;
use crate::window;

thread_local! {
    static UPDATE_STATUS_ROW: RefCell<Option<glib::WeakRef<libadwaita::ActionRow>>> =
        const { RefCell::new(None) };
    static PROMPTED_UPDATE_TAG: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Refresh update status from daemon-persisted state (automatic checks run in the daemon).
pub fn init_scheduler() {
    refresh_status_from_state();
    glib::timeout_add_seconds_local(3, move || {
        maybe_present_pending_update();
        glib::ControlFlow::Break
    });
    glib::timeout_add_seconds_local(60 * 60, move || {
        refresh_status_from_state();
        glib::ControlFlow::Continue
    });
}

fn refresh_status_from_state() {
    let state = load_update_state();
    if let Some(avail) = state.available.as_ref() {
        if is_update_newer_than_installed(avail) {
            let msg = tr_fmt(
                "Version {version} is available.",
                &[("version", &avail.version)],
            );
            refresh_status_row_subtitle(&msg);
            return;
        }
    }
    if let Some(err) = state.last_error.as_ref() {
        refresh_status_row_subtitle(&tr_fmt(
            "Last update check failed: {detail}",
            &[("detail", err)],
        ));
        return;
    }
    refresh_status_row_subtitle(&tr("You are running the latest version."));
}

/// Background check (daemon already persisted state); may show the update dialog.
#[allow(dead_code)]
pub fn run_check(channel: UpdateChannel, notify: bool) {
    refresh_status_row_subtitle(&tr("Checking for updates…"));
    spawn_check_task(channel, move |outcome| {
        handle_check_outcome(outcome, notify, false);
    });
}

/// Manual check from settings/about; always offers the update dialog when a release is found.
pub fn run_check_interactive(channel: UpdateChannel, notify: bool, interactive: bool) {
    refresh_status_row_subtitle(&tr("Checking for updates…"));
    if interactive {
        util::show_toast(&tr("Checking for updates…"));
    }
    spawn_check_task(channel, move |outcome| {
        handle_check_outcome(outcome, notify, interactive);
    });
}

fn handle_check_outcome(outcome: UpdateCheckOutcome, notify: bool, interactive: bool) {
    match &outcome {
        UpdateCheckOutcome::UpToDate => {
            let msg = tr("You are running the latest version.");
            refresh_status_row_subtitle(&msg);
            if interactive {
                util::show_toast(&msg);
            }
        }
        UpdateCheckOutcome::UpdateAvailable { availability: info } => {
            let msg = tr_fmt(
                "Version {version} is available.",
                &[("version", &info.version)],
            );
            refresh_status_row_subtitle(&msg);
            if interactive {
                util::show_toast(&msg);
            }
            let should_prompt = interactive
                || (notify && should_prompt_update_dialog(info));
            if should_prompt {
                try_present_update_dialog(info.clone());
            }
            if notify && !interactive {
                let settings = load_app_settings();
                let state = load_update_state();
                if settings.updates.notify_when_available
                    && settings.notifications.notify_on_update
                    && settings.notifications.enabled
                    && should_notify_user(&state, info)
                {
                    send_update_notification(info);
                }
            }
        }
        UpdateCheckOutcome::Error { message: msg } => {
            let line = tr_fmt("Update check failed: {detail}", &[("detail", msg)]);
            refresh_status_row_subtitle(&line);
            if interactive {
                util::show_toast(&line);
            }
        }
    }
}

fn should_prompt_update_dialog(info: &UpdateAvailability) -> bool {
    let state = load_update_state();
    if !should_notify_user(&state, info) {
        return false;
    }
    PROMPTED_UPDATE_TAG.with(|slot| {
        if slot.borrow().as_deref() == Some(info.tag.as_str()) {
            return false;
        }
        *slot.borrow_mut() = Some(info.tag.clone());
        true
    })
}

fn maybe_present_pending_update() {
    let state = load_update_state();
    let Some(avail) = state.available.as_ref() else {
        return;
    };
    if !is_update_newer_than_installed(avail) {
        return;
    }
    let settings = load_app_settings();
    if !settings.updates.notify_when_available {
        return;
    }
    if should_prompt_update_dialog(avail) {
        try_present_update_dialog(avail.clone());
    }
}

fn try_present_update_dialog(info: UpdateAvailability) {
    let Some(parent) = window::main_window() else {
        return;
    };
    present_update_dialog(&parent, info);
}

fn send_update_notification(info: &UpdateAvailability) {
    let summary = tr("New version available");
    let body = tr_fmt(
        "Version {version} is available.",
        &[("version", &info.version)],
    );
    send_desktop_notification(&summary, &body);
}

pub fn register_about_status_row(row: &libadwaita::ActionRow) {
    UPDATE_STATUS_ROW.with(|slot| {
        *slot.borrow_mut() = Some(row.downgrade());
    });
    apply_status_from_state();
}

pub fn apply_status_from_state() {
    let state = load_update_state();
    if let Some(err) = &state.last_error {
        refresh_status_row_subtitle(&tr_fmt(
            "Last check failed: {detail}",
            &[("detail", err)],
        ));
        return;
    }
    if let Some(av) = &state.available {
        if backuppilot_core::is_update_newer_than_installed(av) {
            refresh_status_row_subtitle(&tr_fmt(
                "Version {version} is available.",
                &[("version", &av.version)],
            ));
            return;
        }
    }
    refresh_status_row_subtitle(&tr_fmt(
        "Installed version {version}",
        &[("version", installed_version())],
    ));
}

fn refresh_status_row_subtitle(text: &str) {
    UPDATE_STATUS_ROW.with(|slot| {
        if let Some(weak) = slot.borrow().as_ref() {
            if let Some(row) = weak.upgrade() {
                row.set_subtitle(text);
            }
        }
    });
}

pub fn connect_check_button(row: &libadwaita::ActionRow) {
    let row_weak = row.downgrade();
    row.connect_activated(move |_| {
        if let Some(row) = row_weak.upgrade() {
            row.set_subtitle(&tr("Checking for updates…"));
        }
        let settings = load_app_settings();
        run_check_interactive(settings.updates.channel, false, true);
    });
}

/// Ask whether to update; native installs download after confirmation, Flatpak opens the release page.
pub fn present_update_dialog(parent: &ApplicationWindow, info: UpdateAvailability) {
    let notes_block = info
        .notes
        .as_deref()
        .filter(|n| !n.is_empty())
        .map(|notes| format!("\n\n{notes}"))
        .unwrap_or_default();

    if can_install_update_packages() {
        let body = tr_fmt(
            "Version {version} is available. Do you want to update now?{notes}",
            &[("version", &info.version), ("notes", &notes_block)],
        );
        let alert = libadwaita::AlertDialog::builder()
            .heading(&tr("New version available"))
            .body(&body)
            .build();
        alert.add_response("later", &tr("Not now"));
        alert.add_response("update", &tr("Update now"));
        alert.set_response_appearance("update", libadwaita::ResponseAppearance::Suggested);

        let parent_install = parent.clone();
        let info_install = info.clone();
        alert.connect_response(None, move |_, response| {
            if response == "update" {
                start_install_flow(&parent_install, info_install.clone());
            } else {
                dismiss_available_update();
            }
        });
        alert.present(Some(parent));
        return;
    }

    if is_flatpak_runtime() {
        let body = tr_fmt(
            "Version {version} is available. Open the release page to download the Flatpak bundle or installation instructions.{notes}",
            &[("version", &info.version), ("notes", &notes_block)],
        );
        let alert = libadwaita::AlertDialog::builder()
            .heading(&tr("New version available"))
            .body(&body)
            .build();
        alert.add_response("later", &tr("Not now"));
        alert.add_response("open", &tr("Open release page"));
        alert.set_response_appearance("open", libadwaita::ResponseAppearance::Suggested);
        let uri = info.release_url.clone();
        let parent_uri = parent.clone();
        alert.connect_response(None, move |_, response| {
            if response == "open" {
                open_uri(&parent_uri, &uri);
            } else {
                dismiss_available_update();
            }
        });
        alert.present(Some(parent));
        return;
    }

    let alert = libadwaita::AlertDialog::builder()
        .heading(&tr("Updates not supported"))
        .body(&tr(
            "Automatic installation is only available on Debian/Ubuntu (.deb) and Fedora/RHEL (.rpm) systems.",
        ))
        .build();
    alert.add_response("open", &tr("Open release page"));
    alert.add_response("close", &tr("Close"));
    alert.set_response_appearance("open", libadwaita::ResponseAppearance::Suggested);
    let uri = info.release_url.clone();
    let parent_uri = parent.clone();
    alert.connect_response(None, move |_, response| {
        if response == "open" {
            open_uri(&parent_uri, &uri);
        }
    });
    alert.present(Some(parent));
}

fn start_install_flow(parent: &ApplicationWindow, info: UpdateAvailability) {
    let toast = window::toast_overlay();
    if let Some(toast) = toast.as_ref() {
        toast.add_toast(Toast::new(&tr("Downloading update…")));
    }

    let parent_dl = parent.clone();
    let info_dl = info.clone();
    spawn_update_result_task(
        async move { download_package(&info_dl, |_done, _total| {}).await },
        move |result| {
            match result {
                Ok(path) => {
                    if let Some(toast) = window::toast_overlay().as_ref() {
                        toast.add_toast(Toast::new(&tr("Installing update…")));
                    }
                    match install_package(&path) {
                        Ok(()) => show_install_success(&parent_dl),
                        Err(err) => show_install_error(&parent_dl, &err.to_string()),
                    }
                }
                Err(err) => show_install_error(&parent_dl, &err.to_string()),
            }
        },
    );
}

fn show_install_success(parent: &ApplicationWindow) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(&tr("Update installed"))
        .body(&tr(
            "Restart BackupPilot to use the new version. The background service will be restarted.",
        ))
        .build();
    alert.add_response("later", &tr("Later"));
    alert.add_response("restart", &tr("Restart now"));
    alert.set_response_appearance("restart", libadwaita::ResponseAppearance::Suggested);
    alert.connect_response(None, move |_, response| {
        if response == "restart" {
            window::restart_application();
        }
    });
    alert.present(Some(parent));
    apply_status_from_state();
}

fn show_install_error(parent: &ApplicationWindow, detail: &str) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(&tr("Update failed"))
        .body(&tr_fmt(
            "Could not install the update: {detail}",
            &[("detail", detail)],
        ))
        .build();
    alert.add_response("ok", &tr("OK"));
    alert.present(Some(parent));
}

pub fn open_release_page(parent: &ApplicationWindow) {
    let state = load_update_state();
    let uri = state
        .available
        .as_ref()
        .map(|a| a.release_url.as_str())
        .unwrap_or(GITLAB_PROJECT_URL);
    open_uri(parent, uri);
}

/// Open the update dialog when a pending release is already known (e.g. dashboard banner).
pub fn present_pending_update_if_any() {
    let state = load_update_state();
    let Some(avail) = state.available.clone() else {
        return;
    };
    if !is_update_newer_than_installed(&avail) {
        return;
    }
    try_present_update_dialog(avail);
}

fn open_uri(parent: &ApplicationWindow, uri: &str) {
    let launcher = gtk::UriLauncher::new(uri);
    let parent = parent.clone();
    let uri = uri.to_string();
    launcher.launch(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            if let Err(err) = result {
                tracing::warn!(%err, %uri, "failed to open link");
            }
        },
    );
}

fn spawn_check_task(
    channel: UpdateChannel,
    on_result: impl FnOnce(UpdateCheckOutcome) + 'static,
) {
    crate::dbus_runtime::spawn(
        async move {
            let proxy = crate::dbus_client::connect().await?;
            crate::dbus_client::check_for_updates(&proxy, channel).await
        },
        move |result| {
            let outcome = result.unwrap_or_else(|err| UpdateCheckOutcome::Error {
                message: err.to_string(),
            });
            on_result(outcome);
        },
    );
}

fn spawn_update_result_task<F, T>(
    future: F,
    on_result: impl FnOnce(Result<T, UpdateError>) + 'static,
)
where
    F: std::future::Future<Output = Result<T, UpdateError>> + Send + 'static,
    T: Send + 'static,
{
    glib::spawn_future_local(async move {
        let join = gtk::gio::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime for updates");
            rt.block_on(future)
        });

        match join.await {
            Ok(result) => on_result(result),
            Err(err) => on_result(Err(UpdateError::Network(format!(
                "background task failed: {err:?}"
            )))),
        }
    });
}
