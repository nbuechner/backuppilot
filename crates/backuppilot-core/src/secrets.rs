//! API token and encryption password storage.
//!
//! Storage strategy (both platforms):
//!   Primary   – platform keychain (Windows Credential Manager / freedesktop Secret Service)
//!   Fallback  – plain file under `config_dir()`, mode 0600 on Unix
//!
//! The file fallback is intentional: the daemon runs in a background systemd
//! session where the keyring is often locked right after login, so backup jobs
//! would fail without it. On Windows the file is also kept as a fallback for
//! services running as SYSTEM (different credential store from the user).

use tracing::warn;

use crate::error::{CoreError, Result};
use crate::ids::APP_ID;
use crate::paths::{config_dir, ensure_data_dirs};
use crate::pbs_repository::{encode_repository, PbsRepositoryParts};

// ── Path helpers ─────────────────────────────────────────────────────────────

fn fallback_token_path(profile_id: i64) -> std::path::PathBuf {
    config_dir().join("tokens").join(format!("{profile_id}.secret"))
}

fn fallback_encryption_password_path(key_id: i64) -> std::path::PathBuf {
    config_dir()
        .join("encryption-passwords")
        .join(format!("{key_id}.secret"))
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Stores the API token secret and returns repository JSON without the secret.
pub fn persist_profile_credentials(
    profile_id: i64,
    parts: &PbsRepositoryParts,
) -> Result<String> {
    let token = parts.api_token_secret();
    if token.is_empty() {
        return Err(CoreError::PbsCommand("API token secret is required".into()));
    }
    store_api_token(profile_id, &token)?;
    let mut stored = parts.clone();
    // Keep the token name in the DB so auth-id can be reconstructed on load.
    let (token_id, _) = parts.api_token_parts();
    stored.token = token_id; // e.g. "win-test" (name only, secret is in keyring)
    Ok(encode_repository(&stored))
}

/// Merges a stored repository string with the token from the credential store.
pub fn hydrate_profile_repository(profile_id: i64, repository: &str) -> Result<String> {
    let mut parts = PbsRepositoryParts::parse(repository)
        .map_err(|e| CoreError::PbsCommand(e.to_string()))?;

    if parts.api_token_secret().is_empty() {
        if let Some(secret) = load_api_token(profile_id)? {
            // Reconstruct "name=secret" so pbs_auth_id() can recover the token name.
            let (token_id, _) = parts.api_token_parts();
            parts.token = if token_id.is_empty() {
                secret
            } else {
                format!("{token_id}={secret}")
            };
        }
    }

    let secret = parts.api_token_secret();
    if !secret.is_empty() {
        ensure_fallback_token_file(profile_id, &secret);
    }

    Ok(encode_repository(&parts))
}

/// Ensures a file copy of the token exists (needed for the background daemon).
fn ensure_fallback_token_file(profile_id: i64, token: &str) {
    let path = fallback_token_path(profile_id);
    if path.is_file() {
        return;
    }
    let _ = write_fallback_token(profile_id, token);
}

/// Returns the stored API token secret for profile editing.
pub fn load_stored_api_token(profile_id: i64) -> Option<String> {
    load_api_token(profile_id)
        .ok()
        .flatten()
        .filter(|t| !t.trim().is_empty())
}

/// Whether a token exists for this profile (keyring or file fallback).
pub fn has_api_token(profile_id: i64) -> bool {
    load_api_token(profile_id)
        .ok()
        .flatten()
        .is_some_and(|t| !t.trim().is_empty())
}

pub fn delete_api_token(profile_id: i64) -> Result<()> {
    let _ = keyring_delete(&token_account(profile_id));
    let path = fallback_token_path(profile_id);
    if path.is_file() {
        std::fs::remove_file(path).map_err(CoreError::Io)?;
    }
    Ok(())
}

pub fn store_encryption_key_password(key_id: i64, password: &str) -> Result<()> {
    let keyring_ok = keyring_store(&enc_key_account(key_id), password).is_ok();
    write_fallback_encryption_password(key_id, password)?;
    if !keyring_ok {
        warn!(key_id, "using file fallback for encryption password (keyring unavailable)");
    }
    Ok(())
}

pub fn load_encryption_key_password(key_id: i64) -> Result<Option<String>> {
    if let Some(pw) = keyring_load(&enc_key_account(key_id)) {
        let pw = pw.trim().to_string();
        if !pw.is_empty() {
            return Ok(Some(pw));
        }
    }
    let path = fallback_encryption_password_path(key_id);
    if path.is_file() {
        let pw = std::fs::read_to_string(&path).map_err(CoreError::Io)?;
        let pw = pw.trim().to_string();
        if !pw.is_empty() {
            return Ok(Some(pw));
        }
    }
    Ok(None)
}

pub fn has_encryption_key_password(key_id: i64) -> bool {
    load_encryption_key_password(key_id)
        .ok()
        .flatten()
        .is_some_and(|p| !p.is_empty())
}

pub fn delete_encryption_key_password(key_id: i64) -> Result<()> {
    let _ = keyring_delete(&enc_key_account(key_id));
    let path = fallback_encryption_password_path(key_id);
    if path.is_file() {
        std::fs::remove_file(path).map_err(CoreError::Io)?;
    }
    Ok(())
}

/// Migrates tokens still embedded in repository JSON into the credential store.
pub fn migrate_repository_tokens(profile_id: i64, repository: &str) -> Result<String> {
    let parts = PbsRepositoryParts::parse(repository)
        .map_err(|e| CoreError::PbsCommand(e.to_string()))?;
    if parts.api_token_secret().is_empty() {
        return Ok(repository.to_string());
    }
    warn!(profile_id, "migrating API token from database to credential store");
    persist_profile_credentials(profile_id, &parts)
}

// ── Internal token helpers ────────────────────────────────────────────────────

fn store_api_token(profile_id: i64, token: &str) -> Result<()> {
    let keyring_ok = keyring_store(&token_account(profile_id), token).is_ok();
    // Always keep a file copy so the background daemon can read it when the
    // keyring is locked (common right after login under systemd on Linux, and
    // for Windows services running as SYSTEM).
    write_fallback_token(profile_id, token)?;
    if !keyring_ok {
        warn!(profile_id, "using file fallback for API token (keyring unavailable)");
    }
    Ok(())
}

fn load_api_token(profile_id: i64) -> Result<Option<String>> {
    if let Some(token) = keyring_load(&token_account(profile_id)) {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(Some(token));
        }
    }
    let path = fallback_token_path(profile_id);
    if path.is_file() {
        let token = std::fs::read_to_string(&path).map_err(CoreError::Io)?;
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(Some(token));
        }
    }
    Ok(None)
}

