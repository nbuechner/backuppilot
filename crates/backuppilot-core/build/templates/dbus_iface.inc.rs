use backuppilot_core::profile::{CredentialVerifyInput, NewProfile};
use zbus::interface;

use crate::service::DaemonService;

pub struct BackupPilotDaemon {
    pub service: DaemonService,
}

fn json_err(err: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(err.to_string())
}

#[interface(name = "@@DBUS_NAME@@")]
impl BackupPilotDaemon {
    async fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    async fn pbs_client_available(&self) -> bool {
        self.service.pbs_available().await
    }

    async fn host_cli_available(&self) -> bool {
        backuppilot_core::cli_available().await
    }

    async fn list_profiles(&self) -> zbus::fdo::Result<String> {
        let profiles = self
            .service
            .list_profiles()
            .await
            .map_err(json_err)?;
        serde_json::to_string(&profiles).map_err(json_err)
    }

    async fn get_profile(&self, profile_id: i64) -> zbus::fdo::Result<String> {
        let profile = self
            .service
            .get_profile(profile_id)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&profile).map_err(json_err)
    }

    async fn create_profile(&self, profile_json: String) -> zbus::fdo::Result<String> {
        let new: NewProfile = serde_json::from_str(&profile_json).map_err(json_err)?;
        let profile = self
            .service
            .create_profile(new)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&profile).map_err(json_err)
    }

    async fn update_profile(
        &self,
        profile_id: i64,
        profile_json: String,
    ) -> zbus::fdo::Result<String> {
        let update: NewProfile = serde_json::from_str(&profile_json).map_err(json_err)?;
        let profile = self
            .service
            .update_profile(profile_id, update)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&profile).map_err(json_err)
    }

    async fn set_profile_enabled(
        &self,
        profile_id: i64,
        enabled: bool,
    ) -> zbus::fdo::Result<String> {
        let profile = self
            .service
            .set_profile_enabled(profile_id, enabled)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&profile).map_err(json_err)
    }

    async fn delete_profile(&self, profile_id: i64) -> zbus::fdo::Result<()> {
        self.service
            .delete_profile(profile_id)
            .await
            .map_err(json_err)
    }

    async fn list_statuses(&self) -> zbus::fdo::Result<String> {
        let statuses = self
            .service
            .list_statuses()
            .await
            .map_err(json_err)?;
        serde_json::to_string(&statuses).map_err(json_err)
    }

    async fn list_recent_activity(&self, limit: u32) -> zbus::fdo::Result<String> {
        let entries = self
            .service
            .list_recent_activity(limit)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&entries).map_err(json_err)
    }

    async fn run_backup(&self, profile_id: i64) -> zbus::fdo::Result<String> {
        let result = self
            .service
            .run_backup(profile_id)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&result).map_err(json_err)
    }

    async fn cancel_backup(&self, profile_id: i64) -> zbus::fdo::Result<bool> {
        self.service
            .cancel_backup(profile_id)
            .await
            .map_err(json_err)
    }

    async fn cancel_all_running_backups(&self) -> zbus::fdo::Result<u32> {
        self.service
            .cancel_all_running_backups()
            .await
            .map_err(json_err)
    }

    async fn verify_profile_credentials(&self, input_json: String) -> zbus::fdo::Result<String> {
        let input: CredentialVerifyInput = serde_json::from_str(&input_json).map_err(json_err)?;
        let result = self.service.verify_profile_credentials(input).await;
        serde_json::to_string(&result).map_err(json_err)
    }

    async fn list_snapshots(&self, profile_id: i64) -> zbus::fdo::Result<String> {
        let snapshots = self
            .service
            .list_snapshots(profile_id)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&snapshots).map_err(json_err)
    }

    async fn delete_snapshot(
        &self,
        profile_id: i64,
        snapshot_path: String,
    ) -> zbus::fdo::Result<()> {
        self.service
            .delete_snapshot(profile_id, snapshot_path)
            .await
            .map_err(json_err)
    }

    async fn check_snapshot_permissions(
        &self,
        profile_id: i64,
    ) -> zbus::fdo::Result<String> {
        let perms = self.service
            .check_snapshot_permissions(profile_id)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&perms).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn list_catalog(&self, request_json: String) -> zbus::fdo::Result<String> {
        let request: backuppilot_core::ListCatalogRequest =
            serde_json::from_str(&request_json).map_err(json_err)?;
        let result = self
            .service
            .list_catalog(request)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&result).map_err(json_err)
    }

    async fn restore_archive(&self, request_json: String) -> zbus::fdo::Result<String> {
        let request: backuppilot_core::RestoreArchiveRequest =
            serde_json::from_str(&request_json).map_err(json_err)?;
        let result = self
            .service
            .restore_archive(request)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&result).map_err(json_err)
    }

    async fn run_preflight(&self, profile_id: i64) -> zbus::fdo::Result<String> {
        let report = self
            .service
            .run_preflight(profile_id)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&report).map_err(json_err)
    }

    async fn list_runs_for_profile(
        &self,
        profile_id: i64,
        limit: u32,
    ) -> zbus::fdo::Result<String> {
        let runs = self
            .service
            .list_runs_for_profile(profile_id, limit)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&runs).map_err(json_err)
    }

    async fn apply_global_defaults_to_profiles(&self) -> zbus::fdo::Result<u32> {
        self.service
            .apply_global_defaults_to_profiles()
            .await
            .map_err(json_err)
    }

    async fn list_encryption_keys(&self) -> zbus::fdo::Result<String> {
        let keys = self
            .service
            .list_encryption_keys()
            .await
            .map_err(json_err)?;
        serde_json::to_string(&keys).map_err(json_err)
    }

    async fn create_encryption_key(&self, input_json: String) -> zbus::fdo::Result<String> {
        let input: backuppilot_core::CreateEncryptionKeyInput =
            serde_json::from_str(&input_json).map_err(json_err)?;
        let key = self
            .service
            .create_encryption_key(input)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&key).map_err(json_err)
    }

    async fn import_encryption_key(&self, input_json: String) -> zbus::fdo::Result<String> {
        let input: backuppilot_core::ImportEncryptionKeyInput =
            serde_json::from_str(&input_json).map_err(json_err)?;
        let key = self
            .service
            .import_encryption_key(input)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&key).map_err(json_err)
    }

    async fn delete_encryption_key(&self, key_id: i64) -> zbus::fdo::Result<()> {
        self.service
            .delete_encryption_key(key_id)
            .await
            .map_err(json_err)
    }

    async fn mark_encryption_key_exported(&self, key_id: i64) -> zbus::fdo::Result<()> {
        self.service
            .mark_encryption_key_exported(key_id)
            .await
            .map_err(json_err)
    }

    async fn profiles_using_encryption_key(&self, key_id: i64) -> zbus::fdo::Result<String> {
        let names = self
            .service
            .profiles_using_encryption_key(key_id)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&names).map_err(json_err)
    }

    async fn reset_all_data(&self) -> zbus::fdo::Result<()> {
        self.service.reset_all_data().await.map_err(json_err)
    }

    async fn get_update_state(&self) -> zbus::fdo::Result<String> {
        let state = self.service.get_update_state();
        serde_json::to_string(&state).map_err(json_err)
    }

    async fn check_for_updates(&self, channel: String) -> zbus::fdo::Result<String> {
        use backuppilot_core::app_settings::UpdateChannel;
        let channel = match channel.as_str() {
            "beta" => UpdateChannel::Beta,
            _ => UpdateChannel::Stable,
        };
        let outcome = self.service.check_for_updates_now(channel).await;
        serde_json::to_string(&outcome).map_err(json_err)
    }

    async fn dismiss_available_update(&self) -> zbus::fdo::Result<()> {
        self.service.dismiss_available_update().map_err(json_err)
    }

    async fn toggle_pause_all_backups(&self) -> zbus::fdo::Result<bool> {
        self.service.toggle_pause_all_backups().map_err(json_err)
    }

    async fn pause_all_backups(&self) -> zbus::fdo::Result<bool> {
        Ok(self.service.is_pause_all_backups())
    }

    async fn complete_onboarding(&self) -> zbus::fdo::Result<()> {
        self.service.complete_onboarding().map_err(json_err)
    }

    async fn is_onboarding_completed(&self) -> zbus::fdo::Result<bool> {
        Ok(self.service.is_onboarding_completed())
    }

    async fn list_active_mounts(&self) -> zbus::fdo::Result<String> {
        let mounts = self
            .service
            .list_active_mounts()
            .await
            .map_err(json_err)?;
        serde_json::to_string(&mounts).map_err(json_err)
    }

    async fn mount_snapshot(&self, request_json: String) -> zbus::fdo::Result<String> {
        let request: backuppilot_core::MountSnapshotRequest =
            serde_json::from_str(&request_json).map_err(json_err)?;
        let result = self
            .service
            .mount_snapshot(request)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&result).map_err(json_err)
    }

    async fn unmount_snapshot(&self, mount_id: String) -> zbus::fdo::Result<String> {
        let result = self
            .service
            .unmount_snapshot(&mount_id)
            .await
            .map_err(json_err)?;
        serde_json::to_string(&result).map_err(json_err)
    }
}

pub const DBUS_WELL_KNOWN_NAME: &str = "@@DBUS_NAME@@";
pub const DBUS_OBJECT_PATH: &str = "@@DBUS_PATH@@";
