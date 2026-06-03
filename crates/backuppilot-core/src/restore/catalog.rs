//! Parse `proxmox-backup-client catalog dump` output into a browsable tree.

use std::collections::BTreeMap;

use tracing::debug;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogLine {
    /// PBS archive name, e.g. `Dokumente.pxar` (from BackupPilot path backups).
    pub archive: String,
    /// Path inside that archive (no leading slash).
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CatalogEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub archive: String,
}

/// Parses catalog dump stdout/stderr (may contain multiple archives).
pub fn parse_catalog_dump(output: &str, fallback_archive: &str) -> Vec<CatalogLine> {
    let mut by_key: BTreeMap<(String, String), bool> = BTreeMap::new();
    let mut unparsed_lines = 0usize;

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((kind, raw_path)) = split_catalog_line(line) else {
            unparsed_lines += 1;
            continue;
        };
        let Some((archive, rel)) = split_archive_and_path(raw_path, fallback_archive) else {
            continue;
        };
        if rel.is_empty() {
            continue;
        }
        let is_dir = matches!(kind, 'd' | 'D');
        let key = (archive, rel);
        by_key
            .entry(key)
            .and_modify(|dir| *dir = *dir || is_dir)
            .or_insert(is_dir);
    }

    // Infer parent directories within each archive.
    let keys: Vec<(String, String)> = by_key.keys().cloned().collect();
    for (archive, path) in keys {
        let mut parent = path.as_str();
        while let Some((p, _)) = parent.rsplit_once('/') {
            if p.is_empty() {
                break;
            }
            by_key
                .entry((archive.clone(), p.to_string()))
                .or_insert(true);
            parent = p;
        }
    }

    let lines: Vec<CatalogLine> = by_key
        .into_iter()
        .map(|((archive, path), is_dir)| CatalogLine {
            archive,
            path,
            is_dir,
        })
        .collect();

    debug!(
        fallback_archive,
        entries = lines.len(),
        unparsed_lines,
        archives = ?archives_in_lines(&lines),
        "parsed catalog dump"
    );

    lines
}

/// Archive that appears most often in catalog lines (for default restore target).
pub fn primary_archive(lines: &[CatalogLine]) -> Option<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in lines {
        *counts.entry(line.archive.clone()).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(archive, _)| archive)
}

pub fn archives_in_lines(lines: &[CatalogLine]) -> Vec<String> {
    let mut names: Vec<String> = lines
        .iter()
        .map(|l| l.archive.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    names.sort();
    names
}

/// Lists direct children of `parent_path` within `active_archive`.
pub fn list_children(
    lines: &[CatalogLine],
    parent_path: &str,
    active_archive: &str,
) -> Vec<CatalogEntry> {
    let parent = normalize_parent(parent_path);
    let parent_prefix = if parent.is_empty() {
        String::new()
    } else {
        format!("{parent}/")
    };

    let mut children: BTreeMap<String, CatalogEntry> = BTreeMap::new();

    for line in lines {
        if line.archive != active_archive {
            continue;
        }
        if is_pxar_internal_catalog_path(&line.path) {
            continue;
        }
        let path = &line.path;
        if !path.starts_with(&parent_prefix) {
            continue;
        }
        let rest = &path[parent_prefix.len()..];
        if rest.is_empty() {
            continue;
        }
        let (name, remainder) = match rest.split_once('/') {
            Some((n, rem)) => (n.to_string(), Some(rem)),
            None => (rest.to_string(), None),
        };
        if name.is_empty() || is_pxar_internal_catalog_name(&name) {
            continue;
        }
        let child_path = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent}/{name}")
        };
        let is_dir = remainder.is_some() || line.is_dir;
        children
            .entry(name.clone())
            .and_modify(|e| e.is_dir = e.is_dir || is_dir)
            .or_insert(CatalogEntry {
                name,
                path: child_path,
                is_dir,
                archive: line.archive.clone(),
            });
    }

    let mut out: Vec<CatalogEntry> = children.into_values().collect();
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

fn normalize_parent(parent_path: &str) -> String {
    parent_path.trim().trim_matches('/').to_string()
}

/// Chunk/index paths from pxar storage — not user files (must not appear in browse UI).
pub fn is_pxar_internal_catalog_name(name: &str) -> bool {
    name.ends_with(".didx")
        || name.ends_with(".fidx")
        || name.ends_with(".pxar.didx")
}