// ── Keyring account name helpers ──────────────────────────────────────────────

fn token_account(profile_id: i64) -> String {
    format!("profile-{profile_id}")
}

fn enc_key_account(key_id: i64) -> String {
    format!("encryption-key-{key_id}")
}

// ── Platform keyring implementations ─────────────────────────────────────────

// ·· Windows: Windows Credential Manager via the `keyring` crate ··············

#[cfg(windows)]
fn keyring_store(account: &str, secret: &str) -> Result<()> {
    keyring::Entry::new(APP_ID, account)
        .and_then(|e| e.set_password(secret))
        .map_err(|e| CoreError::PbsCommand(format!("Windows Credential Manager store failed: {e}")))
}

#[cfg(windows)]
fn keyring_load(account: &str) -> Option<String> {
    keyring::Entry::new(APP_ID, account)
        .and_then(|e| e.get_password())
        .ok()
}

#[cfg(windows)]
fn keyring_delete(account: &str) -> Result<()> {
    keyring::Entry::new(APP_ID, account)
        .and_then(|e| e.delete_credential())
        .map_err(|e| CoreError::PbsCommand(format!("Windows Credential Manager delete failed: {e}")))
}

// ·· Unix: freedesktop Secret Service via `secret-tool` subprocess ············

#[cfg(unix)]
fn keyring_store(account: &str, secret: &str) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label",
            &format!("{APP_ID} {account}"),
            "xdg:service",
            APP_ID,
            "xdg:account",
            account,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::PbsCommand(format!("secret-tool not available: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(secret.as_bytes()).map_err(CoreError::Io)?;
    }
    let status = child.wait().map_err(CoreError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(CoreError::PbsCommand(
            "secret-tool store failed (is libsecret installed?)".into(),
        ))
    }
}

#[cfg(unix)]
fn keyring_load(account: &str) -> Option<String> {
    use std::process::Command;
    let output = Command::new("secret-tool")
        .args(["lookup", "xdg:service", APP_ID, "xdg:account", account])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

#[cfg(unix)]
fn keyring_delete(account: &str) -> Result<()> {
    use std::process::Command;
    let status = Command::new("secret-tool")
        .args(["clear", "xdg:service", APP_ID, "xdg:account", account])
        .status()
        .map_err(CoreError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(CoreError::PbsCommand("secret-tool clear failed".into()))
    }
}

// ── Fallback file helpers ─────────────────────────────────────────────────────

fn write_fallback_token(profile_id: i64, token: &str) -> Result<()> {
    ensure_data_dirs().map_err(CoreError::Io)?;
    let dir = config_dir().join("tokens");
    std::fs::create_dir_all(&dir).map_err(CoreError::Io)?;
    let path = fallback_token_path(profile_id);
    std::fs::write(&path, token).map_err(CoreError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn write_fallback_encryption_password(key_id: i64, password: &str) -> Result<()> {
    ensure_data_dirs().map_err(CoreError::Io)?;
    let dir = config_dir().join("encryption-passwords");
    std::fs::create_dir_all(&dir).map_err(CoreError::Io)?;
    let path = fallback_encryption_password_path(key_id);
    std::fs::write(&path, password).map_err(CoreError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
