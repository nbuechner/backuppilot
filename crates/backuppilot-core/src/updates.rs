//! GitLab release checks, package download, and system package installation.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::app_settings::UpdateChannel;
use crate::paths::{config_dir, ensure_data_dirs};

/// Public project page (releases are published here).
pub const GITLAB_PROJECT_URL: &str = "https://git.onesystems.ch/backuppilot/app";

const DEFAULT_RELEASES_API: &str =
    "https://git.onesystems.ch/api/v4/projects/backuppilot%2Fapp/releases";

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("network error: {0}")]
    Network(String),

    #[error("release API error: {0}")]
    Api(String),

    #[error("no suitable release package found for this system")]
    NoPackage,

    #[error("version parse error: {0}")]
    Version(String),

    #[error("download failed: {0}")]
    Download(String),

    #[error("checksum mismatch (expected {expected}, got {actual})")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("unsupported package format")]
    UnsupportedFormat,

    #[error("installation failed: {0}")]
    Install(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type UpdateResult<T> = std::result::Result<T, UpdateError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageFormat {
    Deb,
    Rpm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAvailability {
    pub version: String,
    pub tag: String,
    pub download_url: String,
    pub package_filename: String,
    pub sha256: Option<String>,
    pub release_url: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<UpdateAvailability>,
    /// User dismissed notification for this tag (until a newer tag appears).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed_tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateCheckOutcome {
    UpToDate,
    UpdateAvailable { availability: UpdateAvailability },
    Error { message: String },
}

pub fn installed_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// GitLab release checks (GUI, daemon, notifications) are enabled for all install types.
pub fn app_update_checks_enabled() -> bool {
    true
}

/// Alias for [`app_update_checks_enabled`].
pub fn builtin_app_updates_enabled() -> bool {
    app_update_checks_enabled()
}

/// True when the app may download and install a native `.deb` or `.rpm` package.
pub fn can_install_update_packages() -> bool {
    !crate::paths::is_flatpak_runtime() && detect_package_format().is_some()
}

pub fn update_state_path() -> PathBuf {
    config_dir().join("update-state.json")
}

pub fn load_update_state() -> UpdateState {
    let path = update_state_path();
    let Ok(data) = std::fs::read_to_string(&path) else {
        return UpdateState::default();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save_update_state(state: &UpdateState) -> UpdateResult<()> {
    ensure_data_dirs().map_err(UpdateError::Io)?;
    let data = serde_json::to_string_pretty(state)?;
    std::fs::write(update_state_path(), data).map_err(UpdateError::Io)
}

pub fn should_run_automatic_check(state: &UpdateState) -> bool {
    let Some(last) = state.last_check_at else {
        return true;
    };
    Utc::now()
        .signed_duration_since(last)
        .to_std()
        .unwrap_or(CHECK_INTERVAL)
        >= CHECK_INTERVAL
}

pub fn is_update_newer_than_installed(available: &UpdateAvailability) -> bool {
    version_gt(&available.version, installed_version())
}

pub fn should_notify_user(state: &UpdateState, available: &UpdateAvailability) -> bool {
    if state.dismissed_tag.as_deref() == Some(available.tag.as_str()) {
        return false;
    }
    is_update_newer_than_installed(available)
}

/// Run a release check against GitLab and persist [`UpdateState`].
pub async fn check_for_updates(channel: UpdateChannel) -> UpdateCheckOutcome {
    let mut state = load_update_state();
    state.last_check_at = Some(Utc::now());
    state.last_error = None;

    match fetch_latest_release(channel).await {
        Ok(Some(info)) => {
            if is_update_newer_than_installed(&info) {
                state.available = Some(info.clone());
                let _ = save_update_state(&state);
                UpdateCheckOutcome::UpdateAvailable {
                    availability: info,
                }
            } else {
                state.available = None;
                let _ = save_update_state(&state);
                UpdateCheckOutcome::UpToDate
            }
        }
        Ok(None) => {
            state.available = None;
            let _ = save_update_state(&state);
            UpdateCheckOutcome::UpToDate
        }
        Err(err) => {
            let msg = err.to_string();
            state.last_error = Some(msg.clone());
            let _ = save_update_state(&state);
            UpdateCheckOutcome::Error { message: msg }
        }
    }
}

pub fn dismiss_available_update() {
    let mut state = load_update_state();
    if let Some(av) = state.available.clone() {
        state.dismissed_tag = Some(av.tag);
    }
    let _ = save_update_state(&state);
}

pub fn detect_package_format() -> Option<PackageFormat> {
    if Path::new("/etc/debian_version").exists() || Path::new("/etc/dpkg").exists() {
        return Some(PackageFormat::Deb);
    }
    if Path::new("/etc/redhat-release").exists()
        || Path::new("/etc/fedora-release").exists()
        || Path::new("/etc/SuSE-release").exists()
        || Path::new("/etc/os-release").exists()
            && std::fs::read_to_string("/etc/os-release")
                .map(|s| s.contains("ID=fedora") || s.contains("ID=rhel") || s.contains("ID=suse"))
                .unwrap_or(false)
    {
        return Some(PackageFormat::Rpm);
    }
    None
}

pub fn expected_package_filename(version: &str, format: PackageFormat) -> String {
    match format {
        PackageFormat::Deb => format!("backuppilot_{version}_{}.deb", deb_arch()),
        PackageFormat::Rpm => format!(
            "backuppilot-{version}-1.{}.rpm",
            rpm_arch()
        ),
    }
}

pub async fn download_package(
    availability: &UpdateAvailability,
    progress: impl Fn(u64, Option<u64>) + Send + Sync + 'static,
) -> UpdateResult<PathBuf> {
    if !can_install_update_packages() {
        return Err(UpdateError::UnsupportedFormat);
    }
    ensure_data_dirs().map_err(UpdateError::Io)?;
    let dest = config_dir()
        .join("downloads")
        .join(&availability.package_filename);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let client = http_client()?;
    let response = client
        .get(&availability.download_url)
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?;

    if !response.status().is_success() {
        return Err(UpdateError::Download(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let total = response.content_length();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| UpdateError::Download(e.to_string()))?;
    std::fs::write(&dest, &bytes)?;
    progress(bytes.len() as u64, total);

    if let Some(expected) = availability.sha256.as_deref() {
        verify_sha256_file(&dest, expected)?;
    }

    Ok(dest)
}

pub fn verify_sha256_file(path: &Path, expected_hex: &str) -> UpdateResult<()> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path)?;
    let hash = Sha256::digest(&data);
    let actual = hex::encode(hash);
    let expected = expected_hex.trim().to_ascii_lowercase();
    if actual != expected {
        return Err(UpdateError::ChecksumMismatch {
            expected: expected.clone(),
            actual,
        });
    }
    Ok(())
}

pub fn install_package(path: &Path) -> UpdateResult<()> {
    let format = package_format_for_path(path)?;
    let path_str = path.to_string_lossy();
    let status = match format {
        PackageFormat::Deb => {
            if command_exists("apt") {
                run_privileged(&["apt", "install", "-y", &path_str])
            } else {
                run_privileged(&["dpkg", "-i", &path_str])
            }
        }
        PackageFormat::Rpm => run_privileged(&["rpm", "-Uvh", &path_str]),
    }?;
    if status.success() {
        Ok(())
    } else {
        Err(UpdateError::Install(format!(
            "installer exited with {}",
            status.code().unwrap_or(-1)
        )))
    }
}

fn run_privileged(args: &[&str]) -> UpdateResult<std::process::ExitStatus> {
    let mut cmd = std::process::Command::new("pkexec");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = cmd.output()?;
    if output.status.success() {
        return Ok(output.status);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("dismissed") || stderr.contains("not authorized") {
        return Err(UpdateError::Install(
            "Administrator rights are required to install the update.".into(),
        ));
    }
    Err(UpdateError::Install(if stderr.is_empty() {
        format!("command failed: pkexec {}", args.join(" "))
    } else {
        stderr.trim().to_string()
    }))
}

fn package_format_for_path(path: &Path) -> UpdateResult<PackageFormat> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".deb") {
        Ok(PackageFormat::Deb)
    } else if name.ends_with(".rpm") {
        Ok(PackageFormat::Rpm)
    } else {
        Err(UpdateError::UnsupportedFormat)
    }
}

fn http_client() -> UpdateResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(format!("BackupPilot/{}", installed_version()));
    if let Ok(token) = std::env::var("BACKUPPILOT_GITLAB_TOKEN") {
        if !token.is_empty() {
            builder = builder.default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {token}").parse().unwrap(),
                );
                headers
            });
        }
    }
    builder
        .build()
        .map_err(|e| UpdateError::Network(e.to_string()))
}

