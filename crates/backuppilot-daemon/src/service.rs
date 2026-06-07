use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use backuppilot_core::activity_events::{
    RESTORE_FINISHED_KIND, RESTORE_STARTED_KIND, SNAPSHOT_MOUNT_KIND, SNAPSHOT_UNMOUNT_KIND,
};
use backuppilot_core::app_settings::{
    activity_log_cutoff, load_app_settings, save_app_settings, validate_log_retention,
};
use backuppilot_core::db::ActivityPruneResult;
use backuppilot_core::retry::PREFLIGHT_RETRY_DELAY;
use backuppilot_core::pbs::parse_snapshot_id_from_stderr;
use backuppilot_core::db::Database;
use backuppilot_core::reset::wipe_all_local_data;
use backuppilot_core::health::{
    backup_group_key, build_profile_status, display_backup_error_message, is_backup_cancelled,
    is_pbs_backup_lock_error, is_run_in_progress, stored_backup_failure_message,
};
use backuppilot_core::pbs::backup_process_running;
use backuppilot_core::notify;
use backuppilot_core::pbs::PbsClient;
use backuppilot_core::restore::{
    ListCatalogRequest, ListCatalogResponse, PbsRestore, PbsSnapshotInfo, RestoreArchiveRequest,
};
use backuppilot_i18n::{tr, tr_fmt};
use backuppilot_core::profile::{
    normalize_new_profile, redact_profile_for_client, ActivityLogEntry, BackupProfile, BackupRun,
    BackupStartResult, CredentialVerifyInput, CredentialVerifyResult, NewProfile, ProfileStatus,
    RestoreStartResult, RunStatus,
};
use backuppilot_core::schedule::ScheduleSlot;
use backuppilot_core::updates::{
    check_for_updates, dismiss_available_update, load_update_state, save_update_state,
    UpdateCheckOutcome, UpdateState,
};
use backuppilot_core::app_settings::UpdateChannel;
use backuppilot_core::Result;
use chrono::Utc;
use tokio::sync::Mutex;
use tracing::{error, info};

use backuppilot_core::pbs_mount::{
    ActiveMount, MountSnapshotRequest, MountSnapshotResult, UnmountSnapshotResult,
};

use backuppilot_core::MountManager;
use crate::preflight::{run_preflight as core_run_preflight, PreflightOptions, PreflightReport};

#[derive(Clone)]
pub struct DaemonService {
    db: Arc<Mutex<Database>>,
    running_profiles: Arc<Mutex<HashSet<i64>>>,
    /// PBS backup-group lock scope (repository + namespace + backup id).
    active_backup_groups: Arc<Mutex<HashMap<String, i64>>>,
    backup_progress: Arc<std::sync::Mutex<HashMap<i64, String>>>,
    mounts: Arc<MountManager>,
}

