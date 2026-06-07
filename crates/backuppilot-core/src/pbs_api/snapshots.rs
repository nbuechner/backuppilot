use chrono::DateTime;
use serde_json::Value;
use tracing::debug;

use crate::profile::CredentialVerifyResult;
use crate::restore::PbsSnapshotInfo;

use super::PbsApiClient;

#[derive(Debug)]
pub enum PbsApiError {
    /// HTTP / network layer error.
    Http(String),
    /// PBS returned an error response with an optional message.
    Pbs { status: u16, message: String },
    /// Response body could not be parsed.
    Parse(String),
    /// A snapshot path could not be decoded into its components.
    InvalidSnapshot(String),
}

impl std::fmt::Display for PbsApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "PBS HTTP error: {e}"),
            Self::Pbs { status, message } => write!(f, "PBS {status}: {message}"),
            Self::Parse(e) => write!(f, "PBS response parse error: {e}"),
            Self::InvalidSnapshot(s) => write!(f, "invalid PBS snapshot path: {s}"),
        }
    }
}

impl From<reqwest::Error> for PbsApiError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e.to_string())
    }
}

// -- Credential verification -------------------------------------------------

pub(super) async fn api_verify_credentials(client: &PbsApiClient) -> CredentialVerifyResult {
    let url = format!(
        "/api2/json/admin/datastore/{}/snapshots",
        client.datastore()
    );
    let mut req = client.get(&url);
    if let Some(ns) = client.namespace() {
        req = req.query(&[("ns", ns)]);
    }

    debug!(url, "PBS API credential verification");

    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            debug!(status, "PBS API verify response");
            match status {
                200..=299 | 403 => CredentialVerifyResult { ok: true, message: None },
                401 => CredentialVerifyResult {
                    ok: false,
                    message: Some(extract_pbs_error(resp).await),
                },
                _ => CredentialVerifyResult {
                    ok: false,
                    message: Some(format!(
                        "PBS returned HTTP {status}: {}",
                        extract_pbs_error(resp).await
                    )),
                },
            }
        }
        Err(e) => CredentialVerifyResult {
            ok: false,
            message: Some(format!("PBS connection failed: {e}")),
        },
    }
}

// -- Snapshot listing --------------------------------------------------------

pub(super) async fn api_list_snapshots(
    client: &PbsApiClient,
    backup_id: &str,
) -> Result<Vec<PbsSnapshotInfo>, PbsApiError> {
    let url = format!(
        "/api2/json/admin/datastore/{}/snapshots",
        client.datastore()
    );
    let mut req = client
        .get(&url)
        .query(&[("backup-type", "host"), ("backup-id", backup_id)]);
    if let Some(ns) = client.namespace() {
        req = req.query(&[("ns", ns)]);
    }

    debug!(%url, backup_id, "PBS API list snapshots");

    let resp = req.send().await?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(PbsApiError::Pbs {
            status,
            message: extract_pbs_error(resp).await,
        });
    }

    let body: Value = resp.json().await.map_err(|e| PbsApiError::Parse(e.to_string()))?;
    parse_snapshot_list(&body, backup_id)
}

pub(super) async fn api_list_snapshot_archives(
    client: &PbsApiClient,
    snapshot_path: &str,
) -> Result<Vec<String>, PbsApiError> {
    let (backup_type, backup_id, backup_time_unix) = parse_snapshot_path(snapshot_path)?;

    let url = format!("/api2/json/admin/datastore/{}/files", client.datastore());
    let backup_time_str = backup_time_unix.to_string();
    let mut req = client.get(&url).query(&[
        ("backup-type", backup_type),
        ("backup-id", backup_id),
        ("backup-time", &backup_time_str),
    ]);
    if let Some(ns) = client.namespace() {
        req = req.query(&[("ns", ns)]);
    }

    debug!(%url, snapshot_path, "PBS API list snapshot archives");

    let resp = req.send().await?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(PbsApiError::Pbs {
            status,
            message: extract_pbs_error(resp).await,
        });
    }

    let body: Value = resp.json().await.map_err(|e| PbsApiError::Parse(e.to_string()))?;
    Ok(parse_archive_list(&body))
}

