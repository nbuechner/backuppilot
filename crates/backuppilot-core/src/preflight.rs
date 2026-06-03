//! Preflight checks before starting a backup.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use tracing::debug;

use crate::app_settings::load_app_settings;
use crate::network::{network_condition_met, vpn_condition_met};
use crate::pbs::PbsClient;
use crate::pbs_repository::PbsRepositoryParts;
use crate::profile::BackupProfile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightCheck {
    pub id: String,
    pub label: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightReport {
    pub ok: bool,
    pub checks: Vec<PreflightCheck>,
}

impl PreflightReport {
    pub fn reasons(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| {
                if let Some(d) = &c.detail {
                    format!("{}: {d}", c.label)
                } else {
                    c.label.clone()
                }
            })
            .collect()
    }

    pub fn message(&self) -> String {
        self.reasons().join("; ")
    }

    pub fn retryable(&self) -> bool {
        self.checks
            .iter()
            .filter(|c| !c.ok)
            .any(|c| is_transient_check_id(&c.id))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PreflightOptions {
    /// Retries PBS reachability when `true` (scheduled backups).
    pub scheduled: bool,
}

impl Default for PreflightOptions {
    fn default() -> Self {
        Self { scheduled: false }
    }
}

const SCHEDULED_SERVER_RETRIES: u32 = 6;
const SCHEDULED_RETRY_DELAY: Duration = Duration::from_secs(10);

pub async fn run_preflight(profile: &BackupProfile, options: PreflightOptions) -> PreflightReport {
    let mut checks = Vec::new();

    let network_up = network_available().await;
    push_check(
        &mut checks,
        "network_link",
        "Network available",
        network_up,
        if network_up {
            None
        } else {
            Some("no network connectivity detected".into())
        },
    );

    push_check(
        &mut checks,
        "enabled",
        "Profile enabled",
        profile.enabled,
        if profile.enabled {
            None
        } else {
            Some("profile is disabled".into())
        },
    );

    push_check(
        &mut checks,
        "paths",
        "Backup paths configured",
        !profile.paths.is_empty(),
        if profile.paths.is_empty() {
            Some("no backup paths configured".into())
        } else {
            None
        },
    );

    for path in &profile.paths {
        let p = std::path::Path::new(path);
        if !p.exists() {
            push_check(
                &mut checks,
                "path_exists",
                "Backup path exists",
                false,
                Some(format!("path does not exist: {path}")),
            );
        } else {
            let readable = path_is_readable(p);
            push_check(
                &mut checks,
                "path_readable",
                "Read permission on backup path",
                readable,
                if readable {
                    None
                } else {
                    Some(format!("no read permission: {path}"))
                },
            );
        }
    }

    let pbs_available = PbsClient::is_available().await;
    push_check(
        &mut checks,
        "pbs_client",
        "proxmox-backup-client installed",
        pbs_available,
        if pbs_available {
            None
        } else {
            Some("proxmox-backup-client not found".into())
        },
    );

    let token_ok = crate::secrets::has_api_token(profile.id);
    push_check(
        &mut checks,
        "api_token",
        "API token stored",
        token_ok,
        if token_ok {
            None
        } else {
            Some(
                "API token not available for background backups — open the profile and save again"
                    .into(),
            )
        },
    );

    let conditions = &profile.conditions;

    if conditions.require_ac_power {
        let on_ac = on_ac_power();
        push_check(
            &mut checks,
            "ac_power",
            "On AC power",
            on_ac,
            if on_ac {
                None
            } else {
                Some("device not on AC power".into())
            },
        );
    }

    if !conditions.require_network.is_empty() {
        let met = network_condition_met(&conditions.require_network);
        let names = conditions.require_network.join(", ");
        push_check(
            &mut checks,
            "network",
            "Required network active",
            met,
            if met {
                None
            } else {
                Some(format!("required network not active ({names})"))
            },
        );
    }

    if conditions.require_vpn {
        let met = vpn_condition_met(true);
        push_check(
            &mut checks,
            "vpn",
            "VPN connection active",
            met,
            if met {
                None
            } else {
                Some("VPN connection required but not active".into())
            },
        );
    }

    if conditions.require_server_reachable {
        match check_pbs_reachable(profile, options.scheduled).await {
            Ok(()) => {
                push_check(
                    &mut checks,
                    "dns",
                    "PBS DNS resolution",
                    true,
                    None,
                );
                push_check(
                    &mut checks,
                    "tcp",
                    "PBS TCP port reachable",
                    true,
                    None,
                );
                push_check(
                    &mut checks,
                    "datastore",
                    "PBS datastore reachable",
                    true,
                    None,
                );
                push_check(
                    &mut checks,
                    "pbs_write",
                    "PBS write access (repository login)",
                    true,
                    None,
                );
            }
            Err(err) => {
                let id = classify_pbs_error(&err);
                push_check(&mut checks, id, pbs_check_label(id), false, Some(err.clone()));
                if id == "pbs_auth" {
                    push_check(
                        &mut checks,
                        "datastore",
                        "PBS datastore reachable",
                        false,
                        Some(err.clone()),
                    );
                    push_check(
                        &mut checks,
                        "pbs_write",
                        "PBS write access (repository login)",
                        false,
                        Some(err),
                    );
                }
            }
        }
    }

    let ok = checks.iter().all(|c| c.ok);
    if !ok {
        debug!(profile_id = profile.id, ?checks, "preflight failed");
    }

    PreflightReport { ok, checks }
}

/// Backup source paths only need to be readable (not writable).
fn path_is_readable(path: &std::path::Path) -> bool {
    if path.is_dir() {
        return std::fs::read_dir(path).is_ok();
    }
    if path.is_file() {
        return std::fs::File::open(path).is_ok();
    }
    if path.exists() {
        return std::fs::metadata(path).is_ok();
    }
    false
}

fn push_check(
    checks: &mut Vec<PreflightCheck>,
    id: &str,
    label: &str,
    ok: bool,
    detail: Option<String>,
) {
    checks.push(PreflightCheck {
        id: id.to_string(),
        label: label.to_string(),
        ok,
        detail,
    });
}

pub fn is_transient_check_id(id: &str) -> bool {
    matches!(
        id,
        "ac_power" | "network_link" | "network" | "vpn" | "dns" | "tcp" | "datastore" | "pbs_auth"
            | "pbs_reachable"
    )
}

fn classify_pbs_error(err: &str) -> &'static str {
    let lower = err.to_lowercase();
    if lower.contains("dns") {
        "dns"
    } else if lower.contains("cannot reach") || lower.contains("port") || lower.contains("firewall")
    {
        "tcp"
    } else {
        "pbs_auth"
    }
}

fn pbs_check_label(id: &str) -> &'static str {
    match id {
        "dns" => "PBS DNS resolution",
        "tcp" => "PBS TCP port reachable",
        _ => "PBS authentication",
    }
}

