use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::encryption::{apply_encryption_to_command, normalize_fingerprint, EncryptionCliMode};
use crate::error::{CoreError, Result};
use crate::paths::{
    profile_backup_client_config, runtime_profile_config_dir,
};
use crate::pbs_repository::{PbsRepositoryParts, PBS_AUTH_ID_MAX_LEN};
use crate::profile::{BackupProfile, CredentialVerifyResult};

const BACKUP_TIMEOUT: Duration = Duration::from_secs(86_400);
const BACKUP_CANCELLED_MSG: &str = "Backup cancelled by user.";

/// PBS 4.x: `version` subcommand (not `--version`).
async fn probe_pbs_binary(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    spawn_pbs_command(path)
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Builds a `Command` for the given PBS client binary.
/// On Windows, `.cmd`/`.bat` wrapper files are launched via `cmd /c` since
/// `CreateProcess` cannot execute them directly. CREATE_NO_WINDOW suppresses
/// the console popup that would otherwise appear for each PBS operation.
pub(crate) fn spawn_pbs_command(binary: &Path) -> Command {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let ext = binary
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let mut cmd = if ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat") {
            let mut c = Command::new("cmd");
            c.args([std::ffi::OsStr::new("/c"), binary.as_os_str()]);
            c
        } else {
            Command::new(binary)
        };
        cmd.creation_flags(CREATE_NO_WINDOW);
        return cmd;
    }
    #[allow(unreachable_code)]
    Command::new(binary)
}

async fn probe_flatpak_host_pbs() -> bool {
    Command::new("flatpak-spawn")
        .args(["--host", "proxmox-backup-client", "version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

static RUNNING_BACKUPS: OnceLock<Mutex<HashMap<i64, tokio::process::Child>>> = OnceLock::new();

fn running_backups() -> &'static Mutex<HashMap<i64, tokio::process::Child>> {
    RUNNING_BACKUPS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_running_backup(profile_id: i64, child: tokio::process::Child) {
    running_backups().lock().unwrap().insert(profile_id, child);
}

fn take_running_backup(profile_id: i64) -> Option<tokio::process::Child> {
    running_backups().lock().unwrap().remove(&profile_id)
}

/// Stops a running `proxmox-backup-client backup` for the given profile, if any.
///
/// Sends SIGTERM to the process group, then SIGKILL. The child stays registered until
/// the backup task exits and reaps it (avoids zombies).
pub fn cancel_running_backup(profile_id: i64) -> bool {
    let mut guard = running_backups().lock().unwrap();
    let Some(child) = guard.get_mut(&profile_id) else {
        return false;
    };
    #[cfg(unix)]
    signal_process_group(child, libc::SIGTERM);
    #[cfg(not(unix))]
    signal_process_group(child, 15);
    child.start_kill().is_ok()
}

/// Whether `proxmox-backup-client backup` is still running for this profile.
pub fn backup_process_running(profile_id: i64) -> bool {
    running_backups().lock().unwrap().contains_key(&profile_id)
}

/// Stops every running backup (e.g. when the network must be freed immediately).
pub fn cancel_all_running_backups() -> u32 {
    let ids: Vec<i64> = running_backups()
        .lock()
        .unwrap()
        .keys()
        .copied()
        .collect();
    let mut count = 0u32;
    for id in ids {
        if cancel_running_backup(id) {
            count += 1;
        }
    }
    count
}
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone)]
pub struct BackupResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub bytes_uploaded: u64,
}

pub struct PbsClient;

impl PbsClient {
    pub async fn is_available() -> bool {
        // pbs_client_path() returns the cached path (non-blocking) or the bare binary name.
        // The cache is pre-populated by init_pbs_client_path() at daemon startup.
        let binary = crate::paths::pbs_client_path().to_path_buf();

        // On Windows the cached path is the WSL wrapper script.  If it was deleted
        // (e.g. by wipe_all_local_data), regenerate it before probing.
        #[cfg(windows)]
        if !binary.exists() {
            let _ = crate::paths::ensure_wsl_pbs_wrapper();
        }

        if probe_pbs_binary(&binary).await {
            return true;
        }
        if std::env::var_os("FLATPAK_ID").is_some() {
            return probe_flatpak_host_pbs().await;
        }
        false
    }