pub(super) async fn api_delete_snapshot(
    client: &PbsApiClient,
    snapshot_path: &str,
) -> Result<(), PbsApiError> {
    let (backup_type, backup_id, backup_time_unix) = parse_snapshot_path(snapshot_path)?;
    let url = format!("/api2/json/admin/datastore/{}/snapshots", client.datastore());
    let backup_time_str = backup_time_unix.to_string();
    let mut req = client.delete(&url).query(&[
        ("backup-type", backup_type),
        ("backup-id", backup_id),
        ("backup-time", backup_time_str.as_str()),
    ]);
    if let Some(ns) = client.namespace() {
        req = req.query(&[("ns", ns)]);
    }
    debug!(%url, snapshot_path, "PBS API delete snapshot");
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    if status == 200 || status == 204 {
        Ok(())
    } else {
        Err(PbsApiError::Pbs { status, message: extract_pbs_error(resp).await })
    }
}

/// Checks Datastore.Prune (delete) and Datastore.Read (restore/browse) in one API call.
pub(super) async fn api_check_datastore_permissions(
    client: &PbsApiClient,
) -> Result<super::DatastorePermissions, PbsApiError> {
    let path_param = format!("/datastore/{}", client.datastore());
    let req = client.get("/api2/json/access/permissions")
        .query(&[("path", path_param.as_str()), ("propagate", "0")]);
    debug!("PBS API check datastore permissions");
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    if status != 200 {
        // 401 = bad creds, 403 = no permission to query, anything else = network issue.
        // All mean we cannot confirm any elevated permission.
        return Ok(super::DatastorePermissions::none());
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| PbsApiError::Parse(e.to_string()))?;
    let perms = body.get("data").unwrap_or(&body);
    let has = |key: &str| perms.get(key).and_then(|v| v.as_u64()).unwrap_or(0) > 0;
    let modify = has("Datastore.Modify");
    Ok(super::DatastorePermissions {
        can_delete:  modify || has("Datastore.Prune"),
        can_restore: modify || has("Datastore.Read"),
    })
}

// -- Parsing helpers ---------------------------------------------------------

fn parse_snapshot_list(body: &Value, backup_id: &str) -> Result<Vec<PbsSnapshotInfo>, PbsApiError> {
    let items = data_array(body);
    let mut snapshots: Vec<PbsSnapshotInfo> = items
        .iter()
        .filter_map(|item| parse_snapshot_item(item, backup_id))
        .collect();

    snapshots.sort_by(|a, b| b.path.cmp(&a.path));
    snapshots.dedup_by(|a, b| a.path == b.path);
    Ok(snapshots)
}