async fn fetch_latest_release(channel: UpdateChannel) -> UpdateResult<Option<UpdateAvailability>> {
    let api_url = std::env::var("BACKUPPILOT_RELEASES_API_URL")
        .unwrap_or_else(|_| DEFAULT_RELEASES_API.to_string());
    let client = http_client()?;
    let releases: Vec<GitLabRelease> = client
        .get(&api_url)
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| UpdateError::Api(e.to_string()))?
        .json()
        .await
        .map_err(|e| UpdateError::Api(e.to_string()))?;

    let info_only = crate::paths::is_flatpak_runtime();

    for release in releases {
        if !release_matches_channel(channel, &release.tag_name) {
            continue;
        }
        let version = normalize_tag_version(&release.tag_name)?;
        if info_only {
            return Ok(Some(availability_from_release(&release, &version)));
        }
        let format = detect_package_format().ok_or(UpdateError::NoPackage)?;
        if let Some(mut asset) = pick_release_asset(&release, format, &version) {
            if asset.sha256.is_none() {
                asset.sha256 = fetch_sha256_sidecar(&client, &release.assets.links, &asset.filename)
                    .await
                    .ok()
                    .flatten();
            }
            let release_url = format!("{GITLAB_PROJECT_URL}/-/releases/{}", release.tag_name);
            return Ok(Some(UpdateAvailability {
                version,
                tag: release.tag_name.clone(),
                download_url: asset.url,
                package_filename: asset.filename,
                sha256: asset.sha256,
                release_url,
                notes: release.description.filter(|d| !d.trim().is_empty()),
            }));
        }
    }

    Ok(None)
}

