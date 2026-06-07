//! Install guides for `proxmox-backup-client` (APT on Debian/Ubuntu, COPR on Fedora/RHEL).

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Result of an automatic fuse3 install attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallFuse3Result {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Shell script to install `fuse3` using the system package manager.
/// Returns `None` on systems where automatic install is not supported.
pub fn fuse3_install_script() -> Option<String> {
    let guide = PbsClientInstallGuide::detect();
    match guide.method {
        PbsClientInstallMethod::Apt => Some(fuse3_apt_script()),
        PbsClientInstallMethod::DnfCopr => Some(fuse3_dnf_script()),
        PbsClientInstallMethod::Manual => None,
    }
}

fn fuse3_apt_script() -> String {
    r#"#!/usr/bin/env bash
set -euo pipefail
if [[ "${EUID:-}" -ne 0 ]]; then
  exec pkexec -- "$0" "$@"
fi
export DEBIAN_FRONTEND=noninteractive
apt-get install -y --no-install-recommends fuse3
echo "fuse3 installed."
"#
    .to_string()
}

fn fuse3_dnf_script() -> String {
    r#"#!/usr/bin/env bash
set -euo pipefail
if [[ "${EUID:-}" -ne 0 ]]; then
  exec pkexec -- "$0" "$@"
fi
dnf install -y fuse3
echo "fuse3 installed."
"#
    .to_string()
}

pub const PBS_CLIENT_DOC_URL: &str =
    "https://pbs.proxmox.com/docs/installation.html#_client_only_repositories";

/// Community COPR for Fedora / RHEL (see `derenderkeks/proxmox-backup-client`).
pub const PBS_CLIENT_COPR_URL: &str =
    "https://copr.fedorainfracloud.org/coprs/derenderkeks/proxmox-backup-client/";

/// How the dashboard can install or guide PBS client setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PbsClientInstallMethod {
    /// Official Proxmox APT repository (Debian/Ubuntu).
    Apt,
    /// `dnf copr enable derenderkeks/proxmox-backup-client` (Fedora/RHEL family).
    DnfCopr,
    /// Link to documentation only.
    Manual,
}

/// How to install the PBS client on the current system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PbsClientInstallGuide {
    pub os_id: String,
    pub system_label: String,
    pub method: PbsClientInstallMethod,
    /// Debian suite for the PBS client repo (e.g. `bookworm`), only for [`PbsClientInstallMethod::Apt`].
    pub suite: Option<String>,
    /// Debian 13+ uses deb822 `.sources` with `Signed-By`.
    pub uses_deb822_sources: bool,
}

impl PbsClientInstallGuide {
    pub fn supports_terminal_install(&self) -> bool {
        matches!(
            self.method,
            PbsClientInstallMethod::Apt | PbsClientInstallMethod::DnfCopr
        )
    }
}

impl PbsClientInstallGuide {
    pub fn detect() -> Self {
        Self::detect_from_map(read_os_release())
    }

    #[cfg(test)]
    pub fn detect_from_os_release(content: &str) -> Self {
        Self::detect_from_map(parse_os_release(content))
    }

    fn detect_from_map(os: HashMap<String, String>) -> Self {
        let id = os.get("ID").map(String::as_str).unwrap_or("linux");
        let version = os
            .get("VERSION")
            .cloned()
            .or_else(|| os.get("PRETTY_NAME").cloned())
            .unwrap_or_else(|| id.to_string());
        let codename = os
            .get("VERSION_CODENAME")
            .or_else(|| os.get("UBUNTU_CODENAME"))
            .map(String::as_str);
        let id_like = os.get("ID_LIKE").map(String::as_str);

        if let Some((suite, deb822)) = map_apt_suite(id, codename, id_like) {
            return Self {
                os_id: id.to_string(),
                system_label: version,
                method: PbsClientInstallMethod::Apt,
                suite: Some(suite),
                uses_deb822_sources: deb822,
            };
        }

        if is_rhel_family(id, id_like) {
            return Self {
                os_id: id.to_string(),
                system_label: version,
                method: PbsClientInstallMethod::DnfCopr,
                suite: None,
                uses_deb822_sources: false,
            };
        }

        Self {
            os_id: id.to_string(),
            system_label: version,
            method: PbsClientInstallMethod::Manual,
            suite: None,
            uses_deb822_sources: false,
        }
    }

    /// Shell script for `sudo bash install-pbs-client.sh` (root re-exec inside).
    pub fn install_script(&self) -> Option<String> {
        match self.method {
            PbsClientInstallMethod::DnfCopr => Some(dnf_copr_install_script()),
            PbsClientInstallMethod::Apt => self.apt_install_script(),
            PbsClientInstallMethod::Manual => None,
        }
    }