impl DaemonService {
    pub fn new(db: Database) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            running_profiles: Arc::new(Mutex::new(HashSet::new())),
            active_backup_groups: Arc::new(Mutex::new(HashMap::new())),
            backup_progress: Arc::new(std::sync::Mutex::new(HashMap::new())),
            mounts: Arc::new(MountManager::new()),
        }
    }

    pub async fn list_active_mounts(&self) -> Result<Vec<ActiveMount>> {
        Ok(self.mounts.list_active().await)
    }

    pub fn check_fuse_available(&self) -> backuppilot_core::FuseCheckResult {
        backuppilot_core::check_fuse()
    }

    pub fn check_windows_setup(&self) -> backuppilot_core::WslSetupStatus {
        backuppilot_core::check_windows_setup()
    }

    pub async fn install_fuse3(&self) -> Result<backuppilot_core::InstallFuse3Result> {
        #[cfg(windows)]
        {
            // On Windows, fuse3 lives in WSL — install it there directly.
            let output = tokio::process::Command::new("wsl")
                .args(["--", "sudo", "apt-get", "install", "-y", "--no-install-recommends", "fuse3"])
                .output()
                .await
                .map_err(backuppilot_core::CoreError::Io)?;
            let ok = output.status.success();
            let message = if ok {
                None
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                Some(format!("Install failed: {}", format!("{stdout}\n{stderr}").trim()))
            };
            return Ok(backuppilot_core::InstallFuse3Result { ok, message });
        }

        #[cfg(not(windows))]
        {
            use backuppilot_core::fuse3_install_script;
            let script = fuse3_install_script().ok_or_else(|| {
                backuppilot_core::CoreError::PbsCommand(
                    "Automatic fuse3 install is not supported on this system.".into(),
                )
            })?;

            let tmp = std::env::temp_dir().join("backuppilot-install-fuse3.sh");
            std::fs::write(&tmp, &script).map_err(backuppilot_core::CoreError::Io)?;

            let output = tokio::process::Command::new("pkexec")
                .arg("bash")
                .arg(&tmp)
                .output()
                .await
                .map_err(backuppilot_core::CoreError::Io)?;

            let ok = output.status.success();
            let message = if ok {
                None
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Some(format!("Install failed: {}", stderr.trim()))
            };
            Ok(backuppilot_core::InstallFuse3Result { ok, message })
        }
    }

    pub async fn mount_snapshot(
        &self,
        request: MountSnapshotRequest,
    ) -> Result<MountSnapshotResult> {
        let profile = self.profile_with_credentials(request.profile_id).await?;
        let profile_name = profile.name.clone();
        let key_id = request.encryption_key_id.or(profile.encryption_key_id);
        let result = self
            .mounts
            .mount(&profile, &request, key_id)
            .await?;

        let status = if result.ok {
            RunStatus::Success
        } else {
            RunStatus::Failed
        };
        let message = if result.ok {
            result.mount.as_ref().map(|m| {
                format!(
                    "Read-only mount at {} ({}, snapshot {})",
                    m.mount_point, m.archive_name, m.snapshot
                )
            })
        } else {
            result.message.clone()
        };
        self.record_system_event(SNAPSHOT_MOUNT_KIND, &profile_name, status, message)
            .await;

        Ok(result)
    }

    pub async fn unmount_snapshot(&self, mount_id: &str) -> Result<UnmountSnapshotResult> {
        let active = self.mounts.list_active().await;
        let session = active.iter().find(|m| m.id == mount_id);
        let profile_name = session
            .map(|m| m.profile_name.as_str())
            .unwrap_or("Snapshot mount");
        let mount_point = session.map(|m| m.mount_point.as_str());

        let result = self.mounts.unmount(mount_id).await?;

        let status = if result.ok {
            RunStatus::Success
        } else {
            RunStatus::Failed
        };
        let message = if result.ok {
            mount_point.map(|p| format!("Disconnected mount at {p}"))
        } else {
            result.message.clone()
        };
        self.record_system_event(SNAPSHOT_UNMOUNT_KIND, profile_name, status, message)
            .await;

        Ok(result)
    }

    /// Deletes activity log entries older than configured in `app-settings.json`.
    pub async fn prune_old_activity_log(&self) -> Result<ActivityPruneResult> {
        let settings = load_app_settings();
        if let Err(message) = validate_log_retention(&settings.log_retention) {
            tracing::warn!(%message, "invalid log retention settings, skipping prune");
            return Ok(ActivityPruneResult::default());
        }
        let Some(cutoff) = activity_log_cutoff(&settings.log_retention) else {
            return Ok(ActivityPruneResult::default());
        };
        let db = self.db.lock().await;
        let result = db.prune_activity_before(cutoff)?;
        if result.total_deleted() > 0 {
            info!(
                runs = result.runs_deleted,
                system_events = result.system_events_deleted,
                retain_days = settings.log_retention.retain_days,
                "pruned old activity log entries"
            );
        }
        Ok(result)
    }

    /// Marks DB runs left as `running` after a crash/restart when no PBS client is active.
    pub async fn reconcile_interrupted_runs(&self) {
        let profiles = match self.list_profiles().await {
            Ok(p) => p,
            Err(err) => {
                error!(%err, "could not reconcile interrupted backup runs");
                return;
            }
        };

        for profile in profiles {
            let active = self.running_profiles.lock().await.contains(&profile.id)
                || backup_process_running(profile.id);
            if active {
                continue;
            }
            let db = self.db.lock().await;
            let Some(run) = db.latest_run_for_profile(profile.id).ok().flatten() else {
                continue;
            };
            if !is_run_in_progress(Some(run.status)) {
                continue;
            }
            let message = tr("Interrupted (application restarted)");
            if let Err(err) = db.finish_run(run.id, RunStatus::Failed, Some(message.clone()), 0, None)
            {
                error!(profile_id = profile.id, run_id = run.id, %err, "failed to reconcile run");
            } else {
                info!(profile_id = profile.id, run_id = run.id, "reconciled interrupted backup run");
            }
        }
    }

    pub async fn list_profiles(&self) -> Result<Vec<BackupProfile>> {
        let profiles = {
            let db = self.db.lock().await;
            db.list_profiles()?
        };
        Ok(profiles.into_iter().map(redact_profile_for_client).collect())
    }

    pub async fn get_profile(&self, profile_id: i64) -> Result<BackupProfile> {
        Ok(redact_profile_for_client(self.profile_with_credentials(profile_id).await?))
    }

    /// Full profile including hydrated API token (daemon-internal only).
    async fn profile_with_credentials(&self, profile_id: i64) -> Result<BackupProfile> {
        let db = self.db.lock().await;
        db.get_profile(profile_id)
    }

    pub async fn list_runs_for_profile(&self, profile_id: i64, limit: u32) -> Result<Vec<BackupRun>> {
        let db = self.db.lock().await;
        db.list_runs_for_profile(profile_id, limit)
    }

    pub async fn run_preflight(&self, profile_id: i64) -> Result<PreflightReport> {
        let profile = self.profile_with_credentials(profile_id).await?;
        Ok(core_run_preflight(&profile, PreflightOptions::default()).await)
    }

    pub async fn apply_global_defaults_to_profiles(&self) -> Result<u32> {
        let settings = load_app_settings();
        let db = self.db.lock().await;
        db.apply_global_conditions_to_profiles(&settings.conditions, &settings.health_check)
    }

    pub async fn scheduler_slot_fired(&self, profile_id: i64, slot: &ScheduleSlot) -> Result<bool> {
        let db = self.db.lock().await;
        db.scheduler_slot_fired(profile_id, slot)
    }

    pub async fn record_scheduler_slot(&self, profile_id: i64, slot: &ScheduleSlot) -> Result<()> {
        let db = self.db.lock().await;
        db.record_scheduler_slot(profile_id, slot)
    }

    pub async fn create_profile(&self, new: NewProfile) -> Result<BackupProfile> {
        let new = normalize_new_profile(new);
        let profile = {
            let db = self.db.lock().await;
            db.insert_profile(&new)?
        };
        Ok(redact_profile_for_client(profile))
    }

    pub async fn update_profile(&self, profile_id: i64, update: NewProfile) -> Result<BackupProfile> {
        let update = normalize_new_profile(update);
        let profile = {
            let db = self.db.lock().await;
            db.update_profile(profile_id, &update)?
        };
        Ok(redact_profile_for_client(profile))
    }

    pub async fn set_profile_enabled(&self, profile_id: i64, enabled: bool) -> Result<BackupProfile> {
        let profile = {
            let db = self.db.lock().await;
            db.set_profile_enabled(profile_id, enabled)?
        };
        Ok(redact_profile_for_client(profile))
    }

    pub async fn delete_profile(&self, profile_id: i64) -> Result<()> {
        self.mounts.unmount_profile(profile_id).await?;
        let db = self.db.lock().await;
        db.delete_profile(profile_id)
    }

    /// Cancel running jobs, delete all local data and open a fresh database.
    pub async fn reset_all_data(&self) -> Result<()> {
        let running: Vec<i64> = self
            .running_profiles
            .lock()
            .await
            .iter()
            .copied()
            .collect();
        for profile_id in running {
            let _ = backuppilot_core::cancel_running_backup(profile_id);
        }
        self.running_profiles.lock().await.clear();
        if let Ok(mut map) = self.backup_progress.lock() {
            map.clear();
        }
        self.mounts.unmount_all().await?;

        {
            let mut db = self.db.lock().await;
            *db = Database::open_in_memory()?;
        }

        wipe_all_local_data().map_err(backuppilot_core::CoreError::Io)?;

        {
            let mut db = self.db.lock().await;
            *db = Database::open()?;
        }

        info!("all local BackupPilot data was reset");
        Ok(())
    }

    pub async fn list_recent_activity(&self, limit: u32) -> Result<Vec<ActivityLogEntry>> {
        let db = self.db.lock().await;
        db.list_recent_activity(limit)
    }

    pub async fn list_statuses(&self) -> Result<Vec<ProfileStatus>> {
        let profiles = self.list_profiles().await?;
        let mut statuses = Vec::with_capacity(profiles.len());

        for profile in profiles {
            statuses.push(self.status_for_profile(&profile).await?);
        }

        Ok(statuses)
    }

    pub async fn status_for_profile(&self, profile: &BackupProfile) -> Result<ProfileStatus> {
        let (last_run, last_success) = {
            let db = self.db.lock().await;
            (
                db.latest_run_for_profile(profile.id)?,
                db.latest_successful_run(profile.id)?,
            )
        }; // db lock released before acquiring running_profiles
        let in_progress = self.running_profiles.lock().await.contains(&profile.id)
            || backup_process_running(profile.id);
        let progress_message = self
            .backup_progress
            .lock()
            .ok()
            .and_then(|map| map.get(&profile.id).cloned());

        let mut status = build_profile_status(
            profile,
            last_run,
            in_progress,
            last_success.and_then(|r| r.finished_at),
        );
        status.progress_message = progress_message;
        Ok(status)
    }

    pub async fn run_backup(&self, profile_id: i64) -> Result<BackupStartResult> {
        self.run_backup_with_options(profile_id, PreflightOptions::default())
            .await
    }

    /// Stops a running backup for `profile_id` (terminates the PBS client process group).
    pub async fn cancel_backup(&self, profile_id: i64) -> Result<bool> {
        if !self.running_profiles.lock().await.contains(&profile_id)
            && !backup_process_running(profile_id)
        {
            return Ok(false);
        }
        if let Ok(mut map) = self.backup_progress.lock() {
            map.insert(profile_id, tr("Cancelling backup…"));
        }
        Ok(backuppilot_core::cancel_running_backup(profile_id))
    }

    /// Stops all running backups (frees bandwidth immediately).
    pub async fn cancel_all_running_backups(&self) -> Result<u32> {
        let ids: Vec<i64> = self
            .running_profiles
            .lock()
            .await
            .iter()
            .copied()
            .collect();
        let mut stopped = 0u32;
        for profile_id in ids {
            if self.cancel_backup(profile_id).await? {
                stopped += 1;
            }
        }
        Ok(stopped)
    }

    /// Scheduled runs retry PBS reachability while the network may still be starting.
    pub async fn run_scheduled_backup(&self, profile_id: i64) -> Result<BackupStartResult> {
        self.run_backup_with_options(
            profile_id,
            PreflightOptions {
                scheduled: true,
            },
        )
        .await
    }

    async fn run_backup_with_options(
        &self,
        profile_id: i64,
        preflight_options: PreflightOptions,
    ) -> Result<BackupStartResult> {
        if crate::preflight::backups_globally_paused() {
            let message = tr("All scheduled backups are paused in settings");
            let run = {
                let db = self.db.lock().await;
                let run = db.insert_run(profile_id, RunStatus::Skipped)?;
                db.finish_run(run.id, RunStatus::Skipped, Some(message.clone()), 0, None)?
            };
            if let Ok(profile) = self.get_profile(profile_id).await {
                notify_backup_finished(&profile.name, RunStatus::Skipped, Some(&message));
            }
            return Ok(BackupStartResult {
                run_id: run.id,
                started: false,
                skipped: true,
                retryable: false,
                already_running: false,
                message: Some(message),
            });
        }

        if let Some(blocked) = self.backup_already_running_result(profile_id).await {
            return Ok(blocked);
        }

        {
            let mut running = self.running_profiles.lock().await;
            if !running.insert(profile_id) {
                // Drop the lock before awaiting — backup_already_running_result
                // also acquires running_profiles, and tokio Mutex is not reentrant.
                drop(running);
                return Ok(
                    self.backup_already_running_result(profile_id)
                        .await
                        .expect("profile was just marked running"),
                );
            }
        }

        let profile = self.profile_with_credentials(profile_id).await?;
        if profile.conditions.execution_context == backuppilot_core::profile::ExecutionContext::HostCli
        {
            self.running_profiles.lock().await.remove(&profile_id);
            let message = tr(
                "This profile runs on the host via backuppilot-cli (systemd/cron). Start it with: backuppilot-cli backup <name>",
            );
            let run = {
                let db = self.db.lock().await;
                let run = db.insert_run(profile_id, RunStatus::Skipped)?;
                db.finish_run(run.id, RunStatus::Skipped, Some(message.clone()), 0, None)?
            };
            return Ok(BackupStartResult {
                run_id: run.id,
                started: false,
                skipped: true,
                retryable: false,
                already_running: false,
                message: Some(message),
            });
        }

        let preflight = core_run_preflight(&profile, preflight_options).await;
        if !preflight.ok {
            self.running_profiles.lock().await.remove(&profile_id);
            let message = preflight.message();
            let retryable = preflight.retryable();
            let run = {
                let db = self.db.lock().await;
                let run = db.insert_run(profile_id, RunStatus::Skipped)?;
                db.finish_run(run.id, RunStatus::Skipped, Some(message.clone()), 0, None)?
            };
            info!(profile_id, run_id = run.id, %message, retryable, "backup skipped by preflight");
            if retryable {
                let retry_after =
                    Utc::now() + chrono::Duration::from_std(PREFLIGHT_RETRY_DELAY).unwrap_or_default();
                let db = self.db.lock().await;
                let _ = db.upsert_pending_retry(profile_id, retry_after, Some(&message));
            } else {
                let db = self.db.lock().await;
                let _ = db.clear_pending_retry(profile_id);
            }
            if let Ok(p) = self.profile_name(profile_id).await {
                notify_backup_finished(&p, RunStatus::Skipped, Some(&message));
            }
            return Ok(BackupStartResult {
                run_id: run.id,
                started: false,
                skipped: true,
                retryable,
                already_running: false,
                message: Some(message),
            });
        }

        let group_key = backup_group_key(&profile);
        {
            let mut groups = self.active_backup_groups.lock().await;
            if groups.contains_key(&group_key) {
                self.running_profiles.lock().await.remove(&profile_id);
                let message = tr("A backup for this backup target is already running on the server or in BackupPilot.");
                let run = {
                    let db = self.db.lock().await;
                    let run = db.insert_run(profile_id, RunStatus::Skipped)?;
                    db.finish_run(run.id, RunStatus::Skipped, Some(message.clone()), 0, None)?
                };
                info!(profile_id, run_id = run.id, %group_key, "backup skipped: backup group busy");
                notify_backup_finished(&profile.name, RunStatus::Skipped, Some(&message));
                return Ok(BackupStartResult {
                    run_id: run.id,
                    started: false,
                    skipped: true,
                    retryable: false,
                    already_running: true,
                    message: Some(message),
                });
            }
            groups.insert(group_key, profile_id);
        }

        {
            let db = self.db.lock().await;
            let _ = db.clear_pending_retry(profile_id);
        }

        let run_id = {
            let db = self.db.lock().await;
            let run = db.insert_run(profile_id, RunStatus::Running)?;
            run.id
        };

        let profile_clone = profile.clone();
        let service = self.clone();
        let progress_map = self.backup_progress.clone();

        if let Ok(mut map) = progress_map.lock() {
            map.insert(profile_id, "Preparing backup…".into());
        }

        let progress_summary = tr_fmt(
            "Backup «{name}»",
            &[("name", &profile_clone.name)],
        );
        let progress_notifier = std::sync::Arc::new(std::sync::Mutex::new(
            notify::BackupProgressNotifier::start(
                progress_summary,
                &tr("Preparing backup…"),
            ),
        ));

        info!(profile_id, run_id, backup_id = %profile.backup_id, "backup started");
        tokio::spawn(async move {
            let notifier = progress_notifier.clone();
            let result = PbsClient::run_backup_with_progress(&profile_clone, |message| {
                if let Ok(mut map) = progress_map.lock() {
                    map.insert(profile_id, message.clone());
                }
                if let Ok(mut slot) = notifier.lock() {
                    if let Some(ref mut n) = *slot {
                        n.update(&message);
                    }
                }
            })
            .await;
            service
                .finish_backup_run(profile_id, run_id, result, progress_notifier)
                .await;
        });

        Ok(BackupStartResult {
            run_id,
            started: true,
            skipped: false,
            retryable: false,
            already_running: false,
            message: None,
        })
    }

    async fn backup_already_running_result(&self, profile_id: i64) -> Option<BackupStartResult> {
        let reserved = self.running_profiles.lock().await.contains(&profile_id);
        let process = backup_process_running(profile_id);
        let open_in_db = {
            let db = self.db.lock().await;
            backuppilot_core::profile_backup_in_progress(&db, profile_id).unwrap_or(false)
        };
        if !reserved && !process && !open_in_db {
            return None;
        }

        let run_id = {
            let db = self.db.lock().await;
            db.latest_run_for_profile(profile_id)
                .ok()
                .flatten()
                .filter(|r| is_run_in_progress(Some(r.status)))
                .map(|r| r.id)
                .unwrap_or(0)
        };

        Some(BackupStartResult {
            run_id,
            started: false,
            skipped: true,
            retryable: false,
            already_running: true,
            message: Some(tr("A backup for this profile is already running. Use Stop to cancel it first.")),
        })
    }

    async fn profile_name(&self, profile_id: i64) -> Result<String> {
        let db = self.db.lock().await;
        Ok(db.get_profile(profile_id)?.name)
    }

    async fn finish_backup_run(
        &self,
        profile_id: i64,
        run_id: i64,
        result: backuppilot_core::Result<backuppilot_core::pbs::BackupResult>,
        progress_notifier: std::sync::Arc<std::sync::Mutex<Option<notify::BackupProgressNotifier>>>,
    ) {
        let profile_name = {
            let db = self.db.lock().await;
            db.get_profile(profile_id).ok().map(|p| p.name)
        };

        let backup_id = {
            let db = self.db.lock().await;
            db.get_profile(profile_id)
                .ok()
                .map(|p| p.backup_id)
                .unwrap_or_default()
        };

        let (status, error_message, bytes_uploaded, snapshot_id) = match &result {
            Ok(pbs_result) if pbs_result.exit_code == 0 && !is_backup_cancelled(&pbs_result.stderr, pbs_result.exit_code) => {
                let snapshot_id =
                    parse_snapshot_id_from_stderr(&pbs_result.stderr, &backup_id);
                info!(
                    profile_id,
                    run_id,
                    bytes = pbs_result.bytes_uploaded,
                    ?snapshot_id,
                    "backup completed successfully"
                );
                (RunStatus::Success, None, pbs_result.bytes_uploaded, snapshot_id)
            }
            Ok(pbs_result) if is_backup_cancelled(&pbs_result.stderr, pbs_result.exit_code) => {
                info!(profile_id, run_id, "backup cancelled");
                (
                    RunStatus::Cancelled,
                    Some(tr("Backup cancelled")),
                    pbs_result.bytes_uploaded,
                    None,
                )
            }
            Ok(pbs_result) => {
                let raw = if pbs_result.stderr.is_empty() {
                    format!("exit code {}", pbs_result.exit_code)
                } else {
                    pbs_result.stderr.clone()
                };
                let message = stored_backup_failure_message(&raw);
                if is_pbs_backup_lock_error(&raw) {
                    error!(profile_id, run_id, "backup failed: backup group lock held");
                } else {
                    error!(profile_id, run_id, ?message, "backup failed");
                }
                (RunStatus::Failed, message, pbs_result.bytes_uploaded, None)
            }
            Err(err) => {
                error!(profile_id, run_id, error = %err, "backup failed");
                (RunStatus::Failed, Some(err.to_string()), 0, None)
            }
        };

        {
            let db = self.db.lock().await;
            let _ = db.finish_run(
                run_id,
                status,
                error_message.clone(),
                bytes_uploaded,
                snapshot_id.clone(),
            );
            if status == RunStatus::Success {
                let _ = db.clear_pending_retry(profile_id);
            }
        }

        if let Some(name) = profile_name.as_deref() {
            let notification_detail = notification_error_detail(
                status,
                error_message.as_deref(),
                result.as_ref().ok().and_then(|pbs| {
                    if pbs.stderr.is_empty() {
                        None
                    } else {
                        Some(pbs.stderr.as_str())
                    }
                }),
            );
            let mut used_progress = false;
            if let Ok(mut slot) = progress_notifier.lock() {
                if let Some(mut notifier) = slot.take() {
                    let (summary, body) =
                        backup_finished_notification_text(name, status, notification_detail.as_deref());
                    notifier.finish(&summary, &body);
                    used_progress = true;
                }
            }
            if !used_progress {
                notify_backup_finished(name, status, notification_detail.as_deref());
            }
        }

        self.running_profiles.lock().await.remove(&profile_id);
        self.active_backup_groups
            .lock()
            .await
            .retain(|_, owner| *owner != profile_id);
        if let Ok(mut map) = self.backup_progress.lock() {
            map.remove(&profile_id);
        }
    }

    pub async fn list_encryption_keys(&self) -> Result<Vec<backuppilot_core::EncryptionKey>> {
        let (mut keys, profile_ids) = {
            let db = self.db.lock().await;
            let keys = db.list_encryption_keys()?;
            let ids: Vec<i64> = db
                .list_profiles()?
                .into_iter()
                .map(|p| p.id)
                .collect();
            (keys, ids)
        };

        let mut cred_profiles = Vec::new();
        for id in profile_ids {
            if let Ok(profile) = self.profile_with_credentials(id).await {
                cred_profiles.push(profile);
            }
        }

        backuppilot_core::enrich_encryption_keys_usage(&mut keys, &cred_profiles).await;
        Ok(keys)
    }

    pub async fn create_encryption_key(
        &self,
        input: backuppilot_core::CreateEncryptionKeyInput,
    ) -> Result<backuppilot_core::EncryptionKey> {
        let db = self.db.lock().await;
        db.create_encryption_key(&input)
    }

    pub async fn import_encryption_key(
        &self,
        input: backuppilot_core::ImportEncryptionKeyInput,
    ) -> Result<backuppilot_core::EncryptionKey> {
        let db = self.db.lock().await;
        db.import_encryption_key(&input)
    }

    pub async fn delete_encryption_key(&self, key_id: i64) -> Result<()> {
        let keys = self.list_encryption_keys().await?;
        if let Some(key) = keys.iter().find(|k| k.id == key_id) {
            if backuppilot_core::encryption_key_in_use(key) {
                return Err(backuppilot_core::CoreError::PbsCommand(
                    "encryption key is still used by encrypted backups or assigned to a profile"
                        .into(),
                ));
            }
        }
        let db = self.db.lock().await;
        db.delete_encryption_key(key_id)
    }

    pub async fn mark_encryption_key_exported(&self, key_id: i64) -> Result<()> {
        let db = self.db.lock().await;
        db.mark_encryption_key_exported(key_id)
    }

    pub async fn profiles_using_encryption_key(&self, key_id: i64) -> Result<Vec<String>> {
        let db = self.db.lock().await;
        db.profiles_using_encryption_key(key_id)
    }

    pub async fn pbs_available(&self) -> bool {
        PbsClient::is_available().await
    }

    pub async fn verify_profile_credentials(
        &self,
        input: CredentialVerifyInput,
    ) -> CredentialVerifyResult {
        let repository = if let Some(profile_id) = input.profile_id {
            match backuppilot_core::secrets::hydrate_profile_repository(profile_id, &input.repository) {
                Ok(r) => r,
                Err(e) => return CredentialVerifyResult { ok: false, message: Some(e.to_string()) },
            }
        } else {
            input.repository.clone()
        };
        PbsClient::verify_credentials(
            &repository,
            input.namespace.as_deref(),
            input.server_fingerprint.as_deref(),
        )
        .await
    }

    pub async fn list_snapshots(&self, profile_id: i64) -> Result<Vec<PbsSnapshotInfo>> {
        let profile = self.profile_with_credentials(profile_id).await?;
        PbsRestore::list_snapshots(&profile).await
    }

    pub async fn list_catalog(&self, request: ListCatalogRequest) -> Result<ListCatalogResponse> {
        let profile = self.profile_with_credentials(request.profile_id).await?;
        PbsRestore::list_catalog(&request, &profile).await
    }

    pub async fn restore_archive(&self, request: RestoreArchiveRequest) -> Result<RestoreStartResult> {
        let profile = self.profile_with_credentials(request.profile_id).await?;
        let profile_name = profile.name.clone();
        let target_dir = request.target_dir.clone();
        let item_count = request.patterns.len().max(1);

        if !request.overwrite {
            let key_id = request.encryption_key_id.or(profile.encryption_key_id);
            let conflicts = PbsRestore::find_restore_conflicts(
                &profile,
                &request.snapshot,
                &request.archive_name,
                std::path::Path::new(&request.target_dir),
                &request.patterns,
                key_id,
            )
            .await?;
            if !conflicts.is_empty() {
                return Ok(RestoreStartResult {
                    started: false,
                    ok: false,
                    message: Some(tr("Restore blocked to avoid overwriting existing files.")),
                    conflicts,
                });
            }
        }

        self.record_system_event(
            RESTORE_STARTED_KIND,
            &profile_name,
            RunStatus::Success,
            Some(format!(
                "Restore to {target_dir} started ({item_count} item(s), snapshot {}, archive {})",
                request.snapshot, request.archive_name
            )),
        )
        .await;

        let service = self.clone();
        let profile_name_done = profile_name.clone();
        let target_dir_done = target_dir.clone();
        tokio::spawn(async move {
            let result = PbsRestore::execute_restore(&profile, &request).await;
            match &result {
                Ok(res) if res.ok => {
                    service
                        .record_system_event(
                            RESTORE_FINISHED_KIND,
                            &profile_name_done,
                            RunStatus::Success,
                            Some(format!("Files restored to {target_dir_done}")),
                        )
                        .await;
                    notify_restore_finished(&profile_name_done, &target_dir_done, true, None);
                }
                Ok(res) => {
                    let detail = res.message.clone().unwrap_or_else(|| "Restore failed".into());
                    service
                        .record_system_event(
                            RESTORE_FINISHED_KIND,
                            &profile_name_done,
                            RunStatus::Failed,
                            Some(detail.clone()),
                        )
                        .await;
                    notify_restore_finished(
                        &profile_name_done,
                        &target_dir_done,
                        false,
                        Some(detail.as_str()),
                    );
                }
                Err(err) => {
                    let detail = err.to_string();
                    service
                        .record_system_event(
                            RESTORE_FINISHED_KIND,
                            &profile_name_done,
                            RunStatus::Failed,
                            Some(detail.clone()),
                        )
                        .await;
                    notify_restore_finished(
                        &profile_name_done,
                        &target_dir_done,
                        false,
                        Some(&detail),
                    );
                }
            }
        });

        Ok(RestoreStartResult {
            started: true,
            ok: true,
            message: Some(tr("Restore started in the background.")),
            conflicts: Vec::new(),
        })
    }

    async fn record_system_event(
        &self,
        kind: &str,
        display_name: &str,
        status: RunStatus,
        message: Option<String>,
    ) {
        let db = self.db.lock().await;
        if let Err(err) = db.insert_system_event(kind, display_name, status, message.as_deref()) {
            error!(%err, kind, display_name, "failed to record activity log system event");
        }
    }

    pub async fn process_due_retries(&self) {
        let due = {
            let db = self.db.lock().await;
            db.list_due_pending_retries(Utc::now()).unwrap_or_default()
        };
        for profile_id in due {
            info!(profile_id, "retrying backup after scheduled preflight retry");
            let _ = self.run_scheduled_backup(profile_id).await;
        }
    }

    pub fn get_update_state(&self) -> UpdateState {
        load_update_state()
    }

    pub async fn check_for_updates_now(&self, channel: UpdateChannel) -> UpdateCheckOutcome {
        let outcome = check_for_updates(channel).await;
        let mut state = load_update_state();
        state.last_check_at = Some(Utc::now());
        match &outcome {
            UpdateCheckOutcome::UpToDate => {
                state.available = None;
                state.last_error = None;
            }
            UpdateCheckOutcome::UpdateAvailable { availability: avail } => {
                state.available = Some(avail.clone());
                state.last_error = None;
            }
            UpdateCheckOutcome::Error { message: msg } => {
                state.last_error = Some(msg.clone());
            }
        }
        let _ = save_update_state(&state);
        outcome
    }

    pub fn dismiss_available_update(&self) -> Result<()> {
        dismiss_available_update();
        Ok(())
    }

    /// Toggles global backup pause; returns the new pause state.
    pub fn toggle_pause_all_backups(&self) -> Result<bool> {
        let mut settings = load_app_settings();
        settings.tray.pause_all_backups = !settings.tray.pause_all_backups;
        save_app_settings(&settings)?;
        Ok(settings.tray.pause_all_backups)
    }

    pub fn is_pause_all_backups(&self) -> bool {
        load_app_settings().tray.pause_all_backups
    }

    pub fn complete_onboarding(&self) -> Result<()> {
        let mut settings = load_app_settings();
        settings.onboarding.completed = true;
        save_app_settings(&settings)
    }

    pub fn is_onboarding_completed(&self) -> bool {
        load_app_settings().onboarding.completed
    }
}