fn availability_from_release(release: &GitLabRelease, version: &str) -> UpdateAvailability {
    let release_url = format!("{GITLAB_PROJECT_URL}/-/releases/{}", release.tag_name);
    let (download_url, package_filename) = pick_info_release_asset(release, version);
    UpdateAvailability {
        version: version.to_string(),
        tag: release.tag_name.clone(),
        download_url,
        package_filename,
        sha256: parse_sha256_from_description(release.description.as_deref()),
        release_url,
        notes: release
            .description
            .clone()
            .filter(|d| !d.trim().is_empty()),
    }
}

/// Prefer a `.flatpak` asset; otherwise any BackupPilot release link (info-only installs).
fn pick_info_release_asset(release: &GitLabRelease, version: &str) -> (String, String) {
    let flatpak_name = format!("backuppilot-{version}.flatpak");
    for link in &release.assets.links {
        let name = link.name.as_str();
        if name.eq_ignore_ascii_case(&flatpak_name) || name.to_ascii_lowercase().ends_with(".flatpak") {
            let url = link
                .direct_asset_url
                .clone()
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| link.url.clone());
            return (url, name.to_string());
        }
    }
    for link in &release.assets.links {
        if link.name.to_ascii_lowercase().contains("backuppilot") {
            let url = link
                .direct_asset_url
                .clone()
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| link.url.clone());
            return (url, link.name.clone());
        }
    }
    (String::new(), flatpak_name)
}