    fn apt_install_script(&self) -> Option<String> {
        let suite = self.suite.as_deref()?;

        let repo_block = if self.uses_deb822_sources {
            format!(
                r#"rm -f /etc/apt/sources.list.d/pbs-client.list \
  /etc/apt/sources.list.d/pbs-client.list.save
install -d -m 0755 /usr/share/keyrings
wget -qO /usr/share/keyrings/proxmox-archive-keyring.gpg \
  "https://enterprise.proxmox.com/debian/proxmox-archive-keyring-{suite}.gpg"
cat > /etc/apt/sources.list.d/pbs-client.sources <<'EOF'
Types: deb
URIs: http://download.proxmox.com/debian/pbs-client
Suites: {suite}
Components: main
Signed-By: /usr/share/keyrings/proxmox-archive-keyring.gpg
EOF
"#
            )
        } else {
            format!(
                r#"rm -f /etc/apt/sources.list.d/pbs-client.sources
install -d -m 0755 /usr/share/keyrings /etc/apt/trusted.gpg.d
if wget -qO /usr/share/keyrings/proxmox-archive-keyring.gpg \
    "https://enterprise.proxmox.com/debian/proxmox-archive-keyring-{suite}.gpg" 2>/dev/null; then
  echo "deb [signed-by=/usr/share/keyrings/proxmox-archive-keyring.gpg] http://download.proxmox.com/debian/pbs-client {suite} main" \
    > /etc/apt/sources.list.d/pbs-client.list
else
  wget -qO "/etc/apt/trusted.gpg.d/proxmox-release-{suite}.gpg" \
    "https://enterprise.proxmox.com/debian/proxmox-release-{suite}.gpg"
  echo "deb http://download.proxmox.com/debian/pbs-client {suite} main" \
    > /etc/apt/sources.list.d/pbs-client.list
fi
"#
            )
        };

        Some(format!(
            r#"#!/usr/bin/env bash
set -euo pipefail

if [[ "${{EUID:-}}" -ne 0 ]]; then
  exec sudo -- "$0" "$@"
fi

echo "==> Proxmox Backup Client — repository setup ({suite})"
{repo_block}
export DEBIAN_FRONTEND=noninteractive

echo "==> Updating package lists"
apt-get update -qq

echo "==> Installing base tools and libraries"
apt-get install -y --no-install-recommends \
  ca-certificates wget gnupg apt-transport-https \
  libfuse3-3 libssl3 zlib1g \
  2>/dev/null || true

if grep -qE '^VERSION_CODENAME=(bookworm)$' /etc/os-release 2>/dev/null; then
  if ! dpkg -s libfuse3-3 >/dev/null 2>&1; then
    echo "==> Trying libfuse3-3 from bookworm-backports"
    if [[ ! -f /etc/apt/sources.list.d/backuppilot-bookworm-backports.list ]]; then
      echo "deb http://deb.debian.org/debian bookworm-backports main" \
        > /etc/apt/sources.list.d/backuppilot-bookworm-backports.list
      apt-get update -qq
    fi
    apt-get install -y -t bookworm-backports --no-install-recommends libfuse3-3 \
      2>/dev/null || true
  fi
fi

if apt-cache show libqrencode4 >/dev/null 2>&1; then
  apt-get install -y --no-install-recommends libqrencode4 2>/dev/null || true
fi

install_pbs_client() {{
  if apt-get install -y --no-install-recommends proxmox-backup-client; then
    return 0
  fi
  echo
  echo "==> Standard package could not be installed (dependency conflict)."
  echo "==> Trying statically linked client (fewer dependencies)…"
  apt-get install -y --no-install-recommends proxmox-backup-client-static
}}

echo "==> Installing Proxmox Backup Client"
install_pbs_client

echo
echo "==> Installed version:"
proxmox-backup-client version
echo
echo "Done. You can close this window and return to BackupPilot."
"#,
            suite = suite,
            repo_block = repo_block
        ))
    }
}

fn dnf_copr_install_script() -> String {
    r#"#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID:-}" -ne 0 ]]; then
  exec sudo -- "$0" "$@"
fi

echo "==> Proxmox Backup Client — COPR (Fedora / RHEL)"
echo "    https://copr.fedorainfracloud.org/coprs/derenderkeks/proxmox-backup-client/"

dnf install -y dnf-plugins-core 2>/dev/null || true
dnf -y copr enable derenderkeks/proxmox-backup-client
dnf install -y proxmox-backup-client

echo
echo "==> Installed version:"
proxmox-backup-client version
echo
echo "Done. You can close this window and return to BackupPilot."
"#
    .to_string()
}

fn is_rhel_family(id: &str, id_like: Option<&str>) -> bool {
    const RHEL_IDS: &[&str] = &[
        "fedora",
        "rhel",
        "centos",
        "rocky",
        "almalinux",
        "alma",
        "ol",
        "virtuozzolinux",
        "openmandriva",
    ];
    let id = id.to_lowercase();
    if RHEL_IDS.iter().any(|&known| id == known) {
        return true;
    }
    id_like.is_some_and(|like| {
        like.split_whitespace().any(|part| {
            let part = part.to_lowercase();
            RHEL_IDS.iter().any(|&known| part == known)
        })
    })
}