    /// Verifies PBS is reachable and credentials work (same check as profile verification).
    pub async fn check_repository_accessible(
        repository: &str,
        namespace: Option<&str>,
        server_fingerprint: Option<&str>,
    ) -> std::result::Result<(), String> {
        let parts = PbsRepositoryParts::parse(repository).map_err(|e| {
            format!("invalid PBS connection settings: {e}")
        })?;

        let endpoint = parts.tcp_connect_address();

        if !tcp_reachable(&endpoint).await {
            return Err(format!(
                "cannot reach {endpoint} (network or firewall); PBS port is usually 8007"
            ));
        }

        // When the CLI binary is absent (e.g. Windows) verify via REST API.
        if !Self::is_available().await {
            return match crate::pbs_api::PbsApiClient::new(&parts, namespace, server_fingerprint) {
                Ok(api) => {
                    let r = api.verify_credentials().await;
                    if r.ok {
                        Ok(())
                    } else {
                        Err(r.message.unwrap_or_else(|| "authentication failed".into()))
                    }
                }
                Err(e) => Err(format!("PBS REST API setup failed: {e}")),
            };
        }

        let result = Self::verify_credentials(repository, namespace, server_fingerprint).await;
        if result.ok {
            return Ok(());
        }

        let detail = result
            .message
            .unwrap_or_else(|| "authentication failed".into());
        Err(format!(
            "PBS at {endpoint} rejected the connection: {detail}"
        ))
    }

