//! Start or stop backups via D-Bus (profile list, dashboard, and tray menu).

use libadwaita::{ApplicationWindow, Toast, ToastOverlay};

use backuppilot_i18n::{tr, tr_fmt};

use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::status_poll;
use crate::window;

/// Starts a backup without opening the main window (tray menu).
pub fn start_backup_from_tray(profile_id: i64) {
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::run_backup(&proxy, profile_id).await
        },
        move |result| {
            if let Err(err) = result {
                tracing::warn!(%err, profile_id, "tray: failed to start backup");
            }
        },
    );
}

/// Stops a running backup without opening the main window (tray menu).
pub fn cancel_backup_from_tray(profile_id: i64) {
    cancel_backup(profile_id, None, None);
}

/// Request cancellation of a running backup and refresh the UI.
pub fn cancel_backup(
    profile_id: i64,
    toast: Option<&ToastOverlay>,
    profiles_page: Option<&gtk::Widget>,
) {
    let toast_owned = toast.cloned();
    let profiles_page = profiles_page.map(|w| w.clone());

    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::cancel_backup(&proxy, profile_id).await
        },
        move |result| {
            match result {
                Ok(true) => {
                    if let Some(ref toast) = toast_owned {
                        let t = Toast::new(&tr("Backup cancellation requested…"));
                        t.set_timeout(4);
                        toast.add_toast(t);
                    }
                    window::refresh_dashboard_public();
                    if let (Some(page), Some(window)) =
                        (profiles_page.as_ref(), window::main_window().as_ref())
                    {
                        if let Some(toast) = toast_owned.as_ref() {
                            crate::profiles::refresh_list(page, window, toast);
                        }
                    }
                }
                Ok(false) => {
                    if let Some(ref toast) = toast_owned {
                        toast.add_toast(Toast::new(&tr("No backup is running for this profile.")));
                    }
                }
                Err(err) => {
                    if let Some(ref toast) = toast_owned {
                        let t = Toast::new(&tr_fmt(
                            "Could not cancel backup: {err}",
                            &[("err", &err.to_string())],
                        ));
                        t.set_timeout(6);
                        toast.add_toast(t);
                    }
                }
            }
        },
    );
}

/// Stops every running backup (overview «Cancel all»).
pub fn cancel_all_running_backups(toast: Option<&ToastOverlay>) {
    let toast_owned = toast.cloned();

    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::cancel_all_running_backups(&proxy).await
        },
        move |result| {
            match result {
                Ok(count) if count > 0 => {
                    if let Some(ref toast) = toast_owned {
                        let t = Toast::new(&tr_fmt(
                            "Stopping {count} backup(s)…",
                            &[("count", &count.to_string())],
                        ));
                        t.set_timeout(4);
                        toast.add_toast(t);
                    }
                    window::refresh_dashboard_public();
                }
                Ok(_) => {
                    if let Some(ref toast) = toast_owned {
                        toast.add_toast(Toast::new(&tr("No backup is currently running.")));
                    }
                }
                Err(err) => {
                    if let Some(ref toast) = toast_owned {
                        let t = Toast::new(&tr_fmt(
                            "Could not cancel backups: {err}",
                            &[("err", &err.to_string())],
                        ));
                        t.set_timeout(6);
                        toast.add_toast(t);
                    }
                }
            }
        },
    );
}

pub fn start_backup(
    profile_id: i64,
    window: &ApplicationWindow,
    toast: &ToastOverlay,
    profiles_page: &gtk::Widget,
) {
    let toast = toast.clone();
    let window = window.clone();
    let profiles_page = profiles_page.clone();

    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::run_backup(&proxy, profile_id).await
        },
        move |result| {
            match result {
                Ok(start) => {
                    status_poll::handle_backup_started(start, &toast, &window, &profiles_page);
                }
                Err(err) => {
                    let t = Toast::new(&tr_fmt(
                        "Failed to start backup: {err}",
                        &[("err", &err.to_string())],
                    ));
                    t.set_timeout(6);
                    toast.add_toast(t);
                }
            }
        },
    );
}