fn parse_snapshot_item(item: &Value, backup_id: &str) -> Option<PbsSnapshotInfo> {
    let btype = item
        .get("backup-type")
        .or_else(|| item.get("backup_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("host");

    let id = item
        .get("backup-id")
        .or_else(|| item.get("backup_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(backup_id);

    let time_iso = timestamp_to_iso(item.get("backup-time"))?;
    let path = format!("{btype}/{id}/{time_iso}");

    let size_bytes = item
        .get("size")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let archives = parse_files_field(item.get("files"));
    let fingerprint = string_field(item, &["fingerprint"]);
    let encrypted = detect_encrypted(item, &archives, fingerprint.as_deref());
    let protected = item.get("protected").and_then(|v| v.as_bool()).unwrap_or(false);

    Some(PbsSnapshotInfo {
        path,
        size_bytes,
        archives,
        fingerprint,
        encrypted,
        protected,
    })
}

fn parse_archive_list(body: &Value) -> Vec<String> {
    let items = data_array(body);
    let mut names: Vec<String> = items
        .iter()
        .filter_map(|item| {
            item.get("filename")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .filter(|name| name.ends_with(".pxar"))
        .collect();
    names.sort();
    names.dedup();
    names
}

// -- Snapshot path utilities -------------------------------------------------

fn parse_snapshot_path(path: &str) -> Result<(&str, &str, i64), PbsApiError> {
    let parts: Vec<&str> = path.trim_matches('/').splitn(3, '/').collect();
    if parts.len() != 3 {
        return Err(PbsApiError::InvalidSnapshot(path.to_string()));
    }
    let backup_type = parts[0];
    let backup_id = parts[1];
    let timestamp_str = parts[2];

    let unix_ts = DateTime::parse_from_rfc3339(timestamp_str)
        .map(|dt| dt.timestamp())
        .map_err(|_| PbsApiError::InvalidSnapshot(format!("bad timestamp: {timestamp_str}")))?;

    Ok((backup_type, backup_id, unix_ts))
}

// -- JSON utilities ----------------------------------------------------------

fn data_array(body: &Value) -> Vec<Value> {
    match body {
        Value::Array(arr) => arr.clone(),
        Value::Object(obj) => obj
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn timestamp_to_iso(value: Option<&Value>) -> Option<String> {
    let secs = value?.as_i64()?;
    DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

fn parse_files_field(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Object(_) => v
                    .get("filename")
                    .or_else(|| v.get("name"))
                    .and_then(|n| n.as_str())
                    .map(str::to_string),
                _ => None,
            })
            .filter(|name| name.ends_with(".pxar") || name.ends_with(".img.fidx"))
            .collect(),
        Some(Value::String(s)) => s
            .split_whitespace()
            .filter(|a| a.ends_with(".pxar"))
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn detect_encrypted(item: &Value, archives: &[String], fingerprint: Option<&str>) -> bool {
    if fingerprint.is_some() {
        return true;
    }
    if archives.iter().any(|n| {
        n == "rsa-encrypted.key" || n.ends_with("encrypted.key") || n == "encryption-key.json"
    }) {
        return true;
    }
    matches!(
        item.get("crypt-mode")
            .or_else(|| item.get("crypt_mode"))
            .and_then(|v| v.as_str()),
        Some("encrypt" | "sign-only")
    )
}

fn string_field(item: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = item.get(*key).and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

// -- Error extraction --------------------------------------------------------

pub(super) async fn extract_pbs_error(resp: reqwest::Response) -> String {
    let status = resp.status().as_u16();
    match resp.json::<Value>().await {
        Ok(body) => body
            .get("errors")
            .and_then(|v| v.as_object())
            .and_then(|obj| obj.values().next())
            .and_then(|v| v.as_str())
            .or_else(|| body.get("message").and_then(|v| v.as_str()))
            .unwrap_or("unknown error")
            .to_string(),
        Err(_) => format!("HTTP {status}"),
    }
}

// -- Tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snapshot_list_data_wrapper() {
        let body = serde_json::json!({
            "data": [
                {
                    "backup-type": "host",
                    "backup-id": "mypc",
                    "backup-time": 1_700_000_000_i64,
                    "size": 1024,
                    "files": ["home.pxar", "catalog.pcat1"]
                }
            ]
        });
        let snaps = parse_snapshot_list(&body, "mypc").unwrap();
        assert_eq!(snaps.len(), 1);
        assert!(snaps[0].path.starts_with("host/mypc/"));
        assert!(snaps[0].archives.contains(&"home.pxar".to_string()));
        assert!(!snaps[0].encrypted);
        assert!(!snaps[0].protected);
    }

    #[test]
    fn parses_protected_snapshot() {
        let body = serde_json::json!({
            "data": [{
                "backup-type": "host",
                "backup-id": "pc",
                "backup-time": 1_700_000_000_i64,
                "size": 0,
                "protected": true
            }]
        });
        let snaps = parse_snapshot_list(&body, "pc").unwrap();
        assert!(snaps[0].protected);
    }

    #[test]
    fn parses_snapshot_path_valid() {
        let (t, id, ts) = parse_snapshot_path("host/myhost/2025-05-18T10:00:00Z").unwrap();
        assert_eq!(t, "host");
        assert_eq!(id, "myhost");
        assert!(ts > 0);
    }

    #[test]
    fn parses_snapshot_path_invalid() {
        assert!(parse_snapshot_path("host/only-two").is_err());
        assert!(parse_snapshot_path("host/id/not-a-timestamp").is_err());
    }
}