    /// Authenticates against PBS via `proxmox-backup-client list` (PBS 4.x component flags).
    pub async fn verify_credentials(
        repository: &str,
        namespace: Option<&str>,
        server_fingerprint: Option<&str>,
    ) -> CredentialVerifyResult {
        let parts = match PbsRepositoryParts::parse(repository) {
            Ok(p) => p,
            Err(err) => {
                return CredentialVerifyResult {
                    ok: false,
                    message: Some(err.to_string()),
                };
            }
        };

        // When the CLI binary is absent (e.g. Windows) fall back to REST API.
        if !Self::is_available().await {
            return match crate::pbs_api::PbsApiClient::new(&parts, namespace, server_fingerprint) {
                Ok(api) => api.verify_credentials().await,
                Err(e) => CredentialVerifyResult {
                    ok: false,
                    message: Some(format!("PBS REST API setup failed: {e}")),
                },
            };
        }

        if let Err(message) = validate_pbs_client_params(&parts, namespace) {
            return CredentialVerifyResult {
                ok: false,
                message: Some(message),
            };
        }

        let password = parts.api_token_secret();
        let mut last_message = None;

        for (mode, repo_arg) in parts.repository_cli_candidates() {
            debug!(repository, mode, %repo_arg, "verifying pbs credentials (--repository)");

            let output = match run_pbs_list_repository(
                &parts,
                &repo_arg,
                namespace,
                &password,
                server_fingerprint,
            )
            .await
            {
                Ok(output) => output,
                Err(err) => {
                    return CredentialVerifyResult {
                        ok: false,
                        message: Some(err),
                    };
                }
            };

            if output.status.success() {
                return CredentialVerifyResult {
                    ok: true,
                    message: None,
                };
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            last_message = pbs_client_error_message(&stderr)
                .or_else(|| Some("pbs authentication failed".into()));
            debug!(mode, message = ?last_message, "pbs --repository auth failed");
        }

        for (mode, auth_id) in parts.auth_id_candidates() {
            debug!(repository, mode, %auth_id, "verifying pbs credentials (component flags)");

            let output = match run_pbs_list(
                &parts,
                namespace,
                &auth_id,
                &password,
                server_fingerprint,
            )
            .await
            {
                Ok(output) => output,
                Err(err) => {
                    return CredentialVerifyResult {
                        ok: false,
                        message: Some(err),
                    };
                }
            };

            if output.status.success() {
                return CredentialVerifyResult {
                    ok: true,
                    message: None,
                };
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            last_message = pbs_client_error_message(&stderr)
                .or_else(|| Some("pbs authentication failed".into()));
            debug!(mode, message = ?last_message, "pbs auth attempt failed");
        }

        warn!(message = ?last_message, "pbs credential verification failed");
        CredentialVerifyResult {
            ok: false,
            message: Some(last_message.unwrap_or_else(|| "pbs authentication failed".into())),
        }
    }

    pub fn write_profile_config(profile: &BackupProfile) -> Result<()> {
        let dir = runtime_profile_config_dir(profile.id);
        std::fs::create_dir_all(&dir).map_err(CoreError::Io)?;

        let parts = PbsRepositoryParts::parse(&profile.repository)
            .map_err(|e| CoreError::PbsCommand(e.to_string()))?;
        let auth_id = parts
            .pbs_auth_id()
            .ok_or_else(|| CoreError::PbsCommand("missing API token".into()))?;
        let repository = parts.pbs_repository_bash_style(&auth_id);

        let mut config = serde_json::json!({
            "repository": repository,
            "backup-id": profile.backup_id,
        });
        if let Some(ns) = &profile.namespace {
            config["namespace"] = serde_json::Value::String(ns.clone());
        }

        let path = profile_backup_client_config(profile.id);
        let body = serde_json::to_string_pretty(&config)?;
        std::fs::write(&path, body).map_err(CoreError::Io)?;
        Ok(())
    }

    pub async fn run_backup(profile: &BackupProfile) -> Result<BackupResult> {
        Self::run_backup_inner(profile, |_| {}).await
    }

    /// Runs a backup and calls `on_progress` with parsed status lines from PBS stderr.
    pub async fn run_backup_with_progress<F>(profile: &BackupProfile, on_progress: F) -> Result<BackupResult>
    where
        F: FnMut(String) + Send,
    {
        Self::run_backup_inner(profile, on_progress).await
    }

    async fn run_backup_inner<F>(profile: &BackupProfile, mut on_progress: F) -> Result<BackupResult>
    where
        F: FnMut(String) + Send,
    {
        if !Self::is_available().await {
            return Err(CoreError::PbsClientMissing);
        }

        let parts = PbsRepositoryParts::parse(&profile.repository)
            .map_err(|e| CoreError::PbsCommand(e.to_string()))?;
        let auth_id = parts
            .pbs_auth_id()
            .ok_or_else(|| CoreError::PbsCommand("missing API token".into()))?;
        let repo_arg = parts.pbs_repository_bash_style(&auth_id);
        let password = parts.api_token_secret();

        let mut cmd = spawn_pbs_command(crate::paths::pbs_client_path());
        apply_pbs_client_env(
            &mut cmd,
            &parts,
            &repo_arg,
            &password,
            profile.server_fingerprint.as_deref(),
        );
        cmd.arg("backup")
            .arg(format!("--repository={repo_arg}"))
            .arg(format!("--backup-id={}", profile.backup_id));
        if let Some(ns) = &profile.namespace {
            if !ns.is_empty() {
                cmd.arg(format!("--ns={ns}"));
            }
        }

        for (index, path) in profile.paths.iter().enumerate() {
            cmd.arg(path_to_backupspec(path, index));
        }

        for exclude in &profile.excludes {
            cmd.arg(format!("--exclude={exclude}"));
        }

        if let Some(kib_s) = profile.conditions.bandwidth_limit_kib_s.filter(|v| *v > 0) {
            cmd.arg(format!("--rate={kib_s}KiB/s"));
        }

        apply_encryption_to_command(&mut cmd, profile.encryption_key_id, EncryptionCliMode::Backup)?;

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        #[cfg(unix)]
        {
            cmd.process_group(0);
        }

        debug!(profile_id = profile.id, "starting proxmox-backup-client backup");

        let mut child = cmd.spawn().map_err(CoreError::Io)?;
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        register_running_backup(profile.id, child);

        let stderr_acc = if let Some(stderr) = stderr_pipe {
            read_backup_stderr(stderr, &mut on_progress).await
        } else {
            String::new()
        };

        if let Some(stdout) = stdout_pipe {
            let _ = read_to_string_lossy(stdout).await;
        }

        let Some(mut child) = take_running_backup(profile.id) else {
            return Ok(BackupResult {
                exit_code: 130,
                stdout: String::new(),
                stderr: BACKUP_CANCELLED_MSG.into(),
                bytes_uploaded: 0,
            });
        };

        let status = timeout(BACKUP_TIMEOUT, child.wait())
            .await
            .map_err(|_| CoreError::PbsCommand("backup timed out".into()))?
            .map_err(CoreError::Io)?;

        let exit_code = status.code().unwrap_or(-1);
        let stdout = String::new();

        let cancelled = crate::health::is_backup_cancelled(&stderr_acc, exit_code);

        if !status.success() && !cancelled {
            warn!(
                profile_id = profile.id,
                exit_code,
                stderr = %stderr_acc,
                "pbs backup failed"
            );
        }

        Ok(BackupResult {
            exit_code,
            stdout,
            stderr: stderr_acc.clone(),
            bytes_uploaded: parse_bytes_uploaded(&stderr_acc),
        })
    }

    pub fn config_path_for_profile(profile_id: i64) -> std::path::PathBuf {
        profile_backup_client_config(profile_id)
    }

    pub fn config_exists(path: &Path) -> bool {
        path.is_file()
    }
}

/// PBS archive name for a backup path (same rules as [`path_to_backupspec`]).
pub fn backup_archive_name(path: &str, index: usize) -> String {
    let fallback = format!("path{index}");
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .map(str::to_string)
        .unwrap_or(fallback);
    format!("{name}.pxar")
}

/// Builds `name.pxar:/path` for proxmox-backup-client 4.x.
fn path_to_backupspec(path: &str, index: usize) -> String {
    format!("{}:{path}", backup_archive_name(path, index))
}

/// Validates fields before calling `proxmox-backup-client` (avoids cryptic CLI errors).
fn validate_pbs_client_params(
    parts: &PbsRepositoryParts,
    namespace: Option<&str>,
) -> std::result::Result<(), String> {
    if parts.datastore.len() < 3 {
        return Err(
            "datastore name must be at least 3 characters (PBS requirement)".to_string(),
        );
    }

    if parts.user.is_empty() {
        return Err("PBS username is required (e.g. backup@pbs)".to_string());
    }

    if parts.api_token_secret().is_empty() {
        return Err("API token secret is required (UUID from PBS)".to_string());
    }

    if parts.pbs_auth_id().is_none() {
        return Err(
            "could not build PBS authentication from profile: use the API token PBS shows once \
             (e.g. 153-Test@pbs!uuid) or tokenname=uuid"
                .to_string(),
        );
    }

    for (_, auth_id) in parts.auth_id_candidates() {
        if !is_valid_pbs_auth_id(&auth_id) {
            return Err(format!(
                "invalid PBS credentials: auth-id is too long or malformed (max {PBS_AUTH_ID_MAX_LEN} characters). \
                 Shorten the PBS username or token name."
            ));
        }
    }

    if let Some(ns) = namespace.filter(|s| !s.is_empty()) {
        if ns.starts_with('/') || ns.ends_with('/') {
            return Err(
                "namespace must not start or end with '/' (or leave the field empty)"
                    .to_string(),
            );
        }
    }

    Ok(())
}

/// Formats a PBS server TLS fingerprint for `PBS_FINGERPRINT` (uppercase hex pairs with colons).
pub fn format_server_fingerprint_for_env(value: &str) -> Option<String> {
    let normalized = normalize_fingerprint(value);
    if normalized.is_empty() {
        return None;
    }
    Some(
        normalized
            .as_bytes()
            .chunks(2)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or("").to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join(":"),
    )
}

/// Sets the same `PBS_*` variables as the OneSystems `backup.sh` client wrapper.
pub(crate) fn apply_pbs_client_env(
    cmd: &mut Command,
    parts: &PbsRepositoryParts,
    repository_arg: &str,
    password: &str,
    server_fingerprint: Option<&str>,
) {
    let (server, port) = parts.server_and_port();
    cmd.env("PBS_PASSWORD", password)
        .env("PBS_REPOSITORY", repository_arg)
        .env("PBS_HOST", &server)
        .env("PBS_PORT", port.to_string())
        .env("PBS_DATASTORE", &parts.datastore);
    if let Some(fp) = server_fingerprint.and_then(format_server_fingerprint_for_env) {
        cmd.env("PBS_FINGERPRINT", fp);
    }
}

async fn run_pbs_list_repository(
    parts: &PbsRepositoryParts,
    repository_arg: &str,
    namespace: Option<&str>,
    password: &str,
    server_fingerprint: Option<&str>,
) -> std::result::Result<std::process::Output, String> {
    let mut cmd = spawn_pbs_command(crate::paths::pbs_client_path());
    apply_pbs_client_env(
        &mut cmd,
        parts,
        repository_arg,
        password,
        server_fingerprint,
    );
    // PBS_REPOSITORY is already set by apply_pbs_client_env; do not pass --repository flag
    // because PBS client 3.x rejects the `user@host:port:store` format as a CLI argument
    // (the same value is accepted via env var).
    cmd.arg("list");
    if let Some(ns) = namespace.filter(|s| !s.is_empty()) {
        cmd.arg(format!("--ns={ns}"));
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    timeout(VERIFY_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "credential check timed out".to_string())?
        .map_err(|e| e.to_string())
}

async fn run_pbs_list(
    parts: &PbsRepositoryParts,
    namespace: Option<&str>,
    auth_id: &str,
    password: &str,
    server_fingerprint: Option<&str>,
) -> std::result::Result<std::process::Output, String> {
    let mut cmd = spawn_pbs_command(crate::paths::pbs_client_path());
    apply_pbs_client_env(
        &mut cmd,
        parts,
        &parts.pbs_repository_bash_style(auth_id),
        password,
        server_fingerprint,
    );
    // Component flags (--auth-id, --server, --datastore) were added in PBS client 4.x.
    // PBS_REPOSITORY is already set by apply_pbs_client_env and works on all versions.
    cmd.arg("list");
    if let Some(ns) = namespace.filter(|s| !s.is_empty()) {
        cmd.arg(format!("--ns={ns}"));
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    timeout(VERIFY_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "credential check timed out".to_string())?
        .map_err(|e| e.to_string())
}

/// Matches proxmox-backup-client `auth-id` parameter rules (approximate).
fn is_valid_pbs_auth_id(auth_id: &str) -> bool {
    !auth_id.is_empty()
        && auth_id.len() <= PBS_AUTH_ID_MAX_LEN
        && auth_id.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '@' | '!' | '.' | '_' | '-')
        })
}

fn pbs_client_error_message(stderr: &str) -> Option<String> {
    let mut parts = Vec::new();

    for line in stderr.lines() {
        let line = line.trim();
        if line.starts_with("Usage:") {
            break;
        }
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Error:") {
            parts.push(rest.trim().to_string());
        } else if line.starts_with("- '") {
            parts.push(line.to_string());
        } else if line.starts_with("Caused by:") {
            parts.push(line.to_string());
        }
    }

