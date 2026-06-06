//! Proxmox Backup Server snapshot listing and restore via `proxmox-backup-client`.

mod catalog;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

pub use catalog::{CatalogEntry, CatalogLine};

use serde_json::Value;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::debug;

use crate::error::{CoreError, Result};
use crate::paths::pbs_client_path;
use crate::encryption::{
    apply_encryption_to_command, fingerprints_match, EncryptionCliMode, EncryptionKey,
    EncryptionKeyProfileUsage,
};
use crate::pbs::{apply_pbs_client_env, backup_archive_name};
use crate::pbs_repository::PbsRepositoryParts;
use crate::profile::BackupProfile;

/// Quick PBS queries (snapshot list, manifest).
const PBS_CMD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
/// Large snapshots can take a long time to dump; same order as backup runs.
const CATALOG_DUMP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
/// Restore can run for hours on large archives (aligned with backup timeout in `pbs.rs`).
const RESTORE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(86_400);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PbsSnapshotInfo {
    /// Full snapshot path, e.g. `host/my-laptop/2025-05-18T10:00:00Z`.
    pub path: String,
    pub size_bytes: u64,
    pub archives: Vec<String>,
    /// Present when the snapshot was created with client-side encryption (PBS manifest fingerprint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// Derived from [`Self::fingerprint`] and encryption markers in the file list.
    #[serde(default)]
    pub encrypted: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RestoreArchiveRequest {
    pub profile_id: i64,
    pub snapshot: String,
    pub archive_name: String,
    pub target_dir: String,
    pub overwrite: bool,
    /// Empty = restore entire archive. Otherwise only matching paths (glob).
    #[serde(default)]
    pub patterns: Vec<String>,
    /// When set, overrides the profile's assigned key (e.g. snapshot fingerprint match).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key_id: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListCatalogRequest {
    pub profile_id: i64,
    pub snapshot: String,
    pub archive_name: String,
    /// Directory inside the archive (`"` = root).
    #[serde(default)]
    pub parent_path: String,
    /// When true, bypass the in-memory catalog cache.
    #[serde(default)]
    pub force_refresh: bool,
    /// When set, overrides the profile's assigned key (e.g. snapshot fingerprint match).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key_id: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListCatalogResponse {
    pub parent_path: String,
    pub entries: Vec<CatalogEntry>,
    /// Archive names found in the snapshot catalog.
    pub archives: Vec<String>,
    /// Best archive for restore when the UI guessed wrong (e.g. not `root.pxar`).
    pub suggested_archive: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RestoreArchiveResult {
    pub ok: bool,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RestoreConflictReport {
    pub conflicts: Vec<String>,
}

pub struct PbsRestore;

/// Archive names for restore/mount: PBS manifest, else profile backup paths (same as GUI).
pub fn resolve_snapshot_archives(profile: &BackupProfile, snap: &PbsSnapshotInfo) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();

    for (index, path) in profile.paths.iter().enumerate() {
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        let name = backup_archive_name(path, index);
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }

    for archive in &snap.archives {
        if archive.ends_with(".pxar") && seen.insert(archive.clone()) {
            names.push(archive.clone());
        }
    }

    names.sort();
    names
}

/// Resolves snapshot by full path or trailing timestamp segment.
pub fn find_snapshot<'a>(
    snapshots: &'a [PbsSnapshotInfo],
    snapshot_path: &str,
) -> Option<&'a PbsSnapshotInfo> {
    if let Some(snap) = snapshots.iter().find(|s| s.path == snapshot_path) {
        return Some(snap);
    }
    let leaf = snapshot_path.trim().trim_matches('/');
    snapshots.iter().find(|s| {
        s.path == leaf
            || s.path.ends_with(&format!("/{leaf}"))
            || s.path.rsplit('/').next() == Some(leaf)
    })
}

