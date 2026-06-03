//! YAML import/export for backup profiles (CLI and GUI).

use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::error::{CoreError, Result};
use crate::pbs_repository::{format_legacy_repository, PbsRepositoryParts};
use crate::profile::{
    normalize_new_profile, redact_profile_for_client, BackupProfile, NewProfile,
};
use crate::secrets::{has_api_token, hydrate_profile_repository, load_stored_api_token};

/// Profile document as stored in `.yaml` files (see `Dokumentation/profil-schema.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileYaml {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub backup_id: String,
    pub paths: Vec<String>,
    #[serde(default)]
    pub excludes: Vec<String>,
    pub schedule: crate::profile::Schedule,
    #[serde(default)]
    pub conditions: crate::profile::BackupConditions,
    #[serde(default)]
    pub health_check: crate::profile::HealthCheck,
    /// Encryption key name from **Encryption keys** in the GUI (not the numeric id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_fingerprint: Option<String>,
}

fn default_true() -> bool {
    true
}

impl ProfileYaml {
    pub fn parse_str(yaml: &str) -> Result<Self> {
        Ok(serde_yaml::from_str(yaml)?)
    }

    pub fn to_yaml_string(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }

    pub fn into_new_profile(self, db: &Database) -> Result<NewProfile> {
        self.validate_for_create()?;
        let encryption_key_id =
            resolve_encryption_key_name(db, self.encryption_key_name.as_deref())?;
        Ok(normalize_new_profile(NewProfile {
            name: self.name,
            enabled: self.enabled,
            repository: self.repository,
            namespace: self.namespace,
            backup_id: self.backup_id,
            paths: self.paths,
            excludes: self.excludes,
            schedule: self.schedule,
            conditions: self.conditions,
            health_check: self.health_check,
            encryption_key_id,
            server_fingerprint: self.server_fingerprint,
        }))
    }

    pub fn validate_for_create(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(CoreError::PbsCommand("profile name is required".into()));
        }
        if self.paths.is_empty() {
            return Err(CoreError::PbsCommand("at least one backup path is required".into()));
        }
        let parts = PbsRepositoryParts::parse(&self.repository)
            .map_err(|e| CoreError::PbsCommand(e.to_string()))?;
        if parts.api_token_secret().trim().is_empty() {
            return Err(CoreError::PbsCommand(
                "repository must include an API token (user@host!token@datastore)".into(),
            ));
        }
        Ok(())
    }

    pub fn from_profile(db: &Database, profile: &BackupProfile) -> Result<Self> {
        let redacted = redact_profile_for_client(profile.clone());
        let repository = repository_for_export(redacted.id, &redacted.repository)?;
        let encryption_key_name = profile
            .encryption_key_id
            .and_then(|id| db.get_encryption_key(id).ok().map(|k| k.name));
        Ok(Self {
            name: redacted.name,
            enabled: redacted.enabled,
            repository,
            namespace: redacted.namespace,
            backup_id: redacted.backup_id,
            paths: redacted.paths,
            excludes: redacted.excludes,
            schedule: redacted.schedule,
            conditions: redacted.conditions,
            health_check: redacted.health_check,
            encryption_key_name,
            server_fingerprint: redacted.server_fingerprint,
        })
    }
}

/// When updating, keep the stored API token if the YAML omits it.
pub fn merge_repository_for_update(profile_id: i64, repository: &str) -> Result<String> {
    let parts = PbsRepositoryParts::parse(repository)
        .map_err(|e| CoreError::PbsCommand(e.to_string()))?;
    if !parts.api_token_secret().trim().is_empty() {
        return Ok(repository.to_string());
    }
    if has_api_token(profile_id) {
        let hydrated = hydrate_profile_repository(profile_id, repository)?;
        return Ok(hydrated);
    }
    if let Some(token) = load_stored_api_token(profile_id) {
        let mut merged = parts;
        merged.token = token;
        return Ok(format_legacy_repository(&merged));
    }
    Err(CoreError::PbsCommand(
        "repository in YAML has no API token and none is stored for this profile".into(),
    ))
}

pub fn parse_profile_yaml(yaml: &str) -> Result<ProfileYaml> {
    ProfileYaml::parse_str(yaml)
}

pub fn profile_to_yaml(db: &Database, profile: &BackupProfile) -> Result<String> {
    ProfileYaml::from_profile(db, profile)?.to_yaml_string()
}

pub fn resolve_encryption_key_name(
    db: &Database,
    name: Option<&str>,
) -> Result<Option<i64>> {
    let Some(name) = name.filter(|n| !n.trim().is_empty()) else {
        return Ok(None);
    };
    let keys = db.list_encryption_keys()?;
    keys.into_iter()
        .find(|k| k.name.eq_ignore_ascii_case(name.trim()))
        .map(|k| Ok(Some(k.id)))
        .unwrap_or_else(|| {
            Err(CoreError::PbsCommand(format!(
                "encryption key not found: {name}"
            )))
        })
}

fn repository_for_export(profile_id: i64, repository: &str) -> Result<String> {
    let parts = PbsRepositoryParts::parse(repository)
        .map_err(|e| CoreError::PbsCommand(e.to_string()))?;
    if parts.api_token_secret().trim().is_empty() {
        if has_api_token(profile_id) {
            let mut redacted = parts;
            redacted.token.clear();
            return Ok(format_legacy_repository(&redacted));
        }
    }
    Ok(format_legacy_repository(&parts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ScheduleType;

    #[test]
    fn parses_schema_example() {
        let yaml = r#"
name: Home-Verzeichnis
enabled: true
repository: pbs-user@pbs!token@pbs.example.ch:datastore
namespace: linux-clients
backup_id: michael-laptop
paths:
  - /home/michael/Dokumente
schedule:
  type: daily
  time: "12:00"
conditions:
  execution_context: host_cli
"#;
        let doc = ProfileYaml::parse_str(yaml).unwrap();
        assert_eq!(doc.name, "Home-Verzeichnis");
        assert_eq!(doc.schedule.schedule_type, ScheduleType::Daily);
        assert_eq!(
            doc.conditions.execution_context,
            crate::profile::ExecutionContext::HostCli
        );
    }
}