fn notify_restore_finished(
    profile_name: &str,
    target_dir: &str,
    success: bool,
    error: Option<&str>,
) {
    let notifications = load_app_settings().notifications;
    if !notifications.should_notify_restore() {
        return;
    }

    let (summary, body) = if success {
        (
            tr_fmt("Restore completed: {name}", &[("name", profile_name)]),
            tr_fmt("Files were restored to {target}.", &[("target", target_dir)]),
        )
    } else {
        let reason = error
            .map(str::to_string)
            .unwrap_or_else(|| tr("unknown error"));
        (
            tr_fmt("Restore failed: {name}", &[("name", profile_name)]),
            reason,
        )
    };
    notify::send_desktop_notification(&summary, &body);
}

fn notification_error_detail(
    status: RunStatus,
    stored: Option<&str>,
    raw_stderr: Option<&str>,
) -> Option<String> {
    if status != RunStatus::Failed {
        return stored.map(str::to_string);
    }
    if let Some(detail) = display_backup_error_message(stored) {
        return Some(detail);
    }
    if raw_stderr.is_some_and(is_pbs_backup_lock_error) {
        return Some(tr("Another backup is already running for this backup target."));
    }
    stored.map(str::to_string)
}

fn backup_finished_notification_text(
    profile_name: &str,
    status: RunStatus,
    error: Option<&str>,
) -> (String, String) {
    match status {
        RunStatus::Success => (
            tr_fmt("Backup completed: {name}", &[("name", profile_name)]),
            tr("The backup finished successfully."),
        ),
        RunStatus::Skipped => {
            let reason = error
                .map(str::to_string)
                .unwrap_or_else(|| tr("Skipped by preflight"));
            (
                tr_fmt("Backup skipped: {name}", &[("name", profile_name)]),
                reason,
            )
        }
        RunStatus::Cancelled => (
            tr_fmt("Backup cancelled: {name}", &[("name", profile_name)]),
            tr("The backup was stopped."),
        ),
        RunStatus::Failed => {
            let summary = tr_fmt("Backup «{name}» failed", &[("name", profile_name)]);
            let body = display_backup_error_message(error)
                .unwrap_or_else(|| tr("unknown error"));
            (summary, body)
        }
        _ => (
            tr_fmt("Backup «{name}»", &[("name", profile_name)]),
            error.unwrap_or("").to_string(),
        ),
    }
}

fn notify_backup_finished(profile_name: &str, status: RunStatus, error: Option<&str>) {
    if status == RunStatus::Cancelled {
        return;
    }
    let notifications = load_app_settings().notifications;
    if !notifications.should_notify_backup(status) {
        return;
    }
    let (summary, body) = backup_finished_notification_text(profile_name, status, error);
    notify::send_desktop_notification(&summary, &body);
}

