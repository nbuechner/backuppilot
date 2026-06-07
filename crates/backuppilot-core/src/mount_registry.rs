//! Persisted mount metadata so CLI/GUI processes can list and unmount FUSE mounts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::error::{CoreError, Result};
use crate::pbs_mount::{is_mountpoint_active, mounts_base_dir, ActiveMount};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MountRegistryFile {
    mounts: Vec<ActiveMount>,
}

pub fn mount_registry_path() -> PathBuf {
    mounts_base_dir().join("registry.json")
}

pub fn register_mount(mount: &ActiveMount) {
    let mut mounts = load_mount_registry_raw();
    mounts.retain(|m| m.id != mount.id && m.mount_point != mount.mount_point);
    mounts.push(mount.clone());
    let _ = save_mount_registry_raw(&mounts);
}

pub fn unregister_mount(id: &str) {
    let mut mounts = load_mount_registry_raw();
    mounts.retain(|m| m.id != id);
    let _ = save_mount_registry_raw(&mounts);
}

pub fn unregister_mount_point(mount_point: &str) {
    let mut mounts = load_mount_registry_raw();
    mounts.retain(|m| m.mount_point != mount_point);
    let _ = save_mount_registry_raw(&mounts);
}

/// Active FUSE mounts: registry file + scan under `mounts/` (survives new CLI processes).
pub fn list_known_active_mounts() -> Vec<ActiveMount> {
    let mut by_point: HashMap<String, ActiveMount> = HashMap::new();

    for mount in load_mount_registry_raw() {
        let path = Path::new(&mount.mount_point);
        if is_mountpoint_active(path) {
            by_point.insert(mount.mount_point.clone(), mount);
        }
    }

    for path in scan_active_mount_directories() {
        let key = path.to_string_lossy().into_owned();
        if by_point.contains_key(&key) {
            continue;
        }
        if let Some(mount) = synthetic_mount_for_path(&path) {
            by_point.insert(key, mount);
        }
    }

    let mut active: Vec<ActiveMount> = by_point.into_values().collect();
    active.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    let _ = save_mount_registry_raw(&active);
    active
}

pub fn prune_mount_registry() {
    let active = list_known_active_mounts();
    let _ = save_mount_registry_raw(&active);
}

pub fn mount_id_from_path(path: &Path) -> String {
    format!("path:{}", path.display())
}

pub fn mount_point_from_id(mount_id: &str) -> Option<PathBuf> {
    mount_id.strip_prefix("path:").map(PathBuf::from)
}

/// Resolves the on-disk path for a mount id (registry first, then `path:` / slug fallback).
pub fn resolve_mount_path(mount_id: &str) -> Option<PathBuf> {
    load_mount_registry_raw()
        .into_iter()
        .find(|m| m.id == mount_id)
        .map(|m| PathBuf::from(m.mount_point))
        .or_else(|| mount_point_from_id(mount_id))
}

fn load_mount_registry_raw() -> Vec<ActiveMount> {
    let path = mount_registry_path();
    if !path.exists() {
        return Vec::new();
    }
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<MountRegistryFile>(&data)
        .map(|f| f.mounts)
        .unwrap_or_else(|err| {
            warn!(%err, "could not parse mount registry");
            Vec::new()
        })
}

fn save_mount_registry_raw(mounts: &[ActiveMount]) -> Result<()> {
    let path = mount_registry_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(CoreError::Io)?;
    }
    let file = MountRegistryFile {
        mounts: mounts.to_vec(),
    };
    let json = serde_json::to_string_pretty(&file).map_err(|e| CoreError::PbsCommand(e.to_string()))?;
    std::fs::write(&path, json).map_err(CoreError::Io)
}

fn scan_active_mount_directories() -> Vec<PathBuf> {
    let base = mounts_base_dir();
    if !base.is_dir() {
        return Vec::new();
    }
    let Ok(profiles) = std::fs::read_dir(&base) else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    for profile_entry in profiles.flatten() {
        let profile_path = profile_entry.path();
        if !profile_path.is_dir() {
            continue;
        }
        let Ok(snaps) = std::fs::read_dir(&profile_path) else {
            continue;
        };
        for snap_entry in snaps.flatten() {
            let snap_path = snap_entry.path();
            if !snap_path.is_dir() {
                continue;
            }
            let Ok(archives) = std::fs::read_dir(&snap_path) else {
                continue;
            };
            for arch_entry in archives.flatten() {
                let mount_path = arch_entry.path();
                if mount_path.is_dir() && is_mountpoint_active(&mount_path) {
                    paths.push(mount_path);
                }
            }
        }
    }
    paths
}

fn synthetic_mount_for_path(path: &Path) -> Option<ActiveMount> {
    let arch_slug = path.file_name()?.to_str()?;
    let snap_path = path.parent()?;
    let snap_slug = snap_path.file_name()?.to_str()?;
    let profile_path = snap_path.parent()?;
    let profile_id: i64 = profile_path.file_name()?.to_str()?.parse().ok()?;

    let archive_name = if arch_slug.ends_with(".pxar") {
        arch_slug.to_string()
    } else {
        format!("{arch_slug}.pxar")
    };

    let snapshot = snap_slug.replace('_', "/");
    let id = mount_id_from_path(path);

    Some(ActiveMount {
        id,
        profile_id,
        profile_name: format!("Profil {profile_id}"),
        snapshot,
        archive_name,
        source_label: mount_point_label(arch_slug),
        mount_point: path.display().to_string(),
        started_at: chrono::Utc::now(),
        windows_path: {
            #[cfg(windows)]
            { crate::pbs_mount::wsl_path_to_unc(path) }
            #[cfg(not(windows))]
            { None }
        },
    })
}

fn mount_point_label(arch_slug: &str) -> String {
    format!("/{arch_slug}")
}
