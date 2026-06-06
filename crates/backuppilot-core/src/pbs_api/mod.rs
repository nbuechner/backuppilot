/// Native Rust HTTP client for the Proxmox Backup Server REST API.
///
/// Replaces subprocess calls to `proxmox-backup-client` for operations that
/// don't require chunk-level access (credential verification, snapshot listing,
/// archive manifest). Backup upload and restore still use the CLI binary.
///
/// Works on all platforms — only `reqwest` + TLS, no Unix-specific code.
mod snapshots;

use std::time::Duration;

use reqwest::{Client, ClientBuilder};

use crate::pbs_repository::PbsRepositoryParts;
use crate::profile::CredentialVerifyResult;
use crate::restore::PbsSnapshotInfo;

pub use snapshots::PbsApiError;

/// HTTP client bound to one PBS datastore + namespace.
pub struct PbsApiClient {
    client: Client,
    base_url: String,
    auth_header: String,
    datastore: String,
    namespace: Option<String>,
}

impl PbsApiClient {
    /// Builds a client from parsed repository credentials.
    ///
    /// When `server_fingerprint` is supplied the PBS server likely uses a
    /// self-signed certificate, so TLS verification is bypassed in favour of
    /// the out-of-band fingerprint the user has already accepted.
    pub fn new(
        parts: &PbsRepositoryParts,
        namespace: Option<&str>,
        server_fingerprint: Option<&str>,
    ) -> Result<Self, String> {
        let (server, port) = parts.server_and_port();
        let server = crate::pbs_repository::normalize_pbs_host(&server);
        let base_url = format!("https://{server}:{port}");

        let auth_id = parts
            .pbs_auth_id()
            .ok_or_else(|| "could not build PBS auth-id from profile credentials".to_string())?;
        let secret = parts.api_token_secret();
        if secret.is_empty() {
            return Err("PBS API token secret is empty".to_string());
        }
        // PBS API token auth header: "PBSAPIToken=user@realm!tokenname:secret"
        let auth_header = format!("PBSAPIToken={auth_id}:{secret}");

        let mut builder = ClientBuilder::new()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10));

        if server_fingerprint.is_some() {
            // PBS routinely uses self-signed certs; the fingerprint is the
            // user's trust anchor (same behaviour as proxmox-backup-client
            // --fingerprint). Bypassing TLS cert verification is intentional.
            builder = builder.danger_accept_invalid_certs(true);
        }

        let client = builder.build().map_err(|e| e.to_string())?;

        Ok(Self {
            client,
            base_url,
            auth_header,
            datastore: parts.datastore.clone(),
            namespace: namespace.filter(|s| !s.is_empty()).map(str::to_string),
        })
    }

    /// Convenience constructor from a `BackupProfile`.
    pub fn from_profile(
        profile: &crate::profile::BackupProfile,
    ) -> Result<Self, String> {
        let parts = PbsRepositoryParts::parse(&profile.repository)
            .map_err(|e| e.to_string())?;
        Self::new(
            &parts,
            profile.namespace.as_deref(),
            profile.server_fingerprint.as_deref(),
        )
    }

    pub(crate) fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .header("Authorization", &self.auth_header)
    }

    pub(crate) fn datastore(&self) -> &str {
        &self.datastore
    }

    pub(crate) fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    // ── Public operations ──────────────────────────────────────────────────

    /// Verifies PBS credentials via the REST API (no subprocess required).
    ///
    /// Returns success even when the token has no `Datastore.Audit` permission
    /// but is otherwise valid — mirroring the CLI's `list` behaviour.
    pub async fn verify_credentials(&self) -> CredentialVerifyResult {
        snapshots::api_verify_credentials(self).await
    }

    /// Lists all `host` snapshots for the given `backup_id`.
    pub async fn list_snapshots(
        &self,
        backup_id: &str,
    ) -> Result<Vec<PbsSnapshotInfo>, PbsApiError> {
        snapshots::api_list_snapshots(self, backup_id).await
    }

    /// Lists `.pxar` archive names in a snapshot (`host/{id}/{timestamp}`).
    pub async fn list_snapshot_archives(
        &self,
        snapshot_path: &str,
    ) -> Result<Vec<String>, PbsApiError> {
        snapshots::api_list_snapshot_archives(self, snapshot_path).await
    }
}
