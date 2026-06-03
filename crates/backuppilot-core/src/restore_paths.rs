//! Map PBS archive paths back to original backup source directories.

use std::path::{Path, PathBuf};

use crate::pbs::backup_archive_name;

/// Resolves the on-disk directory that matches a PBS archive name from profile backup paths.
pub fn archive_source_root(profile_paths: &[String], archive_name: &str) -> Option<PathBuf> {
    let archive_name = archive_name.trim();
    if archive_name.is_empty() {
        return None;
    }
    for (index, path) in profile_paths.iter().enumerate() {
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        if backup_archive_name(path, index) == archive_name {
            return Some(PathBuf::from(path));
        }
        let stem = archive_name.strip_suffix(".pxar").unwrap_or(archive_name);
        if Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n == stem)
        {
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// Target directory for restoring under the original backup location.
pub fn original_restore_target_dir(
    profile_paths: &[String],
    archive_name: &str,
    catalog_rel_path: &str,
) -> Option<PathBuf> {
    let mut base = archive_source_root(profile_paths, archive_name)?;
    let rel = catalog_rel_path.trim().trim_start_matches('/');
    if rel.is_empty() {
        return Some(base);
    }
    let rel_path = Path::new(rel);
    if rel_path.file_name().is_some() && !rel.ends_with('/') {
        if let Some(parent) = rel_path.parent() {
            if !parent.as_os_str().is_empty() {
                base.push(parent);
            }
        }
    } else {
        base.push(rel_path);
    }
    Some(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_archive_to_profile_path() {
        let paths = vec!["/home/user/Dokumente".into()];
        let root = archive_source_root(&paths, "Dokumente.pxar").unwrap();
        assert_eq!(root, PathBuf::from("/home/user/Dokumente"));
    }

    #[test]
    fn original_target_includes_parent_dir() {
        let paths = vec!["/home/user/Dokumente".into()];
        let target =
            original_restore_target_dir(&paths, "Dokumente.pxar", "reports/2024/file.pdf").unwrap();
        assert_eq!(target, PathBuf::from("/home/user/Dokumente/reports/2024"));
    }
}