/// Lists `.pxar` archives for a snapshot (PBS manifest + profile-path fallback).
pub async fn list_archives_for_snapshot(
    profile: &BackupProfile,
    snapshot_path: &str,
) -> Result<Vec<String>> {
    let snapshots = PbsRestore::list_snapshots(profile).await?;
    let snap = find_snapshot(&snapshots, snapshot_path);

    if let Some(snap) = snap {
        let from_profile = resolve_snapshot_archives(profile, snap);
        if !from_profile.is_empty() {
            return Ok(from_profile);
        }
    }

    let from_pbs = PbsRestore::list_snapshot_archives(profile, snapshot_path).await?;
    if !from_pbs.is_empty() {
        return Ok(from_pbs);
    }

    if let Some(snap) = snap {
        return Ok(resolve_snapshot_archives(profile, snap));
    }

    Ok(profile
        .paths
        .iter()
        .enumerate()
        .map(|(index, path)| backup_archive_name(path.trim(), index))
        .filter(|name| !name.is_empty())
        .collect())
}

/// Scans PBS snapshots and merges per-key usage (fingerprints + profile assignment).
pub async fn enrich_encryption_keys_usage(
    keys: &mut [EncryptionKey],
    profiles: &[BackupProfile],
) {
    let mut snapshot_counts: HashMap<i64, HashMap<String, u32>> = HashMap::new();

    for profile in profiles {
        let Ok(snapshots) = PbsRestore::list_snapshots(profile).await else {
            continue;
        };
        for snap in snapshots {
            if !snap.encrypted {
                continue;
            }
            let Some(ref snap_fp) = snap.fingerprint else {
                continue;
            };
            for key in keys.iter() {
                let Some(ref key_fp) = key.fingerprint else {
                    continue;
                };
                if fingerprints_match(snap_fp, key_fp) {
                    *snapshot_counts
                        .entry(key.id)
                        .or_default()
                        .entry(profile.name.clone())
                        .or_insert(0) += 1;
                }
            }
        }
    }

    for key in keys.iter_mut() {
        let mut usage_by_profile: HashMap<String, EncryptionKeyProfileUsage> = HashMap::new();

        for profile in profiles {
            if profile.encryption_key_id == Some(key.id) {
                usage_by_profile.insert(
                    profile.name.clone(),
                    EncryptionKeyProfileUsage {
                        profile_name: profile.name.clone(),
                        assigned: true,
                        encrypted_snapshots: 0,
                    },
                );
            }
        }

        if let Some(counts) = snapshot_counts.get(&key.id) {
            for (profile_name, count) in counts {
                usage_by_profile
                    .entry(profile_name.clone())
                    .and_modify(|u| u.encrypted_snapshots = *count)
                    .or_insert(EncryptionKeyProfileUsage {
                        profile_name: profile_name.clone(),
                        assigned: false,
                        encrypted_snapshots: *count,
                    });
            }
        }

        let mut usage: Vec<_> = usage_by_profile.into_values().collect();
        usage.sort_by(|a, b| a.profile_name.cmp(&b.profile_name));
        key.profile_usage = usage;
        key.profiles_using = key
            .profile_usage
            .iter()
            .map(|u| u.profile_name.clone())
            .collect();
        key.in_use = key
            .profile_usage
            .iter()
            .any(|u| u.encrypted_snapshots > 0 || u.assigned);
    }
}

/// Resolves which stored key matches an encrypted snapshot (by PBS fingerprint).
pub fn resolve_encryption_key_id_for_snapshot(
    profile: &BackupProfile,
    snapshot: &PbsSnapshotInfo,
    keys: &[EncryptionKey],
) -> Option<i64> {
    if !snapshot.encrypted {
        return profile.encryption_key_id;
    }
    let fp = snapshot.fingerprint.as_deref()?;
    keys.iter()
        .find(|k| {
            k.fingerprint
                .as_ref()
                .is_some_and(|kfp| fingerprints_match(fp, kfp))
        })
        .map(|k| k.id)
}

