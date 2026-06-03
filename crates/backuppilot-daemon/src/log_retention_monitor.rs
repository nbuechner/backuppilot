//! Periodically delete old backup runs and system events from the activity log.

use std::time::Duration;

use tracing::warn;

use crate::service::DaemonService;

const PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub fn spawn(service: DaemonService) {
    tokio::spawn(async move {
        run_log_retention_loop(service).await;
    });
}

async fn run_log_retention_loop(service: DaemonService) {
    if let Err(err) = service.prune_old_activity_log().await {
        warn!(%err, "initial activity log prune failed");
    }

    let mut interval = tokio::time::interval(PRUNE_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        if let Err(err) = service.prune_old_activity_log().await {
            warn!(%err, "activity log prune failed");
        }
    }
}