    if parts.is_empty() {
        let trimmed = stderr.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.lines().next()?.trim().to_string())
        }
    } else {
        Some(parts.join(" — "))
    }
}

async fn read_backup_stderr<F>(
    stderr: impl tokio::io::AsyncRead + Unpin,
    on_progress: &mut F,
) -> String
where
    F: FnMut(String) + Send,
{
    let mut reader = BufReader::new(stderr).lines();
    let mut acc = String::new();
    while let Ok(Some(line)) = reader.next_line().await {
        acc.push_str(&line);
        acc.push('\n');
        if let Some(message) = parse_pbs_progress_line(&line) {
            on_progress(message);
        }
    }
    acc
}

async fn read_to_string_lossy(mut reader: impl tokio::io::AsyncRead + Unpin) -> String {
    let mut buf = Vec::new();
    let _ = tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut buf).await;
    String::from_utf8_lossy(&buf).into_owned()
}

/// Best-effort PBS snapshot id from backup stderr (`…/backup_id/2026-05-21T12:00:00Z`).
pub fn parse_snapshot_id_from_stderr(stderr: &str, backup_id: &str) -> Option<String> {
    let backup_id = backup_id.trim();
    if backup_id.is_empty() {
        return None;
    }
    let mut best: Option<String> = None;
    for line in stderr.lines() {
        let line = line.trim();
        if !line.contains(backup_id) {
            continue;
        }
        for segment in line.split(|c: char| c == '/' || c == ' ' || c == ':' || c == '[') {
            if looks_like_snapshot_timestamp(segment) {
                best = Some(normalize_snapshot_timestamp(segment));
            }
        }
    }
    best
}

