use chrono::Utc;

use crate::profile::{BackupProfile, HealthCheck, HealthState, ProfileStatus, RunStatus};

pub fn compute_health(last_success_days_ago: Option<i64>, health_check: &HealthCheck) -> HealthState {
    let Some(days) = last_success_days_ago else {
        return HealthState::Unknown;
    };

    if days >= health_check.critical_after_days as i64 {
        HealthState::Critical
    } else if days >= health_check.warn_after_days as i64 {
        HealthState::Warning
    } else {
        HealthState::Ok
    }
}

pub fn days_since_last_success(
    last_success_finished_at: Option<chrono::DateTime<Utc>>,
) -> Option<i64> {
    let finished = last_success_finished_at?;
    let elapsed = Utc::now().signed_duration_since(finished);
    Some(elapsed.num_days())
}

pub fn build_profile_status(
    profile: &BackupProfile,
    last_run: Option<crate::profile::BackupRun>,
    backup_in_progress: bool,
    last_success_finished_at: Option<chrono::DateTime<Utc>>,
) -> ProfileStatus {
    let health = compute_health(
        days_since_last_success(last_success_finished_at),
        &profile.health_check,
    );

    ProfileStatus {
        profile_id: profile.id,
        profile_name: profile.name.clone(),
        enabled: profile.enabled,
        health,
        last_run,
        backup_in_progress,
        progress_message: None,
        days_since_last_success: days_since_last_success(last_success_finished_at),
    }
}

pub fn is_run_in_progress(status: Option<RunStatus>) -> bool {
    matches!(
        status,
        Some(RunStatus::Running | RunStatus::Pending)
    )
}

pub fn is_backup_cancelled(stderr: &str, exit_code: i32) -> bool {
    stderr.contains("cancelled by user") || exit_code == 130 || exit_code == 143
}

/// PBS could not start because another backup holds the datastore lock.
pub fn is_pbs_backup_lock_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("unable to acquire backup group lock")
        || lower.contains("acquire backup group lock")
}

/// Full proxmox-backup-client protocol output (progress lines + errors), not a single user message.
pub fn looks_like_pbs_backup_stderr(message: &str) -> bool {
    message.contains("Starting backup:")
        || message.contains("Starting backup protocol:")
        || message.contains("Client name:")
        || is_pbs_backup_lock_error(message)
}

/// Last `Error:` line from PBS stderr, if any.
pub fn extract_last_pbs_error_line(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("Error:")
                .map(|rest| rest.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .next_back()
}

/// Message persisted on a failed backup run (never the full PBS session log).
pub fn stored_backup_failure_message(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_pbs_backup_lock_error(trimmed) {
        return None;
    }
    if let Some(line) = extract_last_pbs_error_line(trimmed) {
        if is_pbs_backup_lock_error(&line) {
            return None;
        }
        return Some(line);
    }
    if looks_like_pbs_backup_stderr(trimmed) {
        return None;
    }
    if trimmed.lines().count() > 1 && trimmed.len() > 200 {
        return trimmed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .next_back()
            .map(str::to_string);
    }
    Some(trimmed.to_string())
}

/// Short reason for UI toasts and subtitles; `None` means show only the profile title.
pub fn display_backup_error_message(stored: Option<&str>) -> Option<String> {
    let raw = stored?.trim();
    if raw.is_empty() {
        return None;
    }
    stored_backup_failure_message(raw).or_else(|| {
        if is_pbs_backup_lock_error(raw) || looks_like_pbs_backup_stderr(raw) {
            None
        } else {
            Some(raw.to_string())
        }
    })
}

/// Whether a one-line failure reason is worth showing below the title.
pub fn should_show_backup_failure_detail(reason: &str) -> bool {
    reason.chars().count() <= 160 && !looks_like_pbs_backup_stderr(reason)
}

/// Stable key for PBS backup-group locking (repository + namespace + backup id).
pub fn backup_group_key(profile: &BackupProfile) -> String {
    format!(
        "{}|{}|{}",
        profile.repository.trim(),
        profile.namespace.as_deref().unwrap_or("").trim(),
        profile.backup_id.trim(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK_STDERR: &str = "\
Starting backup: [OneSystemsGmbH/Clients]:host/Scooter/2026-05-21T00:00:02Z
Client name: Scooter
Starting backup protocol: Thu May 21 02:00:02 2026
Error: unable to acquire backup group lock \"/run/proxmox-backup/locks/DS01/OneSystemsGmbH:Clients/host-Scooter\" while creating locked backup group \"/mnt/datastore/ds01/ns/OneSystemsGmbH/ns/Clients/host/Scooter\"
";

    #[test]
    fn detects_backup_group_lock() {
        assert!(is_pbs_backup_lock_error(LOCK_STDERR));
    }

    #[test]
    fn lock_stderr_is_not_stored_verbatim() {
        assert_eq!(stored_backup_failure_message(LOCK_STDERR), None);
        assert_eq!(display_backup_error_message(Some(LOCK_STDERR)), None);
    }

    #[test]
    fn extracts_last_error_line() {
        let line = extract_last_pbs_error_line(LOCK_STDERR).unwrap();
        assert!(line.contains("unable to acquire backup group lock"));
    }
}