pub fn is_pxar_internal_catalog_path(path: &str) -> bool {
    path.split('/')
        .any(|segment| is_pxar_internal_catalog_name(segment))
}

fn split_catalog_line(line: &str) -> Option<(char, &str)> {
    let mut s = line.trim();
    for prefix in ["INFO:", "INFO ", "WARN:", "WARN ", "ERROR:", "ERROR "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim();
        }
    }
    if let Some(pos) = s.find(" d ") {
        s = &s[pos + 1..];
    } else if let Some(pos) = s.find(" f ") {
        s = &s[pos + 1..];
    } else if let Some(pos) = s.find(" l ") {
        s = &s[pos + 1..];
    }

    let kind = s.chars().next()?;
    if !matches!(kind, 'd' | 'f' | 'l' | '-' | 'D' | 'F' | 'L') {
        return None;
    }
    if s.len() < 2 {
        return None;
    }
    let rest = s[1..].trim_start();
    let path = extract_path_token(rest)?;
    if path.is_empty() {
        return None;
    }
    Some((kind, path))
}

fn extract_path_token(s: &str) -> Option<&str> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(&rest[..end]);
    }
    if let Some(rest) = s.strip_prefix('\'') {
        let end = rest.find('\'')?;
        return Some(&rest[..end]);
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    Some(s[..end].trim())
}

/// Splits `./Dokumente.pxar.didx/home/user` → (`Dokumente.pxar`, `home/user`).
fn split_archive_and_path(raw: &str, fallback_archive: &str) -> Option<(String, String)> {
    let mut path = raw.trim().trim_matches('"').trim_matches('\'');
    if let Some(stripped) = path.strip_prefix("./") {
        path = stripped;
    }

    if let Some(pos) = path.find(".didx/") {
        let archive = path[..pos].trim_matches('/');
        let rel = path[pos + ".didx/".len()..].trim_matches('/');
        if archive.is_empty() || rel.is_empty() {
            return None;
        }
        return Some((archive.to_string(), rel.to_string()));
    }

    let rel = path.trim_matches('/');
    if rel.is_empty() {
        return None;
    }
    let archive = if fallback_archive.is_empty() {
        "root.pxar".to_string()
    } else {
        fallback_archive.to_string()
    };
    Some((archive, rel.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_per_path_archive_names() {
        let dump = r#"
d "./Dokumente.pxar.didx/home/user"
f "./Dokumente.pxar.didx/home/user/doc.pdf"
d "./Bilder.pxar.didx/photos"
"#;
        let lines = parse_catalog_dump(dump, "");
        let doc_lines: Vec<_> = lines
            .iter()
            .filter(|l| l.archive == "Dokumente.pxar")
            .collect();
        assert!(!doc_lines.is_empty());
        let root = list_children(&lines, "", "Dokumente.pxar");
        assert!(root.iter().any(|e| e.name == "home"));
    }

    #[test]
    fn parses_standard_didx_dump() {
        let dump = r#"
d "./root.pxar.didx/home"
f "./root.pxar.didx/home/user/doc.pdf"
"#;
        let lines = parse_catalog_dump(dump, "root.pxar");
        let root = list_children(&lines, "", "root.pxar");
        assert!(root.iter().any(|e| e.name == "home"));
        assert_eq!(root[0].archive, "root.pxar");
    }

    #[test]
    fn hides_pxar_chunk_names_in_browser() {
        let lines = vec![
            CatalogLine {
                archive: "Temp.pxar".into(),
                path: "10.pdf".into(),
                is_dir: false,
            },
            CatalogLine {
                archive: "Temp.pxar".into(),
                path: "Temp.pxar.didx".into(),
                is_dir: true,
            },
        ];
        let root = list_children(&lines, "", "Temp.pxar");
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].name, "10.pdf");
    }

    #[test]
    fn primary_archive_picks_most_common() {
        let lines = vec![
            CatalogLine {
                archive: "A.pxar".into(),
                path: "x".into(),
                is_dir: false,
            },
            CatalogLine {
                archive: "A.pxar".into(),
                path: "y".into(),
                is_dir: false,
            },
            CatalogLine {
                archive: "B.pxar".into(),
                path: "z".into(),
                is_dir: false,
            },
        ];
        assert_eq!(primary_archive(&lines).as_deref(), Some("A.pxar"));
    }
}