fn looks_like_snapshot_timestamp(segment: &str) -> bool {
    let segment = segment.trim().trim_end_matches('Z');
    if segment.len() < 19 {
        return false;
    }
    let bytes = segment.as_bytes();
    bytes.len() >= 19
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && segment[..4].chars().all(|c| c.is_ascii_digit())
}

fn normalize_snapshot_timestamp(segment: &str) -> String {
    let segment = segment.trim();
    if segment.ends_with('Z') {
        segment.to_string()
    } else {
        format!("{segment}Z")
    }
}

/// Best-effort upload size from PBS stderr (`uploaded 2.439 GiB`).
pub fn parse_bytes_uploaded(stderr: &str) -> u64 {
    let mut best = 0u64;
    for line in stderr.lines() {
        if let Some(bytes) = parse_uploaded_bytes_line(line) {
            best = best.max(bytes);
        }
    }
    best
}

fn parse_uploaded_bytes_line(line: &str) -> Option<u64> {
    let lower = line.to_lowercase();
    if !lower.contains("upload") {
        return None;
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        let w = word.to_lowercase();
        if w != "uploaded" && !w.ends_with("uploaded") {
            continue;
        }
        if let Some((value, unit)) = words.get(i + 1).zip(words.get(i + 2)) {
            let num: f64 = value.parse().ok()?;
            let unit = unit.to_lowercase();
            let bytes = if unit.starts_with("gib") || unit.starts_with("gb") {
                (num * 1024.0 * 1024.0 * 1024.0) as u64
            } else if unit.starts_with("mib") || unit.starts_with("mb") {
                (num * 1024.0 * 1024.0) as u64
            } else if unit.starts_with("kib") || unit.starts_with("kb") {
                (num * 1024.0) as u64
            } else if unit.starts_with('b') {
                num as u64
            } else {
                continue;
            };
            return Some(bytes);
        }
    }
    None
}