impl PbsRestore {
    pub async fn list_snapshots(profile: &BackupProfile) -> Result<Vec<PbsSnapshotInfo>> {
        // REST API fallback: works on platforms without the CLI binary (e.g. Windows).
        if !crate::pbs::PbsClient::is_available().await {
            let api = crate::pbs_api::PbsApiClient::from_profile(profile)
                .map_err(|e| CoreError::PbsCommand(e))?;
            return api
                .list_snapshots(&profile.backup_id)
                .await
                .map_err(|e| CoreError::PbsCommand(e.to_string()));
        }

        let parts = repository_parts(profile)?;
        let group = snapshot_group(profile);
        let output = run_pbs_with_timeout(
            profile,
            &parts,
            PBS_CMD_TIMEOUT,
            EncryptionCliMode::None,
            None,
            |cmd, repo, _password| {
                cmd.arg("snapshot")
                    .arg("list")
                    .arg(&group)
                    .arg(format!("--repository={repo}"))
                    .arg("--output-format=json");
                apply_namespace(cmd, profile);
            },
        )
        .await?;

        if !output.status.success() {
            return Err(CoreError::PbsCommand(pbs_stderr(&output)));
        }

        parse_snapshot_list(
            &String::from_utf8_lossy(&output.stdout),
            &profile.backup_id,
        )
    }

    /// Central restore entry: conflict check, then `proxmox-backup-client restore`.
    pub async fn execute_restore(
        profile: &BackupProfile,
        request: &RestoreArchiveRequest,
    ) -> Result<RestoreArchiveResult> {
        let target = Path::new(&request.target_dir);
        if !request.overwrite {
            let key_id = request.encryption_key_id.or(profile.encryption_key_id);
            let conflicts = Self::find_restore_conflicts(
                profile,
                &request.snapshot,
                &request.archive_name,
                target,
                &request.patterns,
                key_id,
            )
            .await?;
            if !conflicts.is_empty() {
                return Ok(RestoreArchiveResult {
                    ok: false,
                    message: Some(format_restore_conflicts(&conflicts)),
                    conflicts,
                });
            }
        }

        let key_id = request.encryption_key_id.or(profile.encryption_key_id);
        Self::restore_archive(
            profile,
            &request.snapshot,
            &request.archive_name,
            target,
            request.overwrite,
            &request.patterns,
            key_id,
        )
        .await
    }

    pub async fn find_restore_conflicts(
        profile: &BackupProfile,
        snapshot: &str,
        archive_name: &str,
        target: &Path,
        patterns: &[String],
        encryption_key_id: Option<i64>,
    ) -> Result<Vec<String>> {
        let lines =
            load_catalog_lines(profile, snapshot, archive_name, false, encryption_key_id).await?;
        let mut conflicts = Vec::new();
        let rel_paths = restore_relative_paths(&lines, archive_name, patterns);
        for rel in rel_paths {
            let dest = target.join(&rel);
            if dest.exists() {
                conflicts.push(dest.display().to_string());
            }
        }
        conflicts.sort();
        conflicts.dedup();
        Ok(conflicts)
    }

    pub async fn restore_archive(
        profile: &BackupProfile,
        snapshot: &str,
        archive_name: &str,
        target: &Path,
        overwrite: bool,
        patterns: &[String],
        encryption_key_id: Option<i64>,
    ) -> Result<RestoreArchiveResult> {
        let parts = repository_parts(profile)?;
        let output = run_pbs_with_timeout(
            profile,
            &parts,
            RESTORE_TIMEOUT,
            EncryptionCliMode::Decrypt,
            encryption_key_id,
            |cmd, repo, _password| {
                cmd.arg("restore")
                    .arg(snapshot)
                    .arg(archive_name)
                    .arg(target);
                cmd.arg(format!("--repository={repo}"));
                apply_restore_overwrite_flags(cmd, overwrite);
                for pattern in patterns {
                    let p = pattern.trim();
                    if !p.is_empty() {
                        cmd.arg(format!("--pattern={p}"));
                    }
                }
                apply_namespace(cmd, profile);
            },
        )
        .await?;

        if output.status.success() {
            Ok(RestoreArchiveResult {
                ok: true,
                message: None,
                conflicts: Vec::new(),
            })
        } else {
            Ok(RestoreArchiveResult {
                ok: false,
                message: Some(pbs_stderr(&output)),
                conflicts: Vec::new(),
            })
        }
    }

