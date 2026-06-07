//! Application-wide settings (JSON in the config directory).

use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::paths::{config_dir, ensure_data_dirs};
use crate::profile::{BackupConditions, HealthCheck};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettings {
    /// Start tray icon and background daemon when the user logs in.
    #[serde(default)]
    pub start_at_login: bool,
    #[serde(default)]
    pub conditions: BackupConditions,
    #[serde(default)]
    pub health_check: HealthCheck,
    #[serde(default)]
    pub notifications: NotificationSettings,
    #[serde(default)]
    pub tray: TraySettings,
    #[serde(default)]
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub updates: UpdateSettings,
    #[serde(default)]
    pub onboarding: OnboardingSettings,
    #[serde(default)]
    pub log_retention: LogRetentionSettings,
    #[serde(default)]
    migrated_from_profiles_v1: bool,
}

/// How long backup runs and system events stay in the activity log database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRetentionSettings {
    /// Delete entries older than this many days. `0` keeps all entries (no automatic pruning).
    #[serde(default = "default_log_retain_days")]
    pub retain_days: u32,
}

impl Default for LogRetentionSettings {
    fn default() -> Self {
        Self {
            retain_days: default_log_retain_days(),
        }
    }
}

fn default_log_retain_days() -> u32 {
    90
}

const LOG_RETAIN_DAYS_MIN: u32 = 7;
const LOG_RETAIN_DAYS_MAX: u32 = 3650;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnboardingSettings {
    /// User completed or skipped the first-run setup assistant.
    #[serde(default)]
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub notify_on_success: bool,
    #[serde(default = "default_true")]
    pub notify_on_failure: bool,
    #[serde(default = "default_true")]
    pub notify_on_skipped: bool,
    #[serde(default = "default_true")]
    pub notify_on_restore: bool,
    #[serde(default = "default_true")]
    pub notify_on_health_warning: bool,
    #[serde(default = "default_true")]
    pub notify_on_update: bool,
    /// Persistent notification with progress while a backup is running.
    #[serde(default = "default_true")]
    pub notify_on_backup_progress: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            notify_on_success: false,
            notify_on_failure: true,
            notify_on_skipped: true,
            notify_on_restore: true,
            notify_on_health_warning: true,
            notify_on_update: true,
            notify_on_backup_progress: true,
        }
    }
}

impl NotificationSettings {
    pub fn should_notify_backup_progress(&self) -> bool {
        self.enabled && self.notify_on_backup_progress
    }

    pub fn should_notify_backup(&self, status: crate::profile::RunStatus) -> bool {
        if !self.enabled {
            return false;
        }
        use crate::profile::RunStatus;
        match status {
            RunStatus::Success => self.notify_on_success,
            RunStatus::Failed => self.notify_on_failure,
            RunStatus::Skipped => self.notify_on_skipped,
            _ => false,
        }
    }

    pub fn should_notify_restore(&self) -> bool {
        self.enabled && self.notify_on_restore
    }

    pub fn should_notify_health(&self, state: crate::profile::HealthState) -> bool {
        if !self.enabled || !self.notify_on_health_warning {
            return false;
        }
        matches!(
            state,
            crate::profile::HealthState::Warning | crate::profile::HealthState::Critical
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraySettings {
    #[serde(default = "default_true")]
    pub show_tray_icon: bool,
    #[serde(default)]
    pub pause_all_backups: bool,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// Ask to unmount active FUSE mounts when closing the app.
    #[serde(default = "default_true")]
    pub ask_unmount_on_quit: bool,
}

impl Default for TraySettings {
    fn default() -> Self {
        Self {
            show_tray_icon: true,
            pause_all_backups: false,
            close_to_tray: true,
            ask_unmount_on_quit: true,
        }
    }
}

/// Mirrors `backuppilot_i18n::UiLanguage` for persistence (same serde names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiLanguage {
    #[default]
    System,
    German,
    English,
    French,
    Italian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppearanceSettings {
    #[serde(default)]
    pub language: UiLanguage,
    #[serde(default)]
    pub color_scheme: ColorScheme,
    /// Show expert options in settings and the profile editor.
    #[serde(default)]
    pub advanced_mode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    #[serde(default = "default_true")]
    pub check_automatically: bool,
    #[serde(default)]
    pub channel: UpdateChannel,
    #[serde(default = "default_true")]
    pub notify_when_available: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check_automatically: true,
            channel: UpdateChannel::Stable,
            notify_when_available: true,
        }
    }
}

fn default_true() -> bool {
    true
}

pub fn settings_path() -> PathBuf {
    config_dir().join("app-settings.json")
}

pub fn load_app_settings() -> AppSettings {
    let path = settings_path();
    let mut settings = if let Ok(data) = fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        AppSettings::default()
    };

    if !settings.migrated_from_profiles_v1 {
        if let Ok(db) = crate::db::Database::open() {
            if let Ok(legacy) = db.list_legacy_profile_settings() {
                migrate_from_profiles(&mut settings, &legacy);
            }
        }
        settings.migrated_from_profiles_v1 = true;
        let _ = save_app_settings(&settings);
    }

    settings
}

pub fn save_app_settings(settings: &AppSettings) -> Result<()> {
    ensure_data_dirs().map_err(CoreError::Io)?;
    let data = serde_json::to_string_pretty(settings)?;
    fs::write(settings_path(), data).map_err(CoreError::Io)
}

fn migrate_from_profiles(
    settings: &mut AppSettings,
    legacy: &[crate::db::LegacyProfileSettings],
) {
    let Some(first) = legacy.first() else {
        return;
    };
    settings.conditions = first.conditions.clone();
    settings.health_check = first.health_check.clone();
}

pub fn validate_health_check(health: &HealthCheck) -> std::result::Result<(), String> {
    if health.critical_after_days < health.warn_after_days {
        return Err(
            "Critical threshold must be greater than or equal to the warning threshold.".into(),
        );
    }
    Ok(())
}

pub fn validate_log_retention(settings: &LogRetentionSettings) -> std::result::Result<(), String> {
    if settings.retain_days == 0 {
        return Ok(());
    }
    if settings.retain_days < LOG_RETAIN_DAYS_MIN {
        return Err(format!(
            "Log retention must be at least {LOG_RETAIN_DAYS_MIN} days, or 0 to keep all entries."
        ));
    }
    if settings.retain_days > LOG_RETAIN_DAYS_MAX {
        return Err(format!(
            "Log retention cannot exceed {LOG_RETAIN_DAYS_MAX} days."
        ));
    }
    Ok(())
}

/// Cutoff timestamp for pruning; `None` when retention is disabled (`retain_days == 0`).
pub fn activity_log_cutoff(settings: &LogRetentionSettings) -> Option<DateTime<Utc>> {
    if settings.retain_days == 0 {
        return None;
    }
    Some(Utc::now() - chrono::Duration::days(settings.retain_days as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_retention_default_is_ninety_days() {
        assert_eq!(LogRetentionSettings::default().retain_days, 90);
    }

    #[test]
    fn log_retention_zero_is_unlimited() {
        assert!(validate_log_retention(&LogRetentionSettings { retain_days: 0 }).is_ok());
        assert!(activity_log_cutoff(&LogRetentionSettings { retain_days: 0 }).is_none());
    }

    #[test]
    fn log_retention_rejects_out_of_range() {
        assert!(validate_log_retention(&LogRetentionSettings { retain_days: 6 }).is_err());
        assert!(validate_log_retention(&LogRetentionSettings { retain_days: 3651 }).is_err());
        assert!(validate_log_retention(&LogRetentionSettings { retain_days: 90 }).is_ok());
    }
}
