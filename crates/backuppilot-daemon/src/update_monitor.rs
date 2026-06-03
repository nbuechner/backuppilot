//! Automatic GitLab release checks (daily) in the background daemon.

use std::time::Duration;

use backuppilot_core::app_settings::load_app_settings;
use backuppilot_core::app_update_checks_enabled;
use backuppilot_core::notify::send_desktop_notification;
use backuppilot_core::updates::{
    check_for_updates, load_update_state, should_notify_user,
    should_run_automatic_check, UpdateCheckOutcome,
};
use backuppilot_core::installed_version;
use backuppilot_i18n::{tr, tr_fmt};
use tracing::{info, warn};

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub fn spawn() {
    if !app_update_checks_enabled() {
        return;
    }
    tokio::spawn(async move {
        run_update_monitor().await;
    });
}

async fn run_update_monitor() {
    tokio::time::sleep(Duration::from_secs(120)).await;
    let mut interval = tokio::time::interval(CHECK_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tick().await;
        interval.tick().await;
    }
}

async fn tick() {
    let settings = load_app_settings();
    if !settings.updates.check_automatically {
        return;
    }
    let state = load_update_state();
    if !should_run_automatic_check(&state) {
        return;
    }

    let outcome = check_for_updates(settings.updates.channel).await;
    handle_outcome(outcome, settings.updates.notify_when_available);
}

fn handle_outcome(outcome: UpdateCheckOutcome, notify: bool) {
    match &outcome {
        UpdateCheckOutcome::UpToDate => {
            info!(version = installed_version(), "no application update available");
        }
        UpdateCheckOutcome::UpdateAvailable { availability: avail } => {
            info!(version = %avail.version, "application update available");
            if notify && should_notify_user(&load_update_state(), avail) {
                let summary = tr("Update available");
                let body = tr_fmt(
                    "Version {version} is available.",
                    &[("version", &avail.version)],
                );
                send_desktop_notification(&summary, &body);
            }
        }
        UpdateCheckOutcome::Error { message: msg } => {
            warn!(%msg, "update check returned error");
        }
    }
}
