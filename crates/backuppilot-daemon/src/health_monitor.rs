//! Periodic health checks and desktop notifications for overdue backups.

use std::time::Duration;

use backuppilot_core::app_settings::load_app_settings;
use backuppilot_core::health_notify::{
    health_state_key, load_health_notify_state, save_health_notify_state,
};
use backuppilot_core::notify::send_desktop_notification;
use backuppilot_core::profile::HealthState;
use backuppilot_i18n::tr_fmt;
use tracing::{debug, warn};

use crate::service::DaemonService;

const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

pub fn spawn(service: DaemonService) {
    tokio::spawn(async move {
        run_health_monitor(service).await;
    });
}

async fn run_health_monitor(service: DaemonService) {
    let mut interval = tokio::time::interval(CHECK_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;

    loop {
        interval.tick().await;
        if let Err(err) = check_and_notify(&service).await {
            warn!(%err, "health monitor tick failed");
        }
    }
}

async fn check_and_notify(service: &DaemonService) -> backuppilot_core::Result<()> {
    let settings = load_app_settings();
    if !settings.notifications.enabled || !settings.notifications.notify_on_health_warning {
        return Ok(());
    }

    let profiles = service.list_profiles().await?;
    let mut state = load_health_notify_state();

    for profile in profiles {
        if !profile.enabled {
            continue;
        }
        let status = service.status_for_profile(&profile).await?;
        let health = status.health;
        if !matches!(health, HealthState::Warning | HealthState::Critical) {
            state
                .by_profile
                .insert(profile.id.to_string(), health_state_key(health).to_string());
            continue;
        }

        let key = profile.id.to_string();
        let new_key = health_state_key(health);
        let prev = state.by_profile.get(&key).map(String::as_str);
        let escalated = prev != Some(new_key);
        if !escalated {
            continue;
        }

        let days = status.days_since_last_success.unwrap_or(0);
        let (summary, body) = health_notification_text(&profile.name, health, days);
        send_desktop_notification(&summary, &body);
        state.by_profile.insert(key, new_key.to_string());
        debug!(profile_id = profile.id, ?health, days, "health notification sent");
    }

    save_health_notify_state(&state).map_err(|e| {
        backuppilot_core::CoreError::Io(e)
    })?;
    Ok(())
}

fn health_notification_text(name: &str, health: HealthState, days: i64) -> (String, String) {
    match health {
        HealthState::Critical => (
            tr_fmt("Backup overdue: {name}", &[("name", name)]),
            tr_fmt(
                "No successful backup for {days} days (critical threshold reached).",
                &[("days", &days.to_string())],
            ),
        ),
        HealthState::Warning => (
            tr_fmt("Backup warning: {name}", &[("name", name)]),
            tr_fmt(
                "No successful backup for {days} days.",
                &[("days", &days.to_string())],
            ),
        ),
        _ => (String::new(), String::new()),
    }
}