    pub async fn list_catalog(request: &ListCatalogRequest, profile: &BackupProfile) -> Result<ListCatalogResponse> {
        let key_id = request.encryption_key_id.or(profile.encryption_key_id);
        let lines = load_catalog_lines(
            profile,
            &request.snapshot,
            &request.archive_name,
            request.force_refresh,
            key_id,
        )
        .await?;
        let parent = request.parent_path.trim().trim_matches('/').to_string();
        let mut archives = catalog::archives_in_lines(&lines);
        if archives.is_empty() {
            if let Ok(manifest) = Self::list_snapshot_archives(profile, &request.snapshot).await {
                archives = manifest;
            }
        }
        let suggested_archive = catalog::primary_archive(&lines)
            .or_else(|| archives.first().cloned())
            .unwrap_or_else(|| request.archive_name.clone());

        let entries = catalog::list_children(&lines, &parent, &suggested_archive);
        if entries.is_empty() && parent.is_empty() && lines.is_empty() {
            return Err(CoreError::PbsCommand(
                "backup catalog is empty or could not be read — try “Reload file list” or restore via CLI"
                    .into(),
            ));
        }

        Ok(ListCatalogResponse {
            parent_path: parent,
            entries,
            archives,
            suggested_archive,
        })
    }

    /// Lists `.pxar` archive names from the snapshot manifest (`snapshot files`).
    pub async fn list_snapshot_archives(
        profile: &BackupProfile,
        snapshot: &str,
    ) -> Result<Vec<String>> {
        // REST API fallback: works on platforms without the CLI binary (e.g. Windows).
        if !crate::pbs::PbsClient::is_available().await {
            let api = crate::pbs_api::PbsApiClient::from_profile(profile)
                .map_err(|e| CoreError::PbsCommand(e))?;
            return api
                .list_snapshot_archives(snapshot)
                .await
                .map_err(|e| CoreError::PbsCommand(e.to_string()));
        }

        let parts = repository_parts(profile)?;
        let output = run_pbs_with_timeout(
            profile,
            &parts,
            PBS_CMD_TIMEOUT,
            EncryptionCliMode::None,
            None,
            |cmd, repo, _password| {
                cmd.arg("snapshot")
                    .arg("files")
                    .arg(snapshot)
                    .arg(format!("--repository={repo}"))
                    .arg("--output-format=json");
                apply_namespace(cmd, profile);
            },
        )
        .await?;

        if !output.status.success() {
            return Err(CoreError::PbsCommand(pbs_stderr(&output)));
        }

        Ok(parse_snapshot_files_json(
            &String::from_utf8_lossy(&output.stdout),
        ))
    }
}

fn catalog_cache() -> &'static Mutex<HashMap<String, Vec<CatalogLine>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<CatalogLine>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn catalog_cache_key(
    profile_id: i64,
    snapshot: &str,
    archive: &str,
    encryption_key_id: Option<i64>,
) -> String {
    format!("{profile_id}:{snapshot}:{archive}:key={encryption_key_id:?}")
}

pub fn clear_catalog_cache(profile_id: i64, snapshot: &str, archive_name: &str) {
    let key = catalog_cache_key(profile_id, snapshot, archive_name, None);
    if let Ok(mut cache) = catalog_cache().lock() {
        cache.remove(&key);
    }
}

pub fn clear_all_catalog_cache() {
    if let Ok(mut cache) = catalog_cache().lock() {
        cache.clear();
    }
}

