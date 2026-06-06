//! Refreshes overview and profile list while backups run.

use std::cell::RefCell;

use backuppilot_core::profile::{BackupRun, BackupStartResult, ProfileStatus, RunStatus};
use backuppilot_core::{display_backup_error_message, should_show_backup_failure_detail};
use chrono::Utc;
use gtk::glib;
use libadwaita::{Toast, ToastOverlay};

use backuppilot_i18n::{tr, tr_fmt};

use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::profiles;
use crate::window;

thread_local! {
    static POLL_SOURCE: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
}

pub fn handle_backup_started(
    result: BackupStartResult,
    toast: &ToastOverlay,
    window: &libadwaita::ApplicationWindow,
    profiles_page: &gtk::Widget,
) {
    if result.skipped {
        let msg = result.message.unwrap_or_else(|| tr("Preflight check failed"));
        let toast_text = if result.already_running {
            msg
        } else {
            tr_fmt("Backup not started: {msg}", &[("msg", &msg)])
        };
        let t = libadwaita::Toast::new(&toast_text);
        t.set_timeout(if result.already_running { 10 } else { 8 });
        toast.add_toast(t);
        profiles::refresh_list(profiles_page, window, toast);
        window::switch_to_overview();
        if result.already_running {
            start_polling(window, profiles_page, toast);
        }
        return;
    }

    if result.started {
        let t = Toast::new(&tr("Backup running , see Overview for progress. Large backups can take a while."));
        t.set_timeout(6);
        toast.add_toast(t);
        window::switch_to_overview();
        profiles::refresh_list(profiles_page, window, toast);
        start_polling(window, profiles_page, toast);
    }
}

fn start_polling(
    window: &libadwaita::ApplicationWindow,
    profiles_page: &gtk::Widget,
    toast: &ToastOverlay,
) {
    stop_polling();

    let window = window.clone();
    let profiles_page = profiles_page.clone();
    let toast = toast.clone();

    let source = glib::timeout_add_seconds_local(2, move || {
        poll_once(window.clone(), profiles_page.clone(), toast.clone());
        glib::ControlFlow::Continue
    });

    POLL_SOURCE.with(|slot| {
        *slot.borrow_mut() = Some(source);
    });
}

fn poll_once(
    window: libadwaita::ApplicationWindow,
    profiles_page: gtk::Widget,
    toast: ToastOverlay,
) {
    dbus_runtime::spawn(
        async move { daemon_list_statuses().await },
        move |result| {
            let Ok(statuses) = result else { return };

            window::refresh_dashboard_public();
            profiles::refresh_list(&profiles_page, &window, &toast);

            let still_running = statuses.iter().any(|s| s.backup_in_progress);
            if !still_running {
                stop_polling();
                if let Some((name, finished)) = find_finished_run(&statuses) {
                    show_finished_toast(&toast, &name, finished);
                }
                window::refresh_dashboard_public();
            }
        },
    );
}

fn find_finished_run(statuses: &[ProfileStatus]) -> Option<(String, &BackupRun)> {
    statuses.iter().find_map(|s| {
        let run = s.last_run.as_ref()?;
        if s.backup_in_progress || !run_just_finished(run) {
            return None;
        }
        match run.status {
            RunStatus::Success | RunStatus::Failed | RunStatus::Skipped | RunStatus::Cancelled => {
                Some((s.profile_name.clone(), run))
            }
            _ => None,
        }
    })
}

fn run_just_finished(run: &BackupRun) -> bool {
    let Some(finished) = run.finished_at else {
        return false;
    };
    Utc::now()
        .signed_duration_since(finished)
        .num_seconds()
        < 60
}

fn show_finished_toast(toast: &ToastOverlay, profile_name: &str, run: &BackupRun) {
    let (title, timeout) = match run.status {
        RunStatus::Success => (
            tr_fmt("Backup completed: {name}", &[("name", profile_name)]),
            5,
        ),
        RunStatus::Skipped => {
            let skipped = tr("skipped");
            let reason = run.error_message.as_deref().unwrap_or(&skipped);
            (
                tr_fmt("Backup skipped ({name}): {reason}", &[
                    ("name", profile_name),
                    ("reason", reason),
                ]),
                8,
            )
        }
        RunStatus::Failed => {
            let title = display_backup_error_message(run.error_message.as_deref())
                .filter(|reason| should_show_backup_failure_detail(reason))
                .map(|reason| {
                    tr_fmt("Backup failed ({name}): {reason}", &[
                        ("name", profile_name),
                        ("reason", &reason),
                    ])
                })
                .unwrap_or_else(|| {
                    tr_fmt("Backup «{name}» failed", &[("name", profile_name)])
                });
            (title, 8)
        }
        _ => return,
    };
    let t = libadwaita::Toast::new(&title);
    t.set_timeout(timeout);
    toast.add_toast(t);
}

pub fn stop_polling() {
    POLL_SOURCE.with(|slot| {
        if let Some(id) = slot.borrow_mut().take() {
            id.remove();
        }
    });
}

async fn daemon_list_statuses() -> backuppilot_ipc::Result<Vec<ProfileStatus>> {
    let proxy = connect().await?;
    dbus_client::list_statuses(&proxy).await
}
