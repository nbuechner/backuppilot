//! PBS backup encryption keys (create, import, storage under the app data directory).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use tokio::process::Command;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::error::{CoreError, Result};
use crate::paths::{data_dir, ensure_data_dirs, pbs_client_path};
use crate::secrets::{
    delete_encryption_key_password, has_encryption_key_password, load_encryption_key_password,
    store_encryption_key_password,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionKey {
    pub id: i64,
    pub name: String,
    /// Relative path under [`data_dir()`], e.g. `encryption-keys/1.json`.
    pub key_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hint: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_exported_at: Option<DateTime<Utc>>,
    /// PBS key fingerprint (`aa:bb:…`), used to match encrypted snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// Profile names that currently use this key (legacy; prefer [`Self::profile_usage`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles_using: Vec<String>,
    /// Per-profile usage from PBS snapshot fingerprints and profile assignment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile_usage: Vec<EncryptionKeyProfileUsage>,
    /// Set when serializing for the GUI (password is not sent over D-Bus).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub password_configured: bool,
    /// Assigned to a profile and/or referenced by encrypted snapshots on PBS.
    #[serde(default)]
    pub in_use: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionKeyProfileUsage {
    pub profile_name: String,
    /// Profile has this key selected for new backups.
    pub assigned: bool,
    /// Encrypted snapshots on PBS that match this key's fingerprint.
    pub encrypted_snapshots: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEncryptionKeyInput {
    pub name: String,
    pub password: String,
    #[serde(default)]
    pub password_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportEncryptionKeyInput {
    pub name: String,
    pub source_path: String,
    pub password: String,
    #[serde(default)]
    pub password_hint: Option<String>,
}

pub fn encryption_keys_dir() -> PathBuf {
    data_dir().join("encryption-keys")
}

pub fn key_absolute_path(key_file: &str) -> PathBuf {
    if Path::new(key_file).is_absolute() {
        key_file.into()
    } else {
        data_dir().join(key_file)
    }
}

pub fn redact_encryption_key(mut key: EncryptionKey) -> EncryptionKey {
    key.password_configured = has_encryption_key_password(key.id);
    key
}

/// Creates a new PBS encryption key file and stores its password.
pub fn create_encryption_key_record(
    id: i64,
    input: &CreateEncryptionKeyInput,
) -> Result<EncryptionKey> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(CoreError::PbsCommand("key name is required".into()));
    }
    if input.password.len() < 8 {
        return Err(CoreError::PbsCommand(
            "encryption password must be at least 8 characters".into(),
        ));
    }

    ensure_data_dirs().map_err(CoreError::Io)?;
    std::fs::create_dir_all(encryption_keys_dir()).map_err(CoreError::Io)?;

    let key_file = format!("encryption-keys/{id}.json");
    let abs = key_absolute_path(&key_file);
    if abs.is_file() {
        return Err(CoreError::PbsCommand("encryption key file already exists".into()));
    }

    run_key_create(&abs, &input.password, input.password_hint.as_deref())?;
    store_encryption_key_password(id, &input.password)?;

    Ok(EncryptionKey {
        id,
        name: name.to_string(),
        key_file,
        password_hint: input.password_hint.clone(),
        created_at: Utc::now(),
        last_exported_at: None,
        fingerprint: None,
        profiles_using: Vec::new(),
        profile_usage: Vec::new(),
        password_configured: true,
        in_use: false,
    })
}

/// Copies an existing PBS key JSON into BackupPilot storage.
pub fn import_encryption_key_record(
    id: i64,
    input: &ImportEncryptionKeyInput,
) -> Result<EncryptionKey> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(CoreError::PbsCommand("key name is required".into()));
    }
    let source = PathBuf::from(input.source_path.trim());
    if !source.is_file() {
        return Err(CoreError::PbsCommand(format!(
            "key file not found: {}",
            source.display()
        )));
    }

    ensure_data_dirs().map_err(CoreError::Io)?;
    std::fs::create_dir_all(encryption_keys_dir()).map_err(CoreError::Io)?;

    let key_file = format!("encryption-keys/{id}.json");
    let abs = key_absolute_path(&key_file);

    let verify_copy = std::env::temp_dir().join(format!("backuppilot-key-verify-{id}.json"));
    std::fs::copy(&source, &verify_copy).map_err(CoreError::Io)?;
    let verify_result = verify_key_unlocks(&verify_copy, &input.password);
    let _ = std::fs::remove_file(&verify_copy);
    verify_result?;

    std::fs::copy(&source, &abs).map_err(CoreError::Io)?;

    store_encryption_key_password(id, &input.password)?;

    Ok(EncryptionKey {
        id,
        name: name.to_string(),
        key_file,
        password_hint: input.password_hint.clone(),
        created_at: Utc::now(),
        last_exported_at: None,
        fingerprint: None,
        profiles_using: Vec::new(),
        profile_usage: Vec::new(),
        password_configured: true,
        in_use: false,
    })
}

