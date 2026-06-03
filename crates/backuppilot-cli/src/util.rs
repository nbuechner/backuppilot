//! Shared helpers for CLI commands.

use std::sync::{Arc, OnceLock};

use backuppilot_core::{
    hydrate_profile_repository, resolve_encryption_key_id_for_snapshot, resolve_profile_id,
    BackupProfile, Database, EncryptionKey, MountManager, PbsSnapshotInfo,
};

static MOUNT_MANAGER: OnceLock<Arc<MountManager>> = OnceLock::new();

pub fn mount_manager() -> Arc<MountManager> {
    MOUNT_MANAGER
        .get_or_init(|| Arc::new(MountManager::new()))
        .clone()
}

pub fn open_db() -> Result<Database, String> {
    Database::open().map_err(|e| e.to_string())
}

pub async fn load_profile(db: &Database, profile_id: i64) -> Result<BackupProfile, String> {
    let mut profile = db.get_profile(profile_id).map_err(|e| e.to_string())?;
    profile.repository =
        hydrate_profile_repository(profile.id, &profile.repository).map_err(|e| e.to_string())?;
    Ok(profile)
}

pub async fn profile_by_name_or_id(db: &Database, name_or_id: &str) -> Result<BackupProfile, String> {
    let id = resolve_profile_id(db, name_or_id).map_err(|e| e.to_string())?;
    load_profile(db, id).await
}

pub fn encryption_key_for_snapshot(
    db: &Database,
    profile: &BackupProfile,
    snapshot: &PbsSnapshotInfo,
) -> Option<i64> {
    let keys: Vec<EncryptionKey> = db.list_encryption_keys().ok()?;
    resolve_encryption_key_id_for_snapshot(profile, snapshot, &keys)
}

pub async fn snapshot_encryption_key(
    db: &Database,
    profile: &BackupProfile,
    snapshot_path: &str,
) -> Result<Option<i64>, String> {
    use backuppilot_core::PbsRestore;

    let snapshots = PbsRestore::list_snapshots(profile)
        .await
        .map_err(|e| e.to_string())?;
    let snap = snapshots
        .iter()
        .find(|s| s.path == snapshot_path)
        .ok_or_else(|| format!("snapshot not found: {snapshot_path}"))?;
    Ok(encryption_key_for_snapshot(db, profile, snap))
}
