use backuppilot_core::profile::{CredentialVerifyInput, NewProfile};
use crate::service::DaemonService;

pub async fn dispatch(
    service: &DaemonService,
    method: &str,
    params: serde_json::Value,
) -> Result<String, String> {
    let p = &params;

    fn get_i64(p: &serde_json::Value, key: &str) -> Result<i64, String> {
        p.get(key)
            .and_then(|v| v.as_i64())
            .ok_or_else(|| format!("missing param '{key}'"))
    }
    fn get_u32(p: &serde_json::Value, key: &str) -> Result<u32, String> {
        p.get(key)
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .ok_or_else(|| format!("missing param '{key}'"))
    }
    fn get_str(p: &serde_json::Value, key: &str) -> Result<String, String> {
        p.get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("missing param '{key}'"))
    }
    fn get_bool(p: &serde_json::Value, key: &str) -> Result<bool, String> {
        p.get(key)
            .and_then(|v| v.as_bool())
            .ok_or_else(|| format!("missing param '{key}'"))
    }
    fn ser<T: serde::Serialize>(v: &T) -> Result<String, String> {
        serde_json::to_string(v).map_err(|e| e.to_string())
    }

    match method {
        "version" => Ok(env!("CARGO_PKG_VERSION").to_string()),

        "pbs_client_available" => Ok(service.pbs_available().await.to_string()),

        "host_cli_available" => Ok(backuppilot_core::cli_available().await.to_string()),

        "list_profiles" => {
            let v = service.list_profiles().await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "get_profile" => {
            let id = get_i64(p, "profile_id")?;
            let v = service.get_profile(id).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "create_profile" => {
            let json = get_str(p, "profile_json")?;
            let new: NewProfile = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let v = service.create_profile(new).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "update_profile" => {
            let id = get_i64(p, "profile_id")?;
            let json = get_str(p, "profile_json")?;
            let update: NewProfile = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let v = service.update_profile(id, update).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "set_profile_enabled" => {
            let id = get_i64(p, "profile_id")?;
            let enabled = get_bool(p, "enabled")?;
            let v = service.set_profile_enabled(id, enabled).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "delete_profile" => {
            let id = get_i64(p, "profile_id")?;
            service.delete_profile(id).await.map_err(|e| e.to_string())?;
            Ok("null".into())
        }

        "list_statuses" => {
            let v = service.list_statuses().await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "list_recent_activity" => {
            let limit = get_u32(p, "limit")?;
            let v = service.list_recent_activity(limit).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "run_backup" => {
            let id = get_i64(p, "profile_id")?;
            let v = service.run_backup(id).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "cancel_backup" => {
            let id = get_i64(p, "profile_id")?;
            let v = service.cancel_backup(id).await.map_err(|e| e.to_string())?;
            Ok(v.to_string())
        }

        "cancel_all_running_backups" => {
            let v = service.cancel_all_running_backups().await.map_err(|e| e.to_string())?;
            Ok(v.to_string())
        }

        "verify_profile_credentials" => {
            let json = get_str(p, "input_json")?;
            let input: CredentialVerifyInput = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let v = service.verify_profile_credentials(input).await;
            ser(&v)
        }

        "list_snapshots" => {
            let id = get_i64(p, "profile_id")?;
            let v = service.list_snapshots(id).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "list_catalog" => {
            let json = get_str(p, "request_json")?;
            let req: backuppilot_core::ListCatalogRequest =
                serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let v = service.list_catalog(req).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "restore_archive" => {
            let json = get_str(p, "request_json")?;
            let req: backuppilot_core::RestoreArchiveRequest =
                serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let v = service.restore_archive(req).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "run_preflight" => {
            let id = get_i64(p, "profile_id")?;
            let v = service.run_preflight(id).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "list_runs_for_profile" => {
            let id = get_i64(p, "profile_id")?;
            let limit = get_u32(p, "limit")?;
            let v = service.list_runs_for_profile(id, limit).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "apply_global_defaults_to_profiles" => {
            let v = service.apply_global_defaults_to_profiles().await.map_err(|e| e.to_string())?;
            Ok(v.to_string())
        }

        "list_encryption_keys" => {
            let v = service.list_encryption_keys().await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "create_encryption_key" => {
            let json = get_str(p, "input_json")?;
            let input: backuppilot_core::CreateEncryptionKeyInput =
                serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let v = service.create_encryption_key(input).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "import_encryption_key" => {
            let json = get_str(p, "input_json")?;
            let input: backuppilot_core::ImportEncryptionKeyInput =
                serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let v = service.import_encryption_key(input).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "delete_encryption_key" => {
            let id = get_i64(p, "key_id")?;
            service.delete_encryption_key(id).await.map_err(|e| e.to_string())?;
            Ok("null".into())
        }

        "mark_encryption_key_exported" => {
            let id = get_i64(p, "key_id")?;
            service.mark_encryption_key_exported(id).await.map_err(|e| e.to_string())?;
            Ok("null".into())
        }

        "profiles_using_encryption_key" => {
            let id = get_i64(p, "key_id")?;
            let v = service.profiles_using_encryption_key(id).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "reset_all_data" => {
            service.reset_all_data().await.map_err(|e| e.to_string())?;
            Ok("null".into())
        }

        "get_update_state" => {
            let v = service.get_update_state();
            ser(&v)
        }

        "check_for_updates" => {
            use backuppilot_core::app_settings::UpdateChannel;
            let channel_str = get_str(p, "channel")?;
            let channel = match channel_str.as_str() {
                "beta" => UpdateChannel::Beta,
                _ => UpdateChannel::Stable,
            };
            let v = service.check_for_updates_now(channel).await;
            ser(&v)
        }

        "dismiss_available_update" => {
            service.dismiss_available_update().map_err(|e| e.to_string())?;
            Ok("null".into())
        }

        "toggle_pause_all_backups" => {
            let v = service.toggle_pause_all_backups().map_err(|e| e.to_string())?;
            Ok(v.to_string())
        }

        "pause_all_backups" => Ok(service.is_pause_all_backups().to_string()),

        "complete_onboarding" => {
            service.complete_onboarding().map_err(|e| e.to_string())?;
            Ok("null".into())
        }

        "is_onboarding_completed" => Ok(service.is_onboarding_completed().to_string()),

        "list_active_mounts" => {
            let v = service.list_active_mounts().await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "mount_snapshot" => {
            let json = get_str(p, "request_json")?;
            let req: backuppilot_core::MountSnapshotRequest =
                serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let v = service.mount_snapshot(req).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        "unmount_snapshot" => {
            let mount_id = get_str(p, "mount_id")?;
            let v = service.unmount_snapshot(&mount_id).await.map_err(|e| e.to_string())?;
            ser(&v)
        }

        other => Err(format!("unknown method: {other}")),
    }
}