/// True when the key is assigned to a profile or used by encrypted snapshots.
pub fn encryption_key_in_use(key: &EncryptionKey) -> bool {
    key.in_use
}

pub fn delete_encryption_key_files(key: &EncryptionKey) -> Result<()> {
    let _ = delete_encryption_key_password(key.id);
    let path = key_absolute_path(&key.key_file);
    if path.is_file() {
        std::fs::remove_file(path).map_err(CoreError::Io)?;
    }
    Ok(())
}

pub fn export_encryption_key_copy(key: &EncryptionKey, target: &Path) -> Result<()> {
    let src = key_absolute_path(&key.key_file);
    if !src.is_file() {
        return Err(CoreError::PbsCommand("encryption key file is missing".into()));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(CoreError::Io)?;
    }
    std::fs::copy(&src, target).map_err(CoreError::Io)?;
    Ok(())
}

/// How PBS encryption flags are passed for a given subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionCliMode {
    /// `backup`: `--keyfile` and `--crypt-mode=encrypt`.
    Backup,
    /// `restore`, `catalog dump`, …: `--keyfile` only (no `--crypt-mode`).
    Decrypt,
    /// `snapshot list`, `snapshot files`, …: no encryption CLI flags.
    None,
}

/// Applies encryption options when a profile uses a key.
pub fn apply_encryption_to_command(
    cmd: &mut Command,
    key_id: Option<i64>,
    mode: EncryptionCliMode,
) -> Result<()> {
    let Some(key_id) = key_id else {
        return Ok(());
    };
    if mode == EncryptionCliMode::None {
        return Ok(());
    }
    let password = load_encryption_key_password(key_id)?
        .ok_or_else(|| CoreError::PbsCommand("encryption password not stored for this key".into()))?;
    let key_path = key_absolute_path(&format!("encryption-keys/{key_id}.json"));
    if !key_path.is_file() {
        return Err(CoreError::PbsCommand(format!(
            "encryption key file missing: {}",
            key_path.display()
        )));
    }
    cmd.arg(format!("--keyfile={}", key_path.display()))
        .env("PBS_ENCRYPTION_PASSWORD", password);
    if mode == EncryptionCliMode::Backup {
        cmd.arg("--crypt-mode=encrypt");
    }
    Ok(())
}

/// Normalizes PBS fingerprints for comparison (`aa:bb:cc` vs `aabbcc`).
pub fn normalize_fingerprint(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase()
}

pub fn fingerprints_match(a: &str, b: &str) -> bool {
    !a.is_empty() && normalize_fingerprint(a) == normalize_fingerprint(b)
}

