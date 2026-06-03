//! Network / VPN conditions for scheduled backups (NetworkManager when available).

use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct ActiveConnections {
    pub names: Vec<String>,
    pub has_vpn: bool,
}

pub fn active_connections() -> ActiveConnections {
    let mut out = ActiveConnections::default();
    let Ok(output) = Command::new("nmcli")
        .args(["-t", "-f", "NAME,TYPE", "con", "show", "--active"])
        .output()
    else {
        return out;
    };
    if !output.status.success() {
        return out;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (name, conn_type) = match line.split_once(':') {
            Some(pair) => pair,
            None => (line, ""),
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        out.names.push(name.to_string());
        let kind = conn_type.to_ascii_lowercase();
        if kind.contains("vpn") || kind == "tun" || kind == "tap" {
            out.has_vpn = true;
        }
    }
    out
}

/// Saved NetworkManager connection names (for profile network picker).
pub fn list_saved_connections() -> Vec<String> {
    let Ok(output) = Command::new("nmcli")
        .args(["-t", "-f", "NAME", "connection", "show"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut names: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    names.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
    names.dedup();
    names
}

/// Returns `true` when no specific network is required or a required name is active.
pub fn network_condition_met(required: &[String]) -> bool {
    if required.is_empty() {
        return true;
    }
    let active = active_connections();
    required.iter().any(|want| {
        let want = want.trim();
        !want.is_empty()
            && active
                .names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(want))
    })
}

pub fn vpn_condition_met(require_vpn: bool) -> bool {
    if !require_vpn {
        return true;
    }
    active_connections().has_vpn
}
