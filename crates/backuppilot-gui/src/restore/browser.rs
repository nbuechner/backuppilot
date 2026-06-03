//! File browser rows and restore pattern helpers.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use backuppilot_core::{backup_archive_name, CatalogEntry, PbsSnapshotInfo};
use backuppilot_i18n::{tr, tr_fmt};
use gtk::prelude::*;
use libadwaita::prelude::*;

pub const ROW_CHECK_NAME: &str = "restore-item-check";
pub const ARCHIVE_EXPAND_NAME: &str = "restore-archive-expand";
pub const ARCHIVE_MOUNT_BTN: &str = "restore-archive-mount";
pub const ARCHIVE_ROW_PREFIX: &str = "arc|";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveMountBtnState {
    Mount,
    Mounted,
    Mounting,
}

/// Top-level PBS archive matching a profile backup path (e.g. `/home` → `home.pxar`).
#[derive(Debug, Clone)]
pub struct ArchiveBrowseItem {
    pub archive_name: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseMode {
    /// All backup paths / `.pxar` archives in the snapshot.
    Archives,
    /// Browsing files inside one archive.
    Inside,
}

pub fn archive_row_key(archive: &str) -> String {
    format!("{ARCHIVE_ROW_PREFIX}{archive}")
}

pub fn archive_from_row_name(name: &str) -> Option<String> {
    name.strip_prefix(ARCHIVE_ROW_PREFIX).map(str::to_string)
}

/// Merges profile backup paths with archives reported for the snapshot.
pub fn resolve_archive_items(
    snap: &PbsSnapshotInfo,
    profile_paths: &[String],
) -> Vec<ArchiveBrowseItem> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    for (index, path) in profile_paths.iter().enumerate() {
        let archive_name = backup_archive_name(path, index);
        if seen.insert(archive_name.clone()) {
            items.push(ArchiveBrowseItem {
                archive_name,
                source_path: path.clone(),
            });
        }
    }

    for archive_name in snap
        .archives
        .iter()
        .filter(|a| a.ends_with(".pxar"))
    {
        if seen.insert(archive_name.clone()) {
            items.push(ArchiveBrowseItem {
                archive_name: archive_name.clone(),
                source_path: archive_name_to_source_label(archive_name),
            });
        }
    }

    items.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    items
}

fn archive_name_to_source_label(archive_name: &str) -> String {
    let stem = archive_name.strip_suffix(".pxar").unwrap_or(archive_name);
    format!("/{stem}")
}

pub fn build_archive_row(
    item: &ArchiveBrowseItem,
    expanded: bool,
    preview_loading: bool,
    mount_state: ArchiveMountBtnState,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&archive_row_key(&item.archive_name));

    let expand = gtk::ToggleButton::builder()
        .icon_name(if expanded {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        })
        .tooltip_text(&tr("Expand to preview top-level folders"))
        .css_classes(["flat"])
        .active(expanded)
        .valign(gtk::Align::Center)
        .build();
    expand.set_widget_name(ARCHIVE_EXPAND_NAME);

    let subtitle = if preview_loading {
        tr("Loading…")} else if expanded {
        tr("Archive , double-click to open, - to collapse preview")} else {
        tr_fmt("Archive {name}", &[("name", &item.archive_name)])
    };

    let action = libadwaita::ActionRow::builder()
        .title(&item.source_path)
        .subtitle(&subtitle)
        .activatable(true)
        .build();
    action.add_prefix(&expand);
    action.add_prefix(
        &gtk::Image::builder()
            .icon_name("drive-harddisk-symbolic")
            .pixel_size(16)
            .build(),
    );

    let (mount_icon, mount_tooltip, sensitive) = match mount_state {
        ArchiveMountBtnState::Mount => (
            "folder-visiting-symbolic",
            tr("Mount read-only in file manager…"),
            true,
        ),
        ArchiveMountBtnState::Mounted => (
            "emblem-ok-symbolic",
            tr("Mounted , click to open in file manager"),
            true,
        ),
        ArchiveMountBtnState::Mounting => (
            "content-loading-symbolic",
            tr("Mounting…"),
            false,
        ),
    };
    let mount_btn = gtk::Button::builder()
        .icon_name(mount_icon)
        .tooltip_text(&mount_tooltip)
        .css_classes(["flat"])
        .sensitive(sensitive)
        .valign(gtk::Align::Center)
        .build();
    mount_btn.set_widget_name(ARCHIVE_MOUNT_BTN);
    action.add_suffix(&mount_btn);

    row.set_child(Some(&action));
    row
}

pub fn build_archive_preview_row(entry: &CatalogEntry, selected: bool) -> gtk::ListBoxRow {
    let row = build_file_row(entry, selected);
    row.add_css_class("restore-archive-preview");
    if let Some(child) = row.child() {
        child.set_margin_start(28);
    }
    row
}

pub fn catalog_row_key(archive: &str, path: &str) -> String {
    format!("cat|{archive}|{path}")
}