/// Reads the encryption key fingerprint via `proxmox-backup-client key show`.
pub fn read_key_fingerprint(path: &Path, password: &str) -> Result<String> {
    let output = StdCommand::new(pbs_client_path())
        .arg("key")
        .arg("show")
        .arg(path)
        .arg("--output-format=json")
        .env("PBS_ENCRYPTION_PASSWORD", password)
        .output()
        .map_err(CoreError::Io)?;
    if output.status.success() {
        if let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) {
            if let Some(fp) = value
                .get("fingerprint")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
            {
                return Ok(normalize_fingerprint(fp));
            }
        }
        if let Some(fp) = parse_fingerprint_from_key_show_text(&String::from_utf8_lossy(&output.stdout))
        {
            return Ok(fp);
        }
    }
    let output = StdCommand::new(pbs_client_path())
        .arg("key")
        .arg("show")
        .arg(path)
        .env("PBS_ENCRYPTION_PASSWORD", password)
        .output()
        .map_err(CoreError::Io)?;
    if output.status.success() {
        if let Some(fp) = parse_fingerprint_from_key_show_text(&String::from_utf8_lossy(&output.stdout))
        {
            return Ok(fp);
        }
    }
    Err(CoreError::PbsCommand(
        String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_string()
            .if_empty_then("could not read encryption key fingerprint"),
    ))
}

fn parse_fingerprint_from_key_show_text(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let rest = line
            .strip_prefix("Fingerprint:")
            .or_else(|| line.strip_prefix("Fingerprint"))?;
        let fp = rest.trim().trim_start_matches(':').trim();
        if !fp.is_empty() && !fp.eq_ignore_ascii_case("none") {
            return Some(normalize_fingerprint(fp));
        }
    }
    None
}

/// Returns false for plaintext PBS keys (`"kdf": null`).
fn encryption_key_requires_password(path: &Path) -> Result<bool> {
    let raw = std::fs::read_to_string(path).map_err(CoreError::Io)?;
    let value: Value = serde_json::from_str(&raw).map_err(|e| {
        CoreError::PbsCommand(format!("invalid encryption key JSON: {e}"))
    })?;
    Ok(!matches!(value.get("kdf"), None | Some(Value::Null)))
}

fn verify_key_unlocks(path: &Path, password: &str) -> Result<()> {
    if !encryption_key_requires_password(path)? {
        return Ok(());
    }
    if password.is_empty() {
        return Err(CoreError::PbsCommand(
            "encryption password is required for this key".into(),
        ));
    }
    verify_key_passphrase_via_pbs(path, password)
}

/// `key show` does not validate the password; `change-passphrase` decrypts the key first.
fn verify_key_passphrase_via_pbs(path: &Path, password: &str) -> Result<()> {
    let pbs = pbs_client_path();
    let cmd = format!(
        "{} key change-passphrase {}",
        shell_escape_single(&pbs.display().to_string()),
        shell_escape_single(&path.display().to_string()),
    );
    let out = run_script_with_password_lines(&cmd, &[password, password, password])?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    if combined.contains("Unable to decrypt key") || combined.contains("wrong password") {
        return Err(CoreError::PbsCommand(
            "encryption password is incorrect".into(),
        ));
    }
    if !combined.contains("New Password:") {
        let detail = extract_pbs_error_line(&combined)
            .if_empty_then("could not verify encryption password");
        return Err(CoreError::PbsCommand(detail));
    }
    Ok(())
}

fn extract_pbs_error_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| line.starts_with("Error:"))
        .map(|line| line.trim_start_matches("Error:").trim().to_string())
        .unwrap_or_default()
}

