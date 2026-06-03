use backuppilot_core::profile::{
    ActivityLogEntry, BackupProfile, BackupRun, BackupStartResult, CredentialVerifyInput,
    CredentialVerifyResult, NewProfile, ProfileStatus, RestoreStartResult,
};
use backuppilot_core::app_settings::UpdateChannel;
use backuppilot_core::{
    ActiveMount, CreateEncryptionKeyInput, EncryptionKey, ImportEncryptionKeyInput,
    ListCatalogRequest, ListCatalogResponse, MountSnapshotRequest, MountSnapshotResult,
    PbsSnapshotInfo, PreflightReport, RestoreArchiveRequest, UnmountSnapshotResult,
    UpdateCheckOutcome,
};

include!(concat!(env!("OUT_DIR"), "/dbus_proxy.rs"));

pub async fn connect() -> zbus::Result<BackupPilotDaemonProxy<'static>> {
    // Daemon-Start nur in Application::startup (ensure_daemon_running), nicht pro Request —
    // sonst blockieren parallele connect()-Aufrufe den Thread-Pool beim Fensteraufbau.
    let connection = zbus::Connection::session().await?;
    BackupPilotDaemonProxy::new(&connection).await
}

fn parse_json<T: serde::de::DeserializeOwned>(json: &str) -> zbus::Result<T> {
    serde_json::from_str(json).map_err(|e| zbus::Error::Failure(e.to_string()))
}

pub async fn host_cli_available(proxy: &BackupPilotDaemonProxy<'_>) -> zbus::Result<bool> {
    proxy.host_cli_available().await
}

pub async fn list_profiles(proxy: &BackupPilotDaemonProxy<'_>) -> zbus::Result<Vec<BackupProfile>> {
    parse_json(&proxy.list_profiles().await?)
}

pub async fn get_profile(
    proxy: &BackupPilotDaemonProxy<'_>,
    profile_id: i64,
) -> zbus::Result<BackupProfile> {
    parse_json(&proxy.get_profile(profile_id).await?)
}

pub async fn create_profile(
    proxy: &BackupPilotDaemonProxy<'_>,
    profile: &NewProfile,
) -> zbus::Result<BackupProfile> {
    let json = serde_json::to_string(profile).map_err(|e| zbus::Error::Failure(e.to_string()))?;
    parse_json(&proxy.create_profile(json).await?)
}

pub async fn update_profile(
    proxy: &BackupPilotDaemonProxy<'_>,
    profile_id: i64,
    profile: &NewProfile,
) -> zbus::Result<BackupProfile> {
    let json = serde_json::to_string(profile).map_err(|e| zbus::Error::Failure(e.to_string()))?;
    parse_json(&proxy.update_profile(profile_id, json).await?)
}

pub async fn delete_profile(
    proxy: &BackupPilotDaemonProxy<'_>,
    profile_id: i64,
) -> zbus::Result<()> {
    proxy.delete_profile(profile_id).await
}

pub async fn list_statuses(proxy: &BackupPilotDaemonProxy<'_>) -> zbus::Result<Vec<ProfileStatus>> {
    parse_json(&proxy.list_statuses().await?)
}

pub async fn list_recent_activity(
    proxy: &BackupPilotDaemonProxy<'_>,
    limit: u32,
) -> zbus::Result<Vec<ActivityLogEntry>> {
    parse_json(&proxy.list_recent_activity(limit).await?)
}

pub async fn run_backup(
    proxy: &BackupPilotDaemonProxy<'_>,
    profile_id: i64,
) -> zbus::Result<BackupStartResult> {
    parse_json(&proxy.run_backup(profile_id).await?)
}

pub async fn cancel_backup(
    proxy: &BackupPilotDaemonProxy<'_>,
    profile_id: i64,
) -> zbus::Result<bool> {
    proxy.cancel_backup(profile_id).await
}

pub async fn cancel_all_running_backups(
    proxy: &BackupPilotDaemonProxy<'_>,
) -> zbus::Result<u32> {
    proxy.cancel_all_running_backups().await
}

pub async fn verify_profile_credentials(
    proxy: &BackupPilotDaemonProxy<'_>,
    input: &CredentialVerifyInput,
) -> zbus::Result<CredentialVerifyResult> {
    let json = serde_json::to_string(input).map_err(|e| zbus::Error::Failure(e.to_string()))?;
    parse_json(&proxy.verify_profile_credentials(json).await?)
}

pub async fn list_snapshots(
    proxy: &BackupPilotDaemonProxy<'_>,
    profile_id: i64,
) -> zbus::Result<Vec<PbsSnapshotInfo>> {
    parse_json(&proxy.list_snapshots(profile_id).await?)
}