async fn check_pbs_reachable(profile: &BackupProfile, scheduled: bool) -> Result<(), String> {
    let parts = PbsRepositoryParts::parse(&profile.repository)
        .map_err(|e| format!("invalid PBS connection settings: {e}"))?;

    let host = parts.host_for_reachability();
    if host.is_empty() {
        return Err("PBS hostname is missing".into());
    }

    if !dns_resolves(host).await {
        return Err(format!("DNS lookup failed for PBS host {host}"));
    }

    let endpoint = parts.tcp_connect_address();
    if !tcp_reachable(&endpoint).await {
        return Err(format!(
            "cannot reach PBS port at {endpoint} (network or firewall); default port is 8007"
        ));
    }

    let max_attempts = if scheduled {
        SCHEDULED_SERVER_RETRIES
    } else {
        1
    };
    let mut last_err: Option<String> = None;
    for attempt in 1..=max_attempts {
        match PbsClient::check_repository_accessible(
            &profile.repository,
            profile.namespace.as_deref(),
            profile.server_fingerprint.as_deref(),
        )
        .await
        {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(err) => {
                let retryable = err.starts_with("cannot reach ");
                debug!(
                    profile_id = profile.id,
                    attempt,
                    max_attempts,
                    retryable,
                    %err,
                    "pbs reachability check failed"
                );
                last_err = Some(err);
                if retryable && attempt < max_attempts {
                    sleep(SCHEDULED_RETRY_DELAY).await;
                } else {
                    break;
                }
            }
        }
    }
    if let Some(err) = last_err {
        return Err(err);
    }
    Ok(())
}

async fn dns_resolves(host: &str) -> bool {
    let host = host.split(':').next().unwrap_or(host).trim();
    if host.is_empty() {
        return false;
    }
    matches!(
        tokio::time::timeout(
            Duration::from_secs(8),
            tokio::net::lookup_host((host, 0u16))
        )
        .await,
        Ok(Ok(_))
    )
}

async fn tcp_reachable(addr: &str) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(8), tokio::net::TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

async fn network_available() -> bool {
    if tokio::net::TcpStream::connect(("1.1.1.1", 443)).await.is_ok() {
        return true;
    }
    dns_resolves("one.one.one.one").await
}

fn on_ac_power() -> bool {
    std::fs::read_to_string("/sys/class/power_supply/AC/online")
        .or_else(|_| std::fs::read_to_string("/sys/class/power_supply/ACAD/online"))
        .map(|s| s.trim() == "1")
        .unwrap_or(true)
}

/// Global pause flag from application settings.
pub fn backups_globally_paused() -> bool {
    load_app_settings().tray.pause_all_backups
}
