//! Periodic evaluation of profile schedules and backup triggers.

use std::time::Duration;

use backuppilot_core::profile::{ExecutionContext, ScheduleType};
use backuppilot_core::schedule::{due_schedule_slot, on_login_slot, ScheduleSlot};
use chrono::{Local, Timelike};
use tracing::{debug, info, warn};

use crate::service::DaemonService;

const TICK_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn(service: DaemonService) {
    tokio::spawn(async move {
        run_scheduler(service).await;
    });
}

async fn run_scheduler(service: DaemonService) {
    run_on_login_backups(&service).await;

    info!(
        interval_secs = TICK_INTERVAL.as_secs(),
        "backup scheduler started"
    );

    let start = tokio::time::Instant::now() + delay_to_next_minute();
    let mut interval = tokio::time::interval_at(start, TICK_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;
        tick(&service).await;
    }
}

async fn run_on_login_backups(service: &DaemonService) {
    tokio::time::sleep(Duration::from_secs(15)).await;

    let profiles = match service.list_profiles().await {
        Ok(p) => p,
        Err(err) => {
            warn!(%err, "scheduler: could not list profiles for on-login backups");
            return;
        }
    };

    for profile in profiles {
        if !profile.enabled || profile.schedule.schedule_type != ScheduleType::OnLogin {
            continue;
        }
        if profile.conditions.execution_context == ExecutionContext::HostCli {
            continue;
        }

        let slot = on_login_slot(profile.id);
        trigger_scheduled_backup(service, profile.id, &slot, ScheduleType::OnLogin).await;
    }
}

async fn tick(service: &DaemonService) {
    process_pending_retries(service).await;

    let now = Local::now();
    let profiles = match service.list_profiles().await {
        Ok(p) => p,
        Err(err) => {
            warn!(%err, "scheduler: could not list profiles");
            return;
        }
    };

    for profile in profiles {
        if profile.conditions.execution_context == ExecutionContext::HostCli {
            continue;
        }

        let Some(slot) = due_schedule_slot(&profile, now) else {
            continue;
        };

        if service.scheduler_slot_fired(profile.id, &slot).await.unwrap_or(false) {
            continue;
        }

        trigger_scheduled_backup(
            service,
            profile.id,
            &slot,
            profile.schedule.schedule_type,
        )
        .await;
    }
}

async fn trigger_scheduled_backup(
    service: &DaemonService,
    profile_id: i64,
    slot: &ScheduleSlot,
    schedule_type: ScheduleType,
) {
    debug!(profile_id, %slot, ?schedule_type, "scheduler: triggering backup");

    match service.run_scheduled_backup(profile_id).await {
        Ok(start) => {
            if start.already_running {
                debug!(profile_id, "scheduled backup skipped: already running");
                return;
            }
            info!(
                profile_id,
                run_id = start.run_id,
                started = start.started,
                skipped = start.skipped,
                retryable = start.retryable,
                ?schedule_type,
                %slot,
                "scheduled backup handled"
            );
            if start.started || !start.retryable {
                let _ = service.record_scheduler_slot(profile_id, slot).await;
            }
        }
        Err(err) => {
            warn!(profile_id, %err, "scheduled backup could not start");
            let _ = service.record_scheduler_slot(profile_id, slot).await;
        }
    }
}

async fn process_pending_retries(service: &DaemonService) {
    service.process_due_retries().await;
}

fn delay_to_next_minute() -> Duration {
    let now = Local::now();
    let secs_into_minute = now.second() as u64;
    if secs_into_minute == 0 {
        Duration::ZERO
    } else {
        Duration::from_secs(60 - secs_into_minute)
    }
}
