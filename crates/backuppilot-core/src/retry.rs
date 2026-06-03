//! Scheduled backup retry after retryable preflight failures.

use std::time::Duration;

/// Delay before retrying a backup skipped due to transient conditions (PBS/network).
pub const PREFLIGHT_RETRY_DELAY: Duration = Duration::from_secs(30 * 60);