pub async fn list_catalog(
    proxy: &BackupPilotDaemonProxy<'_>,
    request: &ListCatalogRequest,
) -> zbus::Result<ListCatalogResponse> {
    let json = serde_json::to_string(request).map_err(|e| zbus::Error::Failure(e.to_string()))?;
    parse_json(&proxy.list_catalog(json).await?)
}

pub async fn restore_archive(
    proxy: &BackupPilotDaemonProxy<'_>,
    request: &RestoreArchiveRequest,
) -> zbus::Result<RestoreStartResult> {
    let json = serde_json::to_string(request).map_err(|e| zbus::Error::Failure(e.to_string()))?;
    parse_json(&proxy.restore_archive(json).await?)
}

pub async fn run_preflight(
    proxy: &BackupPilotDaemonProxy<'_>,
    profile_id: i64,
) -> zbus::Result<PreflightReport> {
    parse_json(&proxy.run_preflight(profile_id).await?)
}

pub async fn list_runs_for_profile(
    proxy: &BackupPilotDaemonProxy<'_>,
    profile_id: i64,
    limit: u32,
) -> zbus::Result<Vec<BackupRun>> {
    parse_json(&proxy.list_runs_for_profile(profile_id, limit).await?)
}

pub async fn apply_global_defaults_to_profiles(
    proxy: &BackupPilotDaemonProxy<'_>,
) -> zbus::Result<u32> {
    proxy.apply_global_defaults_to_profiles().await
}

pub async fn list_encryption_keys(
    proxy: &BackupPilotDaemonProxy<'_>,
) -> zbus::Result<Vec<EncryptionKey>> {
    parse_json(&proxy.list_encryption_keys().await?)
}

pub async fn create_encryption_key(
    proxy: &BackupPilotDaemonProxy<'_>,
    input: &CreateEncryptionKeyInput,
) -> zbus::Result<EncryptionKey> {
    let json = serde_json::to_string(input).map_err(|e| zbus::Error::Failure(e.to_string()))?;
    parse_json(&proxy.create_encryption_key(json).await?)
}

pub async fn import_encryption_key(
    proxy: &BackupPilotDaemonProxy<'_>,
    input: &ImportEncryptionKeyInput,
) -> zbus::Result<EncryptionKey> {
    let json = serde_json::to_string(input).map_err(|e| zbus::Error::Failure(e.to_string()))?;
    parse_json(&proxy.import_encryption_key(json).await?)
}

pub async fn delete_encryption_key(
    proxy: &BackupPilotDaemonProxy<'_>,
    key_id: i64,
) -> zbus::Result<()> {
    proxy.delete_encryption_key(key_id).await
}

pub async fn mark_encryption_key_exported(
    proxy: &BackupPilotDaemonProxy<'_>,
    key_id: i64,
) -> zbus::Result<()> {
    proxy.mark_encryption_key_exported(key_id).await
}

pub async fn reset_all_data(proxy: &BackupPilotDaemonProxy<'_>) -> zbus::Result<()> {
    proxy.reset_all_data().await
}

pub async fn check_for_updates(
    proxy: &BackupPilotDaemonProxy<'_>,
    channel: UpdateChannel,
) -> zbus::Result<UpdateCheckOutcome> {
    let channel = match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Beta => "beta",
    };
    parse_json(&proxy.check_for_updates(channel.to_string()).await?)
}

pub async fn toggle_pause_all_backups(
    proxy: &BackupPilotDaemonProxy<'_>,
) -> zbus::Result<bool> {
    proxy.toggle_pause_all_backups().await
}

pub async fn pause_all_backups(proxy: &BackupPilotDaemonProxy<'_>) -> zbus::Result<bool> {
    proxy.pause_all_backups().await
}

pub async fn complete_onboarding(proxy: &BackupPilotDaemonProxy<'_>) -> zbus::Result<()> {
    proxy.complete_onboarding().await
}

pub async fn is_onboarding_completed(proxy: &BackupPilotDaemonProxy<'_>) -> zbus::Result<bool> {
    proxy.is_onboarding_completed().await
}

pub async fn list_active_mounts(
    proxy: &BackupPilotDaemonProxy<'_>,
) -> zbus::Result<Vec<ActiveMount>> {
    parse_json(&proxy.list_active_mounts().await?)
}

pub async fn mount_snapshot(
    proxy: &BackupPilotDaemonProxy<'_>,
    request: &MountSnapshotRequest,
) -> zbus::Result<MountSnapshotResult> {
    let json = serde_json::to_string(request).map_err(|e| zbus::Error::Failure(e.to_string()))?;
    parse_json(&proxy.mount_snapshot(json).await?)
}

pub async fn unmount_snapshot(
    proxy: &BackupPilotDaemonProxy<'_>,
    mount_id: &str,
) -> zbus::Result<UnmountSnapshotResult> {
    parse_json(&proxy.unmount_snapshot(mount_id.to_string()).await?)
}