fn run_script_with_password_lines(cmd: &str, lines: &[&str]) -> Result<std::process::Output> {
    let mut child = StdCommand::new("script")
        .arg("-qfc")
        .arg(cmd)
        .arg("/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(CoreError::Io)?;
    if let Some(mut stdin) = child.stdin.take() {
        for line in lines {
            writeln!(stdin, "{line}").map_err(CoreError::Io)?;
        }
    }
    child.wait_with_output().map_err(CoreError::Io)
}

fn run_key_create(path: &Path, password: &str, hint: Option<&str>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(CoreError::Io)?;
    }
    if path.is_file() {
        std::fs::remove_file(path).map_err(CoreError::Io)?;
    }

    let pbs = pbs_client_path();
    let pbs_display = pbs.display().to_string();
    let mut create_cmd = format!(
        "{} key create {}",
        shell_escape_single(&pbs_display),
        shell_escape_single(&path.display().to_string()),
    );
    if let Some(h) = hint.filter(|s| !s.trim().is_empty()) {
        create_cmd.push_str(&format!(" --hint={}", shell_escape_single(h.trim())));
    }

    if let Ok(out) = run_key_create_via_script(&create_cmd, password).map(|o| o.status.success()) {
        if out && path.is_file() && path.metadata().map(|m| m.len()).unwrap_or(0) > 0 {
            return Ok(());
        }
    }

    warn!("script key create did not produce a key file, trying direct PBS call");
    let mut cmd = StdCommand::new(pbs);
    cmd.arg("key").arg("create").arg(path);
    if let Some(h) = hint.filter(|s| !s.trim().is_empty()) {
        cmd.arg(format!("--hint={}", h.trim()));
    }
    cmd.env("PBS_ENCRYPTION_PASSWORD", password);
    let out = cmd.output().map_err(CoreError::Io)?;
    if out.status.success() && path.is_file() && path.metadata().map(|m| m.len()).unwrap_or(0) > 0 {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let detail = stderr.if_empty_then("no TTY — create the key in a terminal and use Import");
    Err(CoreError::PbsCommand(format!(
        "could not create encryption key: {detail}"
    )))
}

/// Runs `proxmox-backup-client key create` inside a pseudo-TTY (`script -qfc`).
fn run_key_create_via_script(create_cmd: &str, password: &str) -> Result<std::process::Output> {
    let out = run_script_with_password_lines(create_cmd, &[password, password])?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        warn!(%err, "script key create failed");
    }
    Ok(out)
}

fn shell_escape_single(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_fingerprint_strips_separators() {
        assert_eq!(
            normalize_fingerprint("AA:BB:CC"),
            normalize_fingerprint("aabbcc")
        );
    }

    #[test]
    fn fingerprints_match_ignores_format() {
        assert!(fingerprints_match("aa:bb:cc", "aabbcc"));
        assert!(!fingerprints_match("aa:bb:cc", "dd:ee:ff"));
    }

    #[test]
    fn encryption_key_requires_password_detects_kdf() {
        let dir = std::env::temp_dir().join("backuppilot-key-kdf-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("plain.json");
        std::fs::write(&plain, r#"{"kdf":null,"data":"x"}"#).unwrap();
        assert!(!encryption_key_requires_password(&plain).unwrap());
        let protected = dir.join("protected.json");
        std::fs::write(
            &protected,
            r#"{"kdf":{"Scrypt":{"n":1,"r":1,"p":1,"salt":"AA=="}},"data":"x"}"#,
        )
        .unwrap();
        assert!(encryption_key_requires_password(&protected).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_key_unlocks_rejects_wrong_password_when_pbs_available() {
        let pbs = pbs_client_path();
        if !pbs.is_file() {
            return;
        }
        if !StdCommand::new("script")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return;
        }
        let dir = std::env::temp_dir().join("backuppilot-key-verify-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let key_path = dir.join("test.json");
        let create_cmd = format!(
            "{} key create {}",
            shell_escape_single(&pbs.display().to_string()),
            shell_escape_single(&key_path.display().to_string()),
        );
        let out = run_key_create_via_script(&create_cmd, "TestPass1234").unwrap();
        if !out.status.success() || !key_path.is_file() {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        assert!(verify_key_unlocks(&key_path, "TestPass1234").is_ok());
        let err = verify_key_unlocks(&key_path, "wrong-password").unwrap_err();
        assert!(
            err.to_string().contains("incorrect"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

trait IfEmptyThen {
    fn if_empty_then(self, fallback: &str) -> String;
}

impl IfEmptyThen for String {
    fn if_empty_then(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}