fn read_os_release() -> HashMap<String, String> {
    let Ok(content) = std::fs::read_to_string("/etc/os-release") else {
        return HashMap::new();
    };
    parse_os_release(&content)
}

fn parse_os_release(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        map.insert(key.to_string(), value);
    }
    map
}

/// Returns `(suite, uses_deb822_sources)`.
fn map_apt_suite(id: &str, codename: Option<&str>, id_like: Option<&str>) -> Option<(String, bool)> {
    let id = id.to_lowercase();
    if let Some((suite, deb822)) = map_debian_codename(codename) {
        return Some((suite, deb822));
    }

    match id.as_str() {
        "debian" => map_debian_codename(codename),
        "ubuntu" | "linuxmint" | "pop" | "zorin" | "elementary" | "neon" => {
            map_ubuntu_codename(codename)
        }
        _ => {
            let debian_like = id == "debian"
                || id_like.is_some_and(|like| {
                    like.split_whitespace().any(|part| part == "debian" || part == "ubuntu")
                });
            if debian_like {
                map_ubuntu_codename(codename).or_else(|| Some(("trixie".into(), true)))
            } else {
                None
            }
        }
    }
}

fn map_debian_codename(codename: Option<&str>) -> Option<(String, bool)> {
    match codename?.to_lowercase().as_str() {
        "trixie" | "forky" => Some(("trixie".into(), true)),
        "bookworm" => Some(("bookworm".into(), true)),
        "bullseye" => Some(("bullseye".into(), false)),
        "buster" => Some(("buster".into(), false)),
        _ => None,
    }
}

/// Ubuntu releases map to the PBS client suite tested with that Debian generation.
fn map_ubuntu_codename(codename: Option<&str>) -> Option<(String, bool)> {
    match codename?.to_lowercase().as_str() {
        "questing" | "plucky" | "oracular" | "noble" | "mantic" | "lunar" => {
            Some(("trixie".into(), true))
        }
        "jammy" => Some(("bullseye".into(), false)),
        "focal" => Some(("buster".into(), false)),
        "bionic" => Some(("buster".into(), false)),
        _ => Some(("trixie".into(), true)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debian_bookworm_maps_to_client_repo() {
        let (suite, deb822) = map_apt_suite("debian", Some("bookworm"), None).unwrap();
        assert_eq!(suite, "bookworm");
        assert!(deb822);
    }

    #[test]
    fn debian_trixie_uses_deb822() {
        let (suite, deb822) = map_apt_suite("debian", Some("trixie"), None).unwrap();
        assert_eq!(suite, "trixie");
        assert!(deb822);
    }

    #[test]
    fn ubuntu_noble_uses_trixie_deb822() {
        let (suite, deb822) = map_apt_suite("ubuntu", Some("noble"), None).unwrap();
        assert_eq!(suite, "trixie");
        assert!(deb822);
    }

    #[test]
    fn fedora_uses_dnf_copr() {
        let guide = PbsClientInstallGuide::detect_from_os_release(
            "ID=fedora\nVERSION_ID=41\nPRETTY_NAME=\"Fedora Linux 41\"\n",
        );
        assert_eq!(guide.method, PbsClientInstallMethod::DnfCopr);
        let script = guide.install_script().unwrap();
        assert!(script.contains("derenderkeks/proxmox-backup-client"));
        assert!(script.contains("proxmox-backup-client"));
    }

    #[test]
    fn rhel_uses_dnf_copr() {
        let guide = PbsClientInstallGuide::detect_from_os_release(
            "ID=rhel\nVERSION_ID=9.4\nID_LIKE=\"centos fedora\"\n",
        );
        assert_eq!(guide.method, PbsClientInstallMethod::DnfCopr);
    }

    #[test]
    fn install_script_contains_suite_and_package() {
        let guide = PbsClientInstallGuide {
            os_id: "debian".into(),
            system_label: "Debian bookworm".into(),
            method: PbsClientInstallMethod::Apt,
            suite: Some("bookworm".into()),
            uses_deb822_sources: true,
        };
        let script = guide.install_script().unwrap();
        assert!(script.contains("pbs-client"));
        assert!(script.contains("proxmox-backup-client"));
        assert!(script.contains("proxmox-backup-client-static"));
        assert!(script.contains("pbs-client.sources"));
        assert!(script.contains("bookworm"));
    }

    #[test]
    fn ubuntu_install_script_uses_trixie_sources() {
        let (suite, deb822) = map_apt_suite("ubuntu", Some("noble"), None).unwrap();
        let guide = PbsClientInstallGuide {
            os_id: "ubuntu".into(),
            system_label: "Ubuntu noble".into(),
            method: PbsClientInstallMethod::Apt,
            suite: Some(suite),
            uses_deb822_sources: deb822,
        };
        let script = guide.install_script().unwrap();
        assert!(script.contains("Suites: trixie"));
        assert!(script.contains("pbs-client.sources"));
    }
}