/// Extracts human-readable progress from PBS stderr (`INFO: processed … uploaded …`).
pub fn parse_pbs_progress_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let payload = line
        .rsplit_once(": ")
        .map(|(_, rest)| rest.trim())
        .unwrap_or(line);

    let lower = payload.to_lowercase();
    if lower.contains("processed") && lower.contains("upload") {
        return Some(payload.to_string());
    }
    if lower.contains("creating backup")
        || lower.contains("create new backup")
        || lower.contains("uploading")
        || lower.contains("backup snapshot")
    {
        return Some(payload.to_string());
    }

    None
}

#[cfg(test)]
mod progress_tests {
    use super::{format_server_fingerprint_for_env, parse_pbs_progress_line};

    #[test]
    fn formats_server_fingerprint_for_env() {
        assert_eq!(
            format_server_fingerprint_for_env("aa:bb:cc:dd"),
            Some("AA:BB:CC:DD".into())
        );
        assert_eq!(
            format_server_fingerprint_for_env("  AABBCCDD  "),
            Some("AA:BB:CC:DD".into())
        );
        assert_eq!(format_server_fingerprint_for_env(""), None);
        assert_eq!(format_server_fingerprint_for_env("   "), None);
    }

    #[test]
    fn parses_processed_line() {
        let line = "INFO: processed 2.471 GiB in 1min, uploaded 2.439 GiB";
        let msg = parse_pbs_progress_line(line).unwrap();
        assert!(msg.contains("processed"));
        assert!(msg.contains("uploaded"));
    }
}

#[cfg(unix)]
fn signal_process_group(child: &tokio::process::Child, sig: i32) {
    let Some(pid) = child.id() else {
        return;
    };
    let pgid = -(pid as i32);
    unsafe {
        let _ = libc::kill(pgid, sig);
    }
}

#[cfg(not(unix))]
fn signal_process_group(_child: &tokio::process::Child, _sig: i32) {}

async fn tcp_reachable(addr: &str) -> bool {
    debug!(%addr, "pbs tcp reachability check");
    matches!(
        timeout(TCP_CONNECT_TIMEOUT, tokio::net::TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

