//! Tracks active `proxmox-backup-client mount` FUSE sessions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::mount_registry::{
    list_known_active_mounts, prune_mount_registry, register_mount, resolve_mount_path,
    unregister_mount, unregister_mount_point,
};
use crate::pbs_mount::{
    fuse_available, force_unmount, is_mountpoint_active, mount_point_for, mount_session_id,
    spawn_mount_process, wait_mount_ready, ActiveMount, MountSnapshotRequest, MountSnapshotResult,
    UnmountSnapshotResult,
};
use crate::CoreError;
use crate::profile::BackupProfile;
use crate::Result;

pub struct MountManager {
    sessions: Mutex<HashMap<String, MountSession>>,
}

struct MountSession {
    info: ActiveMount,
    child: Child,
}

impl MountManager {
    pub fn new() -> Self {
        prune_mount_registry();
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub async fn list_active(&self) -> Vec<ActiveMount> {
        self.sync_dead_sessions().await;
        let mut by_id: HashMap<String, ActiveMount> = HashMap::new();
        for session in self.sessions.lock().await.values() {
            by_id.insert(session.info.id.clone(), session.info.clone());
        }
        for mount in list_known_active_mounts() {
            by_id.entry(mount.id.clone()).or_insert(mount);
        }
        by_id.into_values().collect()
    }

    pub async fn mount(
        &self,
        profile: &BackupProfile,
        request: &MountSnapshotRequest,
        encryption_key_id: Option<i64>,
    ) -> Result<MountSnapshotResult> {
        if !fuse_available() {
            return Ok(MountSnapshotResult {
                ok: false,
                message: Some(
                    "FUSE is not available (install fuse3, user must be allowed to use FUSE)."
                        .into(),
                ),
                mount: None,
            });
        }

        let id = mount_session_id(request.profile_id, &request.snapshot, &request.archive_name);
        {
            let guard = self.sessions.lock().await;
            if let Some(existing) = guard.get(&id) {
                return Ok(MountSnapshotResult {
                    ok: true,
                    message: None,
                    mount: Some(existing.info.clone()),
                });
            }
        }

        let mount_point = mount_point_for(
            request.profile_id,
            &request.snapshot,
            &request.archive_name,
        );
        let mut child = spawn_mount_process(
            profile,
            &request.snapshot,
            &request.archive_name,
            &mount_point,
            encryption_key_id,
        )
        .await?;

        if !wait_mount_ready(&mount_point, 50) {
            let _ = child.kill().await;
            let _ = force_unmount(&mount_point);
            return Ok(MountSnapshotResult {
                ok: false,
                message: Some(
                    "Mount did not become ready in time. Check PBS reachability and encryption key."
                        .into(),
                ),
                mount: None,
            });
        }

        let source_label = if request.source_label.is_empty() {
            request.archive_name.clone()
        } else {
            request.source_label.clone()
        };
        let info = ActiveMount {
            id: id.clone(),
            profile_id: request.profile_id,
            profile_name: profile.name.clone(),
            snapshot: request.snapshot.clone(),
            archive_name: request.archive_name.clone(),
            source_label,
            mount_point: mount_point.display().to_string(),
            started_at: Utc::now(),
        };

        self.sessions
            .lock()
            .await
            .insert(id.clone(), MountSession { info: info.clone(), child });

        register_mount(&info);

        info!(
            mount_id = %id,
            mount_point = %info.mount_point,
            profile = %profile.name,
            archive = %request.archive_name,
            "snapshot archive mounted via FUSE"
        );

        Ok(MountSnapshotResult {
            ok: true,
            message: None,
            mount: Some(info),
        })
    }

    pub async fn unmount(&self, mount_id: &str) -> Result<UnmountSnapshotResult> {
        let path = resolve_mount_path(mount_id).or_else(|| self.guess_mount_point_from_id(mount_id));

        if let Some(mut session) = self.sessions.lock().await.remove(mount_id) {
            let session_path = PathBuf::from(&session.info.mount_point);
            let _ = session.child.kill().await;
            let unmount_result = force_unmount(&session_path);
            unregister_mount(mount_id);
            unregister_mount_point(&session.info.mount_point);
            return finish_unmount(mount_id, &session_path, unmount_result);
        }

        if let Some(path) = path {
            let unmount_result = force_unmount(&path);
            unregister_mount(mount_id);
            unregister_mount_point(&path.to_string_lossy());
            return finish_unmount(mount_id, &path, unmount_result);
        }

        unregister_mount(mount_id);
        Ok(UnmountSnapshotResult {
            ok: true,
            message: None,
        })
    }

    pub async fn unmount_all(&self) -> Result<()> {
        let mounts = self.list_active().await;
        let mut failures = Vec::new();

        for mount in mounts {
            match self.unmount(&mount.id).await {
                Ok(result) if result.ok => {}
                Ok(result) => failures.push(
                    result
                        .message
                        .unwrap_or_else(|| format!("{}: unbekannter Fehler", mount.mount_point)),
                ),
                Err(err) => failures.push(format!("{}: {err}", mount.mount_point)),
            }
        }

        self.sessions.lock().await.clear();

        if failures.is_empty() {
            Ok(())
        } else {
            Err(CoreError::PbsCommand(format!(
                "Nicht alle Mounts konnten getrennt werden: {}",
                failures.join("; ")
            )))
        }
    }

    pub async fn unmount_profile(&self, profile_id: i64) -> Result<()> {
        let ids: Vec<String> = self
            .sessions
            .lock()
            .await
            .values()
            .filter(|s| s.info.profile_id == profile_id)
            .map(|s| s.info.id.clone())
            .collect();
        for id in ids {
            let _ = self.unmount(&id).await;
        }
        Ok(())
    }

    fn guess_mount_point_from_id(&self, mount_id: &str) -> Option<PathBuf> {
        let parts: Vec<&str> = mount_id.splitn(3, '|').collect();
        if parts.len() != 3 {
            return None;
        }
        let profile_id: i64 = parts[0].parse().ok()?;
        Some(mount_point_for(profile_id, parts[1], parts[2]))
    }

    async fn sync_dead_sessions(&self) {
        let mut guard = self.sessions.lock().await;
        let mut dead = Vec::new();
        for (id, session) in guard.iter_mut() {
            match session.child.try_wait() {
                Ok(Some(_)) => dead.push(id.clone()),
                Ok(None) => {
                    let path = PathBuf::from(&session.info.mount_point);
                    if !is_mountpoint_active(&path) {
                        dead.push(id.clone());
                    }
                }
                Err(_) => dead.push(id.clone()),
            }
        }
        for id in dead {
            if let Some(session) = guard.remove(&id) {
                let path = PathBuf::from(&session.info.mount_point);
                let _ = force_unmount(&path);
                warn!(mount_id = %id, "removed stale mount session");
            }
        }
    }

}

fn finish_unmount(
    mount_id: &str,
    path: &Path,
    unmount_result: Result<()>,
) -> Result<UnmountSnapshotResult> {
    match unmount_result {
        Ok(()) if is_mountpoint_active(path) => Ok(UnmountSnapshotResult {
            ok: false,
            message: Some(format!(
                "Mount ist noch aktiv unter {} (ggf. `fusermount -uz {}` manuell).",
                path.display(),
                path.display()
            )),
        }),
        Ok(()) => {
            info!(mount_id, mount_point = %path.display(), "snapshot mount disconnected");
            Ok(UnmountSnapshotResult {
                ok: true,
                message: None,
            })
        }
        Err(err) => Ok(UnmountSnapshotResult {
            ok: false,
            message: Some(err.to_string()),
        }),
    }
}

impl Default for MountManager {
    fn default() -> Self {
        Self::new()
    }
}
