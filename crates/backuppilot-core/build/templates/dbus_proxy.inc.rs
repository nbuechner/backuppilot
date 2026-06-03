use zbus::proxy;

#[proxy(
    interface = "@@DBUS_NAME@@",
    default_path = "@@DBUS_PATH@@",
    default_service = "@@DBUS_NAME@@"
)]
pub trait BackupPilotDaemon {
    fn version(&self) -> zbus::Result<String>;

    fn pbs_client_available(&self) -> zbus::Result<bool>;

    fn host_cli_available(&self) -> zbus::Result<bool>;

    fn list_profiles(&self) -> zbus::Result<String>;

    fn get_profile(&self, profile_id: i64) -> zbus::Result<String>;

    fn create_profile(&self, profile_json: String) -> zbus::Result<String>;

    fn update_profile(&self, profile_id: i64, profile_json: String) -> zbus::Result<String>;

    fn set_profile_enabled(&self, profile_id: i64, enabled: bool) -> zbus::Result<String>;

    fn delete_profile(&self, profile_id: i64) -> zbus::Result<()>;

    fn list_statuses(&self) -> zbus::Result<String>;

    fn list_recent_activity(&self, limit: u32) -> zbus::Result<String>;

    fn run_backup(&self, profile_id: i64) -> zbus::Result<String>;

    fn cancel_backup(&self, profile_id: i64) -> zbus::Result<bool>;

    fn cancel_all_running_backups(&self) -> zbus::Result<u32>;

    fn verify_profile_credentials(&self, input_json: String) -> zbus::Result<String>;

    fn list_snapshots(&self, profile_id: i64) -> zbus::Result<String>;

    fn list_catalog(&self, request_json: String) -> zbus::Result<String>;

    fn restore_archive(&self, request_json: String) -> zbus::Result<String>;

    fn run_preflight(&self, profile_id: i64) -> zbus::Result<String>;

    fn list_runs_for_profile(&self, profile_id: i64, limit: u32) -> zbus::Result<String>;

    fn apply_global_defaults_to_profiles(&self) -> zbus::Result<u32>;

    fn list_encryption_keys(&self) -> zbus::Result<String>;

    fn create_encryption_key(&self, input_json: String) -> zbus::Result<String>;

    fn import_encryption_key(&self, input_json: String) -> zbus::Result<String>;

    fn delete_encryption_key(&self, key_id: i64) -> zbus::Result<()>;

    fn mark_encryption_key_exported(&self, key_id: i64) -> zbus::Result<()>;

    fn profiles_using_encryption_key(&self, key_id: i64) -> zbus::Result<String>;

    fn reset_all_data(&self) -> zbus::Result<()>;

    fn get_update_state(&self) -> zbus::Result<String>;

    fn check_for_updates(&self, channel: String) -> zbus::Result<String>;

    fn dismiss_available_update(&self) -> zbus::Result<()>;

    fn toggle_pause_all_backups(&self) -> zbus::Result<bool>;

    fn pause_all_backups(&self) -> zbus::Result<bool>;

    fn complete_onboarding(&self) -> zbus::Result<()>;

    fn is_onboarding_completed(&self) -> zbus::Result<bool>;

    fn list_active_mounts(&self) -> zbus::Result<String>;

    fn mount_snapshot(&self, request_json: String) -> zbus::Result<String>;

    fn unmount_snapshot(&self, mount_id: String) -> zbus::Result<String>;
}