fn release_matches_channel(channel: UpdateChannel, tag: &str) -> bool {
    let beta = is_beta_tag(tag);
    match channel {
        UpdateChannel::Beta => beta,
        UpdateChannel::Stable => !beta,
    }
}

fn is_beta_tag(tag: &str) -> bool {
    let lower = tag.to_ascii_lowercase();
    lower.contains("beta")
        || lower.contains("alpha")
        || lower.contains("rc")
        || lower.contains("preview")
}

#[derive(Debug, Deserialize)]
struct GitLabRelease {
    tag_name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    assets: GitLabAssets,
}

#[derive(Debug, Default, Deserialize)]
struct GitLabAssets {
    #[serde(default)]
    links: Vec<GitLabAssetLink>,
}

#[derive(Debug, Deserialize)]
struct GitLabAssetLink {
    name: String,
    url: String,
    #[serde(default)]
    direct_asset_url: Option<String>,
}

struct PickedAsset {
    url: String,
    filename: String,
    sha256: Option<String>,
}

fn pick_release_asset(
    release: &GitLabRelease,
    format: PackageFormat,
    version: &str,
) -> Option<PickedAsset> {
    let expected = expected_package_filename(version, format);
    let mut links: Vec<_> = release.assets.links.iter().collect();
    links.sort_by(|a, b| score_asset(b, &expected).cmp(&score_asset(a, &expected)));

    let primary = links
        .iter()
        .find(|link| score_asset(link, &expected) > 0)
        .or_else(|| links.first())?;

    let filename = primary.name.clone();
    let url = primary
        .direct_asset_url
        .clone()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| primary.url.clone());

    let sha256 = parse_sha256_from_description(release.description.as_deref());

    Some(PickedAsset {
        url,
        filename,
        sha256,
    })
}

fn score_asset(link: &GitLabAssetLink, expected: &str) -> i32 {
    let name = link.name.to_ascii_lowercase();
    if link.name == expected {
        return 100;
    }
    if name == expected.to_ascii_lowercase() {
        return 95;
    }
    if name.contains("backuppilot") && name.ends_with(".deb") && expected.ends_with(".deb") {
        return 50;
    }
    if name.contains("backuppilot") && name.ends_with(".rpm") && expected.ends_with(".rpm") {
        return 50;
    }
    0
}

async fn fetch_sha256_sidecar(
    client: &reqwest::Client,
    links: &[GitLabAssetLink],
    filename: &str,
) -> UpdateResult<Option<String>> {
    let sidecar = format!("{filename}.sha256");
    let Some(link) = links.iter().find(|l| l.name == sidecar) else {
        return Ok(None);
    };
    let url = link
        .direct_asset_url
        .clone()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| link.url.clone());
    let text = client
        .get(&url)
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| UpdateError::Api(e.to_string()))?
        .text()
        .await
        .map_err(|e| UpdateError::Download(e.to_string()))?;
    Ok(parse_sha256_sidecar_text(&text))
}

fn parse_sha256_sidecar_text(text: &str) -> Option<String> {
    for line in text.lines() {
        let token = line.split_whitespace().next()?;
        if token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(token.to_ascii_lowercase());
        }
    }
    None
}

fn parse_sha256_from_description(description: Option<&str>) -> Option<String> {
    let text = description?;
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("sha256") {
            if let Some(hex) = line.split(':').nth(1) {
                let hex = hex.trim();
                if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(hex.to_ascii_lowercase());
                }
            }
        }
    }
    None
}

fn normalize_tag_version(tag: &str) -> UpdateResult<String> {
    let trimmed = tag.trim().trim_start_matches('v').trim_start_matches('V');
    semver::Version::parse(trimmed)
        .map(|_| trimmed.to_string())
        .map_err(|e| UpdateError::Version(e.to_string()))
}

fn version_gt(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(va), Ok(vb)) => va > vb,
        _ => a != b,
    }
}

fn deb_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn rpm_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => other,
    }
}

fn command_exists(cmd: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {cmd}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
