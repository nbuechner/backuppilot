//! Persisted state for health-warning desktop notifications.

use std::collections::HashMap;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths::{config_dir, ensure_data_dirs};
use crate::profile::HealthState;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HealthNotifyState {
    /// Last notified health per profile id (`"ok"`, `"warning"`, `"critical"`, `"unknown"`).
    #[serde(default)]
    pub by_profile: HashMap<String, String>,
}

fn state_path() -> std::path::PathBuf {
    config_dir().join("health-notify-state.json")
}

pub fn load_health_notify_state() -> HealthNotifyState {
    let path = state_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

pub fn save_health_notify_state(state: &HealthNotifyState) -> std::io::Result<()> {
    ensure_data_dirs()?;
    let data = serde_json::to_string_pretty(state)?;
    fs::write(state_path(), data)
}

pub fn health_state_key(state: HealthState) -> &'static str {
    match state {
        HealthState::Ok => "ok",
        HealthState::Warning => "warning",
        HealthState::Critical => "critical",
        HealthState::Unknown => "unknown",
    }
}

pub fn parse_health_state_key(key: &str) -> Option<HealthState> {
    match key {
        "ok" => Some(HealthState::Ok),
        "warning" => Some(HealthState::Warning),
        "critical" => Some(HealthState::Critical),
        "unknown" => Some(HealthState::Unknown),
        _ => None,
    }
}
