//! GUI ↔ daemon IPC client. Drop-in replacement for the former zbus D-Bus client.
//! All public functions have identical signatures except `zbus::Result` →
//! `backuppilot_ipc::Result`.

use backuppilot_core::app_settings::UpdateChannel;
use backuppilot_core::profile::{
    ActivityLogEntry, BackupProfile, BackupRun, BackupStartResult, CredentialVerifyInput,
    CredentialVerifyResult, NewProfile, ProfileStatus, RestoreStartResult,
};
use backuppilot_core::{
    ActiveMount, CreateEncryptionKeyInput, EncryptionKey, ImportEncryptionKeyInput,
    ListCatalogRequest, ListCatalogResponse, MountSnapshotRequest, MountSnapshotResult,
    PbsSnapshotInfo, PreflightReport, RestoreArchiveRequest, UnmountSnapshotResult,
    UpdateCheckOutcome,
};
use backuppilot_ipc::{IpcClient, IpcError, Result as IpcResult};
use serde_json::json;

// ── connection handle (opaque, kept for API compat) ───────────────────────────

pub struct DaemonClient;

pub async fn connect() -> IpcResult<DaemonClient> {
    // Verify daemon is reachable with a version call.
    IpcClient::call("version", json!(null)).await?;
    Ok(DaemonClient)
}

// ── helpers ───────────────────────────────────────────────────────────────────

async fn call(method: &str, params: serde_json::Value) -> IpcResult<String> {
    IpcClient::call(method, params).await
}

fn parse<T: serde::de::DeserializeOwned>(json: &str) -> IpcResult<T> {
    serde_json::from_str(json).map_err(|e| IpcError::failure(e.to_string()))
}

fn parse_bool(s: &str) -> IpcResult<bool> {
    s.trim().parse::<bool>().map_err(|e| IpcError::failure(e.to_string()))
}

fn parse_u32(s: &str) -> IpcResult<u32> {
    s.trim().parse::<u32>().map_err(|e| IpcError::failure(e.to_string()))
}

fn to_json<T: serde::Serialize>(v: &T) -> IpcResult<String> {
    serde_json::to_string(v).map_err(|e| IpcError::failure(e.to_string()))
}

// ── public API (same signatures as before, DaemonClient replaces proxy arg) ───

pub async fn host_cli_available(_: &DaemonClient) -> IpcResult<bool> {
    parse_bool(&call("host_cli_available", json!(null)).await?)
}

pub async fn pbs_client_available(_: &DaemonClient) -> IpcResult<bool> {
    parse_bool(&call("pbs_client_available", json!(null)).await?)
}

pub async fn list_profiles(_: &DaemonClient) -> IpcResult<Vec<BackupProfile>> {
    parse(&call("list_profiles", json!(null)).await?)
}

pub async fn get_profile(_: &DaemonClient, profile_id: i64) -> IpcResult<BackupProfile> {
    parse(&call("get_profile", json!({"profile_id": profile_id})).await?)
}

pub async fn create_profile(_: &DaemonClient, profile: &NewProfile) -> IpcResult<BackupProfile> {
    let pj = to_json(profile)?;
    parse(&call("create_profile", json!({"profile_json": pj})).await?)
}

pub async fn update_profile(
    _: &DaemonClient,
    profile_id: i64,
    profile: &NewProfile,
) -> IpcResult<BackupProfile> {
    let pj = to_json(profile)?;
    parse(&call("update_profile", json!({"profile_id": profile_id, "profile_json": pj})).await?)
}

pub async fn set_profile_enabled(
    _: &DaemonClient,
    profile_id: i64,
    enabled: bool,
) -> IpcResult<BackupProfile> {
    parse(&call("set_profile_enabled", json!({"profile_id": profile_id, "enabled": enabled})).await?)
}

pub async fn delete_profile(_: &DaemonClient, profile_id: i64) -> IpcResult<()> {
    call("delete_profile", json!({"profile_id": profile_id})).await?;
    Ok(())
}

pub async fn list_statuses(_: &DaemonClient) -> IpcResult<Vec<ProfileStatus>> {
    parse(&call("list_statuses", json!(null)).await?)
}

pub async fn list_recent_activity(_: &DaemonClient, limit: u32) -> IpcResult<Vec<ActivityLogEntry>> {
    parse(&call("list_recent_activity", json!({"limit": limit})).await?)
}

pub async fn run_backup(_: &DaemonClient, profile_id: i64) -> IpcResult<BackupStartResult> {
    parse(&call("run_backup", json!({"profile_id": profile_id})).await?)
}

pub async fn cancel_backup(_: &DaemonClient, profile_id: i64) -> IpcResult<bool> {
    parse_bool(&call("cancel_backup", json!({"profile_id": profile_id})).await?)
}

pub async fn cancel_all_running_backups(_: &DaemonClient) -> IpcResult<u32> {
    parse_u32(&call("cancel_all_running_backups", json!(null)).await?)
}

pub async fn verify_profile_credentials(
    _: &DaemonClient,
    input: &CredentialVerifyInput,
) -> IpcResult<CredentialVerifyResult> {
    let ij = to_json(input)?;
    parse(&call("verify_profile_credentials", json!({"input_json": ij})).await?)
}

pub async fn list_snapshots(_: &DaemonClient, profile_id: i64) -> IpcResult<Vec<PbsSnapshotInfo>> {
    parse(&call("list_snapshots", json!({"profile_id": profile_id})).await?)
}

