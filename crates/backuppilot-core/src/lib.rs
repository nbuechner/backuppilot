pub mod activity_events;
pub mod backup_job;
pub mod app_settings;
pub mod ids;
pub use activity_events::{
    RESTORE_FINISHED_KIND, RESTORE_STARTED_KIND, SNAPSHOT_MOUNT_KIND, SNAPSHOT_UNMOUNT_KIND,
};
pub use app_settings::{
    activity_log_cutoff, load_app_settings, save_app_settings, validate_health_check,
    validate_log_retention, AppearanceSettings, AppSettings, ColorScheme, LogRetentionSettings,
    NotificationSettings, TraySettings, UiLanguage, UpdateChannel, UpdateSettings,
};
pub mod db;
pub mod encryption;
pub mod error;
pub mod health;
pub mod health_notify;
pub mod network;
pub mod retry;
pub mod restore_paths;
pub mod windows_setup;
pub mod notify;
pub mod preflight;
pub mod secrets;
pub mod paths;
pub mod pbs;
pub mod pbs_api;
pub mod mount_manager;
pub mod mount_registry;
pub mod pbs_mount;
pub mod pbs_install;
pub mod pbs_install_result;
pub mod pbs_repository;
pub mod profile;
pub mod profile_yaml;
pub mod reset;
pub mod restore;
pub mod schedule;
pub mod updates;

pub use db::{ActivityPruneResult, Database};
pub use error::{CoreError, Result};
pub use ids::{APP_ID, AUTOSTART_DESKTOP, DBUS_NAME, DBUS_PATH, DBUS_SERVICE_FILE, ICON_NAME};
pub use backup_job::{
    execute_profile_backup, list_running_backups, profile_backup_in_progress, resolve_profile_id,
    BackupJobOutcome, RunningBackupInfo,
};
pub use paths::{
    apply_config_owner, cli_available, cli_available_sync, config_dir, data_dir, database_path,
    find_executable, is_flatpak_runtime, legacy_autostart_desktop_path, legacy_systemd_unit_path,
    pbs_client_install_result_path, pbs_client_path, pbs_client_version, resolve_cli_binary,
    user_autostart_dir, user_systemd_user_dir, xdg_config_home,
};
pub use pbs_install_result::{
    consume_install_result_file, install_result_path_for_shell, PbsInstallResult,
    PbsInstallResultStatus, PBS_CLIENT_INSTALL_DISPLAY, PBS_CLIENT_INSTALL_KIND,
};
pub use pbs_install::{
    fuse3_install_script, InstallFuse3Result,
    PbsClientInstallGuide, PbsClientInstallMethod, PBS_CLIENT_COPR_URL, PBS_CLIENT_DOC_URL,
};
pub use pbs_repository::{normalize_pbs_host, PbsRepositoryParts, RepositoryParseError};
pub use pbs_api::{PbsApiClient, PbsApiError};
pub use encryption::{
    apply_encryption_to_command, encryption_key_in_use, export_encryption_key_copy,
    fingerprints_match, key_absolute_path, normalize_fingerprint, read_key_fingerprint,
    redact_encryption_key, CreateEncryptionKeyInput, EncryptionCliMode, EncryptionKey,
    EncryptionKeyProfileUsage, ImportEncryptionKeyInput,
};
pub use profile::{
    default_backup_id, normalize_new_profile, redact_profile_for_client, ActivityLogEntry,
    BackupConditions, BackupProfile, BackupRun, CredentialVerifyInput, CredentialVerifyResult,
    BackupStartResult, ExecutionContext, HealthCheck, HealthState, NewProfile, ProfileStatus,
    RestoreStartResult, RunStatus, Schedule, ScheduleType,
};
pub use profile_yaml::{
    merge_repository_for_update, parse_profile_yaml, profile_to_yaml, resolve_encryption_key_name,
    ProfileYaml,
};
pub use pbs_repository::format_legacy_repository;
pub use preflight::{run_preflight, PreflightCheck, PreflightOptions, PreflightReport};
pub use network::list_saved_connections;
pub use secrets::{
    delete_api_token, hydrate_profile_repository, load_stored_api_token, persist_profile_credentials,
};
pub use reset::{remove_autostart_artifacts, wipe_all_local_data};
pub use restore::{
    clear_all_catalog_cache, clear_catalog_cache, enrich_encryption_keys_usage,
    find_snapshot, list_archives_for_snapshot, resolve_encryption_key_id_for_snapshot,
    resolve_snapshot_archives, CatalogEntry, ListCatalogRequest, ListCatalogResponse, PbsRestore,
    PbsSnapshotInfo, RestoreArchiveRequest, RestoreArchiveResult, RestoreConflictReport,
};
pub use health::{
    backup_group_key, display_backup_error_message, is_backup_cancelled, is_pbs_backup_lock_error,
    looks_like_pbs_backup_stderr, should_show_backup_failure_detail, stored_backup_failure_message,
};
pub use health_notify::{
    health_state_key, load_health_notify_state, parse_health_state_key, save_health_notify_state,
    HealthNotifyState,
};
pub use retry::PREFLIGHT_RETRY_DELAY;
pub use restore_paths::{archive_source_root, original_restore_target_dir};
pub use windows_setup::{check_windows_setup, WslSetupStatus};
pub use pbs::{
    backup_archive_name, backup_process_running, cancel_all_running_backups, cancel_running_backup,
    parse_snapshot_id_from_stderr,
};
pub use mount_manager::MountManager;
pub use pbs_mount::{
    check_fuse, force_unmount, fuse_available, mount_point_for, ActiveMount, FuseCheckResult,
    MountSnapshotRequest, MountSnapshotResult, UnmountSnapshotResult,
};
pub use updates::{
    app_update_checks_enabled, builtin_app_updates_enabled, can_install_update_packages,
    check_for_updates, detect_package_format, dismiss_available_update, download_package,
    install_package, installed_version, is_update_newer_than_installed, load_update_state,
    save_update_state, should_notify_user, should_run_automatic_check, update_state_path,
    verify_sha256_file, UpdateAvailability, UpdateCheckOutcome, UpdateError, UpdateState,
    GITLAB_PROJECT_URL,
};
