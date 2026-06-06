use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::pbs_repository::{encode_repository, PbsRepositoryParts};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleType {
    #[default]
    Daily,
    Hourly,
    Weekly,
    Custom,
    OnLogin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    #[serde(rename = "type")]
    pub schedule_type: ScheduleType,
    /// `HH:MM` for daily/weekly schedules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// Monday = 1 … Sunday = 7 (local time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekday: Option<u8>,
    /// Five-field cron (`minute hour day month weekday`), e.g. `0 12 * * *`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_expr: Option<String>,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            schedule_type: ScheduleType::Daily,
            time: Some("12:00".into()),
            weekday: None,
            cron_expr: None,
        }
    }
}

/// Where scheduled and manual backups for this profile are executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionContext {
    /// Session `backuppilot-daemon` (default).
    #[default]
    Desktop,
    /// Host `backuppilot-cli` (root/cron/systemd); not started by the desktop daemon.
    HostCli,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupConditions {
    #[serde(default)]
    pub execution_context: ExecutionContext,
    #[serde(default)]
    pub require_ac_power: bool,
    /// Active NetworkManager connection names (empty = any network).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub require_network: Vec<String>,
    #[serde(default)]
    pub require_vpn: bool,
    #[serde(default = "default_true")]
    pub require_server_reachable: bool,
    /// Upload cap in KiB/s for `proxmox-backup-client --rate` (`None` = unlimited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth_limit_kib_s: Option<u32>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    #[serde(default = "default_warn_days")]
    pub warn_after_days: u32,
    #[serde(default = "default_critical_days")]
    pub critical_after_days: u32,
}

fn default_warn_days() -> u32 {
    2
}

fn default_critical_days() -> u32 {
    5
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self {
            warn_after_days: default_warn_days(),
            critical_after_days: default_critical_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupProfile {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub repository: String,
    /// Set when serializing for the GUI (token is not sent over D-Bus).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub api_token_configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub backup_id: String,
    pub paths: Vec<String>,
    #[serde(default)]
    pub excludes: Vec<String>,
    pub schedule: Schedule,
    #[serde(default)]
    pub conditions: BackupConditions,
    #[serde(default)]
    pub health_check: HealthCheck,
    /// When set, backups use this PBS encryption key (`--keyfile`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key_id: Option<i64>,
    /// TLS certificate fingerprint for PBS (`PBS_FINGERPRINT`), e.g. from `proxmox-backup-manager cert info`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_fingerprint: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BackupProfile {
    pub fn to_new_profile(&self) -> NewProfile {
        NewProfile {
            name: self.name.clone(),
            enabled: self.enabled,
            repository: self.repository.clone(),
            namespace: self.namespace.clone(),
            backup_id: self.backup_id.clone(),
            paths: self.paths.clone(),
            excludes: self.excludes.clone(),
            schedule: self.schedule.clone(),
            conditions: self.conditions.clone(),
            health_check: self.health_check.clone(),
            encryption_key_id: self.encryption_key_id,
            server_fingerprint: self.server_fingerprint.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewProfile {
    pub name: String,
    pub enabled: bool,
    pub repository: String,
    pub namespace: Option<String>,
    pub backup_id: String,
    pub paths: Vec<String>,
    pub excludes: Vec<String>,
    pub schedule: Schedule,
    #[serde(default)]
    pub conditions: BackupConditions,
    #[serde(default)]
    pub health_check: HealthCheck,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_fingerprint: Option<String>,
}

/// Hostname used as PBS `backup-id` when the user leaves the field empty.
pub fn default_backup_id() -> String {
    if let Ok(host) = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("hostname"))
        .or_else(|_| std::env::var("COMPUTERNAME"))
    {
        let host = host.trim();
        if !host.is_empty() {
            return host.to_string();
        }
    }

    if let Ok(contents) = std::fs::read_to_string("/etc/hostname") {
        let host = contents.trim().trim_matches('"');
        if !host.is_empty() {
            return host.to_string();
        }
    }

    "localhost".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRun {
    pub id: i64,
    pub profile_id: i64,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default)]
    pub bytes_uploaded: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Ok,
    Warning,
    Critical,
    Unknown,
}

/// One backup run with profile name for activity / log views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityLogEntry {
    pub profile_id: i64,
    pub profile_name: String,
    pub run: BackupRun,
    /// Non-backup events (e.g. PBS client installation).
    #[serde(default)]
    pub is_system: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileStatus {
    pub profile_id: i64,
    pub profile_name: String,
    pub enabled: bool,
    pub health: HealthState,
    pub last_run: Option<BackupRun>,
    pub backup_in_progress: bool,
    /// Latest line from `proxmox-backup-client` while a backup is running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_message: Option<String>,
    /// Days since the last successful backup (`None` if never succeeded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub days_since_last_success: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialVerifyInput {
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialVerifyResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Result of `run_backup` — the job may run in the background or be skipped by preflight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStartResult {
    pub run_id: i64,
    /// `proxmox-backup-client` was spawned.
    pub started: bool,
    /// Preflight blocked the run (see `message`).
    pub skipped: bool,
    /// When skipped: scheduler may retry later in the same slot (network/VPN/AC/PBS).
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Start was rejected because this profile already has a running backup.
    #[serde(default)]
    pub already_running: bool,
}

/// Restore started in the background (conflicts return immediately).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreStartResult {
    pub started: bool,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
}

/// Fills empty `backup_id` with the host name (same as the GUI/daemon).
pub fn normalize_new_profile(mut profile: NewProfile) -> NewProfile {
    if profile.backup_id.trim().is_empty() {
        profile.backup_id = default_backup_id();
    }
    profile
}

/// Strips API tokens before sending a profile to the GUI over D-Bus.
pub fn redact_profile_for_client(mut profile: BackupProfile) -> BackupProfile {
    profile.api_token_configured = crate::secrets::has_api_token(profile.id);
    if let Ok(mut parts) = PbsRepositoryParts::parse(&profile.repository) {
        if !parts.api_token_secret().is_empty() {
            parts.token.clear();
            profile.repository = encode_repository(&parts);
        }
    }
    profile
}
