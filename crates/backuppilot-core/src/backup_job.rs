//! Shared backup execution for the daemon and `backuppilot-cli` (writes `runs`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::db::Database;
use crate::error::{CoreError, Result};
use crate::health::{is_backup_cancelled, is_run_in_progress, stored_backup_failure_message};
use crate::pbs::{backup_process_running, parse_snapshot_id_from_stderr, BackupResult, PbsClient};
use crate::preflight::{run_preflight, PreflightOptions, PreflightReport};
use crate::profile::{BackupProfile, BackupRun, RunStatus};
use crate::secrets::hydrate_profile_repository;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningBackupInfo {
    pub profile_id: i64,
    pub profile_name: String,
    pub run_id: i64,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct BackupJobOutcome {
    pub run_id: i64,
    pub status: RunStatus,
    pub bytes_uploaded: u64,
    pub snapshot_id: Option<String>,
    pub message: Option<String>,
    pub already_running: bool,
}

/// Whether this profile has an open backup run in the DB or a live PBS process in this process.
pub fn profile_backup_in_progress(db: &Database, profile_id: i64) -> Result<bool> {
    if backup_process_running(profile_id) {
        return Ok(true);
    }
    Ok(db
        .latest_run_for_profile(profile_id)?
        .is_some_and(|r| run_is_open(&r)))
}

pub fn run_is_open(run: &BackupRun) -> bool {
    run.finished_at.is_none() && is_run_in_progress(Some(run.status))
}

/// All profiles with a `running` / `pending` run that has not finished yet.
pub fn list_running_backups(db: &Database) -> Result<Vec<RunningBackupInfo>> {
    let mut running = Vec::new();
    for profile in db.list_profiles()? {
        let Some(run) = db.latest_run_for_profile(profile.id)? else {
            continue;
        };
        if !run_is_open(&run) {
            continue;
        }
        running.push(RunningBackupInfo {
            profile_id: profile.id,
            profile_name: profile.name,
            run_id: run.id,
            started_at: run.started_at,
        });
    }
    running.sort_by(|a, b| a.profile_name.cmp(&b.profile_name));
    Ok(running)
}

fn load_profile_for_backup(db: &Database, profile_id: i64) -> Result<BackupProfile> {
    let mut profile = db.get_profile(profile_id)?;
    profile.repository = hydrate_profile_repository(profile_id, &profile.repository)?;
    Ok(profile)
}

fn already_running_outcome(db: &Database, profile_id: i64, message: String) -> Result<BackupJobOutcome> {
    let run = db.insert_run(profile_id, RunStatus::Skipped)?;
    let run = db.finish_run(run.id, RunStatus::Skipped, Some(message.clone()), 0, None)?;
    Ok(BackupJobOutcome {
        run_id: run.id,
        status: RunStatus::Skipped,
        bytes_uploaded: 0,
        snapshot_id: None,
        message: Some(message),
        already_running: true,
    })
}

/// Runs preflight, records a `runs` row, and executes `proxmox-backup-client backup`.
pub async fn execute_profile_backup(
    profile_id: i64,
    preflight_options: PreflightOptions,
) -> Result<BackupJobOutcome> {
    let db = Database::open()?;
    let profile = load_profile_for_backup(&db, profile_id)?;

    if profile_backup_in_progress(&db, profile_id)? {
        return already_running_outcome(
            &db,
            profile_id,
            "A backup for this profile is already running.".into(),
        );
    }

    let preflight: PreflightReport = run_preflight(&profile, preflight_options).await;
    if !preflight.ok {
        let message = preflight.message();
        let run = db.insert_run(profile_id, RunStatus::Skipped)?;
        let run = db.finish_run(run.id, RunStatus::Skipped, Some(message.clone()), 0, None)?;
        return Ok(BackupJobOutcome {
            run_id: run.id,
            status: RunStatus::Skipped,
            bytes_uploaded: 0,
            snapshot_id: None,
            message: Some(message),
            already_running: false,
        });
    }

    let run = db.insert_run(profile_id, RunStatus::Running)?;
    let run_id = run.id;
    let backup_id = profile.backup_id.clone();
    info!(profile_id, run_id, %backup_id, "backup started (backup_job)");

    let result = PbsClient::run_backup_with_progress(&profile, |_| {}).await;

    finish_backup_run(&db, profile_id, run_id, &backup_id, result)
}

fn finish_backup_run(
    db: &Database,
    profile_id: i64,
    run_id: i64,
    backup_id: &str,
    result: Result<BackupResult>,
) -> Result<BackupJobOutcome> {
    let (status, error_message, bytes_uploaded, snapshot_id) = match &result {
        Ok(pbs_result)
            if pbs_result.exit_code == 0
                && !is_backup_cancelled(&pbs_result.stderr, pbs_result.exit_code) =>
        {
            let snapshot_id = parse_snapshot_id_from_stderr(&pbs_result.stderr, backup_id);
            (RunStatus::Success, None, pbs_result.bytes_uploaded, snapshot_id)
        }
        Ok(pbs_result) if is_backup_cancelled(&pbs_result.stderr, pbs_result.exit_code) => (
            RunStatus::Cancelled,
            Some("Backup cancelled".into()),
            pbs_result.bytes_uploaded,
            None,
        ),
        Ok(pbs_result) => {
            let raw = if pbs_result.stderr.is_empty() {
                format!("exit code {}", pbs_result.exit_code)
            } else {
                pbs_result.stderr.clone()
            };
            (
                RunStatus::Failed,
                stored_backup_failure_message(&raw),
                pbs_result.bytes_uploaded,
                None,
            )
        }
        Err(err) => (RunStatus::Failed, Some(err.to_string()), 0, None),
    };

    let run = db.finish_run(
        run_id,
        status,
        error_message.clone(),
        bytes_uploaded,
        snapshot_id.clone(),
    )?;

    if status == RunStatus::Success {
        let _ = db.clear_pending_retry(profile_id);
    }

    Ok(BackupJobOutcome {
        run_id: run.id,
        status,
        bytes_uploaded,
        snapshot_id,
        message: error_message,
        already_running: false,
    })
}

/// Resolves a profile by numeric id or exact name (case-sensitive).
pub fn resolve_profile_id(db: &Database, name_or_id: &str) -> Result<i64> {
    if let Ok(id) = name_or_id.parse::<i64>() {
        if db.get_profile(id).is_ok() {
            return Ok(id);
        }
    }
    db.find_profile_id_by_name(name_or_id)?
        .ok_or_else(|| CoreError::PbsCommand(format!("profile not found: {name_or_id}")))
}