async fn load_catalog_lines(
    profile: &BackupProfile,
    snapshot: &str,
    archive_name: &str,
    force_refresh: bool,
    encryption_key_id: Option<i64>,
) -> Result<Vec<CatalogLine>> {
    let key = catalog_cache_key(profile.id, snapshot, archive_name, encryption_key_id);
    if !force_refresh {
        if let Some(lines) = catalog_cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Ok(lines);
        }
    }

    let parts = repository_parts(profile)?;
    let output = run_pbs_with_timeout(
        profile,
        &parts,
        CATALOG_DUMP_TIMEOUT,
        EncryptionCliMode::Decrypt,
        encryption_key_id,
        |cmd, repo, _password| {
            cmd.arg("catalog")
                .arg("dump")
                .arg(snapshot)
                .arg(format!("--repository={repo}"));
            // Avoid log noise on stderr; catalog lines are still read from both streams.
            cmd.env("RUST_LOG", "off");
            apply_namespace(cmd, profile);
        },
    )
    .await?;

    if !output.status.success() {
        return Err(CoreError::PbsCommand(pbs_stderr(&output)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = combine_catalog_output(&stdout, &stderr);

    debug!(
        snapshot,
        archive = archive_name,
        stdout_len = output.stdout.len(),
        stderr_len = output.stderr.len(),
        "catalog dump finished"
    );

    if combined.trim().is_empty() {
        return Err(CoreError::PbsCommand(
            "catalog dump returned no data (snapshot may have no file catalog)".into(),
        ));
    }

    let mut lines = catalog::parse_catalog_dump(&combined, archive_name);

    if lines.is_empty() {
        lines = catalog::parse_catalog_dump(&combined, "");
    }

    if lines.is_empty() {
        let preview: String = combined.chars().take(240).collect();
        return Err(CoreError::PbsCommand(format!(
            "could not parse catalog dump (output starts with: {preview:?})"
        )));
    }

    if let Ok(mut cache) = catalog_cache().lock() {
        cache.insert(key, lines.clone());
    }

    Ok(lines)
}

fn parse_snapshot_files_json(stdout: &str) -> Vec<String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let value: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let items = match value {
        Value::Array(arr) => arr,
        Value::Object(obj) => obj
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    let mut names: Vec<String> = items
        .iter()
        .filter_map(|item| snapshot_file_name(item))
        .filter(|name| name.ends_with(".pxar"))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn snapshot_file_name(item: &Value) -> Option<String> {
    item.get("filename")
        .or_else(|| item.get("name"))
        .or_else(|| item.get("file"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn combine_catalog_output(stdout: &str, stderr: &str) -> String {
    let mut out = String::new();
    if !stdout.trim().is_empty() {
        out.push_str(stdout);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    if !stderr.trim().is_empty() {
        out.push_str(stderr);
    }
    out
}

async fn run_pbs_with_timeout(
    profile: &BackupProfile,
    parts: &PbsRepositoryParts,
    limit: std::time::Duration,
    encryption_mode: EncryptionCliMode,
    encryption_key_id: Option<i64>,
    configure: impl FnOnce(&mut Command, &str, &str),
) -> Result<std::process::Output> {
    let auth_id = parts
        .pbs_auth_id()
        .ok_or_else(|| CoreError::PbsCommand("missing API token".into()))?;
    let repo_arg = parts.pbs_repository_bash_style(&auth_id);
    let password = parts.api_token_secret();

    let _ = crate::pbs::PbsClient::write_profile_config(profile);

    let mut cmd = Command::new(pbs_client_path());
    apply_pbs_client_env(
        &mut cmd,
        parts,
        &repo_arg,
        &password,
        profile.server_fingerprint.as_deref(),
    );
    configure(&mut cmd, &repo_arg, &password);
    let key_id = encryption_key_id.or(profile.encryption_key_id);
    apply_encryption_to_command(&mut cmd, key_id, encryption_mode)?;

    timeout(limit, cmd.output())
        .await
        .map_err(|_| {
            let mins = limit.as_secs() / 60;
            CoreError::PbsCommand(format!(
                "pbs command timed out after {mins} minutes"
            ))
        })?
        .map_err(CoreError::Io)
}

/// PBS needs these together when restoring into an existing directory (e.g. original path).
fn apply_restore_overwrite_flags(cmd: &mut Command, overwrite: bool) {
    if overwrite {
        cmd.arg("--overwrite=true");
        cmd.arg("--overwrite-files=true");
        cmd.arg("--allow-existing-dirs=true");
    }
}

fn restore_relative_paths(
    lines: &[CatalogLine],
    archive_name: &str,
    patterns: &[String],
) -> Vec<String> {
    if patterns.is_empty() {
        return lines
            .iter()
            .filter(|l| l.archive == archive_name || archive_name.is_empty())
            .filter(|l| !l.is_dir)
            .map(|l| l.path.trim_start_matches('/').to_string())
            .filter(|p| !p.is_empty())
            .collect();
    }

    let mut out = Vec::new();
    for pattern in patterns {
        let pattern = pattern.trim().trim_start_matches('/');
        if pattern.is_empty() {
            continue;
        }
        out.push(pattern.to_string());
        for line in lines {
            if line.is_dir {
                continue;
            }
            let path = line.path.trim_start_matches('/');
            if path == pattern || path.starts_with(&format!("{pattern}/")) {
                out.push(path.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn format_restore_conflicts(conflicts: &[String]) -> String {
    let preview: Vec<_> = conflicts.iter().take(5).cloned().collect();
    let mut msg = format!(
        "{} file(s) already exist at the restore target. Enable overwrite or choose another folder.",
        conflicts.len()
    );
    if !preview.is_empty() {
        msg.push_str("\n");
        msg.push_str(&preview.join("\n"));
        if conflicts.len() > preview.len() {
            msg.push_str("\n…");
        }
    }
    msg
}

fn snapshot_group(profile: &BackupProfile) -> String {
    format!("host/{}", profile.backup_id)
}

fn repository_parts(profile: &BackupProfile) -> Result<PbsRepositoryParts> {
    PbsRepositoryParts::parse(&profile.repository).map_err(|e| CoreError::PbsCommand(e.to_string()))
}

fn apply_namespace(cmd: &mut Command, profile: &BackupProfile) {
    if let Some(ns) = profile.namespace.as_ref().filter(|s| !s.is_empty()) {
        cmd.arg(format!("--ns={ns}"));
    }
}

fn pbs_stderr(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.trim().is_empty() {
        format!("exit code {}", output.status.code().unwrap_or(-1))
    } else {
        stderr.trim().to_string()
    }
}

fn parse_snapshot_list(stdout: &str, backup_id: &str) -> Result<Vec<PbsSnapshotInfo>> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let value: Value = serde_json::from_str(trimmed)
        .map_err(|e| CoreError::PbsCommand(format!("invalid snapshot list JSON: {e}")))?;

    let items = match value {
        Value::Array(arr) => arr,
        Value::Object(obj) => obj
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    let mut snapshots: Vec<PbsSnapshotInfo> = items
        .iter()
        .filter_map(|item| parse_snapshot_item(item, backup_id))
        .collect();

    dedupe_snapshots_by_path(&mut snapshots);
    snapshots.sort_by(|a, b| b.path.cmp(&a.path));
    Ok(snapshots)
}

fn dedupe_snapshots_by_path(snapshots: &mut Vec<PbsSnapshotInfo>) {
    let mut seen = HashMap::new();
    snapshots.retain(|snap| {
        if seen.insert(snap.path.clone(), ()).is_some() {
            false
        } else {
            true
        }
    });
}

fn parse_snapshot_item(item: &Value, backup_id: &str) -> Option<PbsSnapshotInfo> {
    if let Some(path) = item.get("snapshot").and_then(|v| v.as_str()) {
        let size_bytes = json_u64(item.get("size")).unwrap_or(0);
        let archives = parse_archives_field(item.get("files"));
        let fingerprint = json_fingerprint(item.get("fingerprint"));
        let encrypted = snapshot_is_encrypted(item, &archives, fingerprint.as_deref());
        return Some(PbsSnapshotInfo {
            path: path.to_string(),
            size_bytes,
            archives,
            fingerprint,
            encrypted,
        });
    }

    let backup_type = item
        .get("backup-type")
        .or_else(|| item.get("backup_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("host");
    let id = item
        .get("backup-id")
        .or_else(|| item.get("backup_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(backup_id);
    let time = json_timestamp_iso(
        item.get("backup-time").or_else(|| item.get("backup_time")),
    )?;

    let path = format!("{backup_type}/{id}/{time}");
    let size_bytes = json_u64(item.get("size")).unwrap_or(0);
    let archives = parse_archives_field(item.get("files"));
    let fingerprint = json_fingerprint(item.get("fingerprint"));
    let encrypted = snapshot_is_encrypted(item, &archives, fingerprint.as_deref());

    Some(PbsSnapshotInfo {
        path,
        size_bytes,
        archives,
        fingerprint,
        encrypted,
    })
}

fn json_fingerprint(value: Option<&Value>) -> Option<String> {
    let v = value?;
    if let Some(s) = v.as_str() {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        return Some(s.to_string());
    }
    if let Some(obj) = v.as_object() {
        if let Some(s) = obj.get("text").and_then(|t| t.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn snapshot_is_encrypted(item: &Value, archives: &[String], fingerprint: Option<&str>) -> bool {
    if fingerprint.is_some() {
        return true;
    }
    if archives.iter().any(|name| {
        name == "rsa-encrypted.key"
            || name.ends_with("encrypted.key")
            || name == "encryption-key.json"
    }) {
        return true;
    }
    let mode = item
        .get("crypt-mode")
        .or_else(|| item.get("crypt_mode"))
        .and_then(|v| v.as_str());
    matches!(mode, Some("encrypt" | "sign-only"))
}

fn json_timestamp_iso(value: Option<&Value>) -> Option<String> {
    let v = value?;
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    let secs = v.as_i64()?;
    chrono::DateTime::from_timestamp(secs, 0).map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

fn json_u64(value: Option<&Value>) -> Option<u64> {
    let v = value?;
    v.as_u64()
        .or_else(|| v.as_i64().map(|n| n.max(0) as u64))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn parse_archives_field(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => s
            .split_whitespace()
            .filter(|a| {
                a.ends_with(".pxar")
                    || *a == "catalog.pcat1"
                    || a.ends_with(".json")
                    || !a.contains('.')
            })
            .filter(|a| *a != "catalog.pcat1" && *a != "index.json")
            .map(str::to_string)
            .collect(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .filter(|a| a.ends_with(".pxar") || !a.contains('.'))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snapshot_array() {
        let json = r#"[
            {"snapshot":"host/pc/2025-01-01T12:00:00Z","size":1000,"files":"root.pxar catalog.pcat1"}
        ]"#;
        let list = parse_snapshot_list(json, "pc").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, "host/pc/2025-01-01T12:00:00Z");
        assert!(list[0].archives.contains(&"root.pxar".to_string()));
        assert!(!list[0].encrypted);
    }

    #[test]
    fn parses_encrypted_snapshot_fingerprint() {
        let json = r#"[
            {"snapshot":"host/pc/2025-05-21T07:25:42Z","size":100,"fingerprint":"aa:bb:cc","files":"home.pxar rsa-encrypted.key"}
        ]"#;
        let list = parse_snapshot_list(json, "pc").unwrap();
        assert!(list[0].encrypted);
        assert_eq!(list[0].fingerprint.as_deref(), Some("aa:bb:cc"));
    }

    #[test]
    fn dedupes_duplicate_snapshot_paths() {
        let json = r#"[
            {"snapshot":"host/pc/2025-01-01T12:00:00Z","size":1000},
            {"snapshot":"host/pc/2025-01-01T12:00:00Z","size":1000}
        ]"#;
        let list = parse_snapshot_list(json, "pc").unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn parses_unencrypted_with_profile_key_unrelated() {
        let json = r#"[
            {"snapshot":"host/pc/2025-01-01T12:00:00Z","size":1000,"files":"root.pxar"}
        ]"#;
        let list = parse_snapshot_list(json, "pc").unwrap();
        assert!(!list[0].encrypted);
    }

    #[test]
    fn resolve_archives_uses_profile_paths_when_manifest_empty() {
        use chrono::Utc;
        use crate::profile::{BackupConditions, HealthCheck, Schedule};

        let profile = BackupProfile {
            id: 1,
            name: "test".into(),
            enabled: true,
            repository: String::new(),
            api_token_configured: false,
            namespace: None,
            backup_id: "pc".into(),
            paths: vec!["/home/user".into()],
            excludes: vec![],
            schedule: Schedule::default(),
            conditions: BackupConditions::default(),
            health_check: HealthCheck::default(),
            encryption_key_id: None,
            server_fingerprint: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let snap = PbsSnapshotInfo {
            path: "host/pc/2025-01-01T12:00:00Z".into(),
            size_bytes: 1,
            archives: vec![],
            fingerprint: None,
            encrypted: false,
        };
        let names = resolve_snapshot_archives(&profile, &snap);
        assert_eq!(names.len(), 1);
        assert!(names[0].ends_with(".pxar"));
    }
}