pub fn build_file_row(entry: &CatalogEntry, selected: bool) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&catalog_row_key(&entry.archive, &entry.path));

    let check = gtk::CheckButton::builder()
        .active(selected)
        .valign(gtk::Align::Center)
        .tooltip_text(if entry.is_dir {
            tr("Restore this folder and everything inside it")} else {
            tr("Restore this file")})
        .build();
    check.set_widget_name(ROW_CHECK_NAME);

    let icon = if entry.is_dir {
        "folder-symbolic"
    } else {
        "text-x-generic-symbolic"
    };

    let subtitle = if entry.is_dir {
        tr("Folder , double-click to open, checkbox to restore all contents")} else if entry.path.is_empty() {
        tr("File")} else {
        entry.path.clone()
    };
    let action = libadwaita::ActionRow::builder()
        .title(&entry.name)
        .subtitle(&subtitle)
        .activatable(false)
        .build();
    action.add_prefix(&check);
    action.add_prefix(
        &gtk::Image::builder()
            .icon_name(icon)
            .pixel_size(16)
            .build(),
    );
    row.set_child(Some(&action));
    row
}

pub fn row_check_button(row: &gtk::ListBoxRow) -> Option<gtk::CheckButton> {
    crate::util::find_child_by_name(row.upcast_ref(), ROW_CHECK_NAME)
        .and_then(|w| w.downcast::<gtk::CheckButton>().ok())
}

pub fn build_parent_row() -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name("catalog-parent");
    let action = libadwaita::ActionRow::builder()
        .title("..")
        .subtitle(&tr("Parent folder"))
        .activatable(true)
        .build();
    action.add_prefix(
        &gtk::Image::builder()
            .icon_name("go-up-symbolic")
            .pixel_size(16)
            .build(),
    );
    row.set_child(Some(&action));
    row
}

pub fn parent_path(current: &str) -> Option<String> {
    let current = current.trim().trim_matches('/');
    if current.is_empty() {
        return None;
    }
    current
        .rsplit_once('/')
        .map(|(p, _)| p.to_string())
        .or(Some(String::new()))
}

pub fn patterns_for_entries(entries: &[CatalogEntry]) -> Vec<String> {
    let mut patterns = Vec::new();
    for entry in entries {
        if entry.is_dir {
            patterns.push(format!("{}/**", entry.path));
        } else {
            patterns.push(entry.path.clone());
        }
    }
    patterns
}

pub fn filter_entries(entries: &[CatalogEntry], query: &str) -> Vec<CatalogEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return entries.to_vec();
    }
    entries
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&q)
                || e.path.to_lowercase().contains(&q)
                || e.archive.to_lowercase().contains(&q)
        })
        .cloned()
        .collect()
}

pub fn selected_catalog_entries(selected: &HashMap<String, CatalogEntry>) -> Vec<CatalogEntry> {
    selected.values().cloned().collect()
}

pub fn select_entries_in_view(
    selected: &mut HashMap<String, CatalogEntry>,
    entries: &[CatalogEntry],
) {
    for entry in entries {
        selected.insert(catalog_row_key(&entry.archive, &entry.path), entry.clone());
    }
}

pub fn format_browse_path(mode: BrowseMode, archive: Option<&str>, path: &str) -> String {
    match mode {
        BrowseMode::Archives => tr("Backup paths in this snapshot"),
        BrowseMode::Inside if path.is_empty() => {
            if let Some(arch) = archive {
                tr_fmt("Inside archive {name}", &[("name", arch)])
            } else {
                tr("/ (archive root)")}
        }
        BrowseMode::Inside => format!("/{path}"),
    }
}

pub fn snapshot_display_name(snapshot_path: &str) -> String {
    snapshot_path
        .rsplit('/')
        .next()
        .unwrap_or(snapshot_path)
        .to_string()
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.2} GiB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MiB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KiB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

pub fn set_row_checked(row: &gtk::ListBoxRow, active: bool) {
    if let Some(check) = row_check_button(row) {
        check.set_active(active);
    }
}

pub fn sync_visible_checks(
    list: &gtk::ListBox,
    selected: &HashMap<String, CatalogEntry>,
    suppress_toggled: &Cell<bool>,
) {
    suppress_toggled.set(true);
    let mut i = 0;
    while let Some(row) = list.row_at_index(i) {
        i += 1;
        if row.widget_name().as_str() == "catalog-parent" {
            continue;
        }
        let key = row.widget_name().to_string();
        set_row_checked(&row, selected.contains_key(&key));
    }
    suppress_toggled.set(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_pattern_includes_children() {
        let entries = vec![CatalogEntry {
            name: "docs".into(),
            path: "docs".into(),
            archive: "home.pxar".into(),
            is_dir: true,
        }];
        let patterns = patterns_for_entries(&entries);
        assert_eq!(patterns, vec!["docs/**"]);
    }
}

pub fn dedupe_patterns(patterns: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for p in patterns {
        if seen.insert(p.clone()) {
            out.push(p);
        }
    }
    out
}
