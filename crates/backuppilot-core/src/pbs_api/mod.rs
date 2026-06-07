/// Native Rust HTTP client for the Proxmox Backup Server REST API.
///
/// Works on all platforms -- only `reqwest` + TLS, no Unix-specific code.
mod snapshots;

use std::time::Duration;

use reqwest::{Client, ClientBuilder};

use crate::pbs_repository::PbsRepositoryParts;
use crate::profile::CredentialVerifyResult;
use crate::restore::PbsSnapshotInfo;

pub use snapshots::PbsApiError;

/// Effective datastore permissions for the configured API token.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DatastorePermissions {
    /// Token has Datastore.Prune or Datastore.Modify -- may delete snapshots.
    pub can_delete: bool,
    /// Token has Datastore.Read or Datastore.Modify -- may restore and browse catalogs.
    pub can_restore: bool,
}

impl DatastorePermissions {
    pub fn none() -> Self {
        Self { can_delete: false, can_restore: false }
    }
}

/// HTTP client bound to one PBS datastore + namespace.
pub struct PbsApiClient {
    client: Client,
    base_url: String,
    auth_header: String,
    datastore: String,
    namespace: Option<String>,
}

impl PbsApiClient {
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
        let auth_header = format!("PBSAPIToken={auth_id}:{secret}");

        let mut builder = ClientBuilder::new()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10));

        if server_fingerprint.is_some() {
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

    pub(crate) fn delete(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .delete(format!("{}{}", self.base_url, path))
            .header("Authorization", &self.auth_header)
    }

    pub(crate) fn datastore(&self) -> &str {
        &self.datastore
    }

    pub(crate) fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    // -- Public operations ---------------------------------------------------

    pub async fn verify_credentials(&self) -> CredentialVerifyResult {
        snapshots::api_verify_credentials(self).await
    }

    pub async fn list_snapshots(
        &self,
        backup_id: &str,
    ) -> Result<Vec<PbsSnapshotInfo>, PbsApiError> {
        snapshots::api_list_snapshots(self, backup_id).await
    }

    pub async fn list_snapshot_archives(
        &self,
        snapshot_path: &str,
    ) -> Result<Vec<String>, PbsApiError> {
        snapshots::api_list_snapshot_archives(self, snapshot_path).await
    }

    pub async fn delete_snapshot(&self, snapshot_path: &str) -> Result<(), PbsApiError> {
        snapshots::api_delete_snapshot(self, snapshot_path).await
    }

    pub async fn check_datastore_permissions(&self) -> Result<DatastorePermissions, PbsApiError> {
        snapshots::api_check_datastore_permissions(self).await
    }
}