pub async fn delete_snapshot(
    _: &DaemonClient,
    profile_id: i64,
    snapshot_path: &str,
) -> IpcResult<()> {
    call("delete_snapshot", json!({"profile_id": profile_id, "snapshot_path": snapshot_path})).await?;
    Ok(())
}

pub async fn check_snapshot_permissions(
    _: &DaemonClient,
    profile_id: i64,
) -> IpcResult<backuppilot_core::DatastorePermissions> {
    parse(&call("check_snapshot_permissions", json!({"profile_id": profile_id})).await?)
}

pub async fn list_catalog(
    _: &DaemonClient,
    request: &ListCatalogRequest,
) -> IpcResult<ListCatalogResponse> {
    let rj = to_json(request)?;
    parse(&call("list_catalog", json!({"request_json": rj})).await?)
}

pub async fn restore_archive(
    _: &DaemonClient,
    request: &RestoreArchiveRequest,
) -> IpcResult<RestoreStartResult> {
    let rj = to_json(request)?;
    parse(&call("restore_archive", json!({"request_json": rj})).await?)
}

pub async fn run_preflight(_: &DaemonClient, profile_id: i64) -> IpcResult<PreflightReport> {
    parse(&call("run_preflight", json!({"profile_id": profile_id})).await?)
}

pub async fn list_runs_for_profile(
    _: &DaemonClient,
    profile_id: i64,
    limit: u32,
) -> IpcResult<Vec<BackupRun>> {
    parse(&call("list_runs_for_profile", json!({"profile_id": profile_id, "limit": limit})).await?)
}

pub async fn apply_global_defaults_to_profiles(_: &DaemonClient) -> IpcResult<u32> {
    parse_u32(&call("apply_global_defaults_to_profiles", json!(null)).await?)
}

pub async fn list_encryption_keys(_: &DaemonClient) -> IpcResult<Vec<EncryptionKey>> {
    parse(&call("list_encryption_keys", json!(null)).await?)
}

pub async fn create_encryption_key(
    _: &DaemonClient,
    input: &CreateEncryptionKeyInput,
) -> IpcResult<EncryptionKey> {
    let ij = to_json(input)?;
    parse(&call("create_encryption_key", json!({"input_json": ij})).await?)
}

pub async fn import_encryption_key(
    _: &DaemonClient,
    input: &ImportEncryptionKeyInput,
) -> IpcResult<EncryptionKey> {
    let ij = to_json(input)?;
    parse(&call("import_encryption_key", json!({"input_json": ij})).await?)
}

pub async fn delete_encryption_key(_: &DaemonClient, key_id: i64) -> IpcResult<()> {
    call("delete_encryption_key", json!({"key_id": key_id})).await?;
    Ok(())
}

pub async fn mark_encryption_key_exported(_: &DaemonClient, key_id: i64) -> IpcResult<()> {
    call("mark_encryption_key_exported", json!({"key_id": key_id})).await?;
    Ok(())
}

pub async fn profiles_using_encryption_key(
    _: &DaemonClient,
    key_id: i64,
) -> IpcResult<Vec<String>> {
    parse(&call("profiles_using_encryption_key", json!({"key_id": key_id})).await?)
}

pub async fn reset_all_data(_: &DaemonClient) -> IpcResult<()> {
    call("reset_all_data", json!(null)).await?;
    Ok(())
}

pub async fn check_for_updates(
    _: &DaemonClient,
    channel: UpdateChannel,
) -> IpcResult<UpdateCheckOutcome> {
    let ch = match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Beta => "beta",
    };
    parse(&call("check_for_updates", json!({"channel": ch})).await?)
}

pub async fn get_update_state(_: &DaemonClient) -> IpcResult<backuppilot_core::UpdateState> {
    parse(&call("get_update_state", json!(null)).await?)
}

pub async fn dismiss_available_update(_: &DaemonClient) -> IpcResult<()> {
    call("dismiss_available_update", json!(null)).await?;
    Ok(())
}

pub async fn toggle_pause_all_backups(_: &DaemonClient) -> IpcResult<bool> {
    parse_bool(&call("toggle_pause_all_backups", json!(null)).await?)
}

pub async fn pause_all_backups(_: &DaemonClient) -> IpcResult<bool> {
    parse_bool(&call("pause_all_backups", json!(null)).await?)
}

pub async fn complete_onboarding(_: &DaemonClient) -> IpcResult<()> {
    call("complete_onboarding", json!(null)).await?;
    Ok(())
}

pub async fn is_onboarding_completed(_: &DaemonClient) -> IpcResult<bool> {
    parse_bool(&call("is_onboarding_completed", json!(null)).await?)
}

pub async fn list_active_mounts(_: &DaemonClient) -> IpcResult<Vec<ActiveMount>> {
    parse(&call("list_active_mounts", json!(null)).await?)
}

pub async fn mount_snapshot(
    _: &DaemonClient,
    request: &MountSnapshotRequest,
) -> IpcResult<MountSnapshotResult> {
    let rj = to_json(request)?;
    parse(&call("mount_snapshot", json!({"request_json": rj})).await?)
}

pub async fn unmount_snapshot(_: &DaemonClient, mount_id: &str) -> IpcResult<UnmountSnapshotResult> {
    parse(&call("unmount_snapshot", json!({"mount_id": mount_id})).await?)
}
