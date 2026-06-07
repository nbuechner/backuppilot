//! Restore page — snapshot browser, file tree, selective restore.

mod browser;
pub mod mount_ui;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

thread_local! {
    static RESTORE_CTX: RefCell<Option<RestoreCtx>> = const { RefCell::new(None) };
}

use backuppilot_core::pbs_mount::mount_session_id;
use backuppilot_core::{
    fingerprints_match, normalize_fingerprint, original_restore_target_dir, ActiveMount, CatalogEntry,
    EncryptionKey, ListCatalogRequest, PbsSnapshotInfo, RestoreArchiveRequest,
};
use backuppilot_i18n::{tr, tr_fmt};
use gtk::glib;
use gtk::prelude::*;
use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, StatusPage, ToastOverlay};

use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::restore::browser::{
    archive_from_row_name, build_archive_preview_row, build_archive_row,
    build_file_row, build_parent_row, catalog_row_key, dedupe_patterns, filter_entries,
    format_browse_path, format_size, parent_path, patterns_for_entries,
    resolve_archive_items, row_check_button, select_entries_in_view, selected_catalog_entries,
    snapshot_display_name, sync_visible_checks, ArchiveBrowseItem, ArchiveMountBtnState,
    BrowseMode, ARCHIVE_EXPAND_NAME, ARCHIVE_MOUNT_BTN,
};
use crate::restore::mount_ui::{
    active_mount_for_archive, confirm_and_mount_archive, mount_state_for_archive,
    open_path_in_file_manager,
};
use crate::util::{clear_list_box, find_child_by_name};
use crate::window;

#[derive(Clone)]
struct RestoreCtx {
    page: gtk::Widget,
    toast: ToastOverlay,
    profile_combo: libadwaita::ComboRow,
    state: Rc<RefCell<PageState>>,
    suppress_check_toggles: Rc<Cell<bool>>,
    suppress_profile_notify: Rc<Cell<bool>>,
}

fn suppress_check_toggles() -> Rc<Cell<bool>> {
    RESTORE_CTX.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|ctx| ctx.suppress_check_toggles.clone())
            .unwrap_or_else(|| Rc::new(Cell::new(false)))
    })
}

struct SnapshotEncryptionInfo {
    encrypted: bool,
    key_name: Option<String>,
}

struct PageState {
    profile_ids: Vec<i64>,
    profile_names: Vec<String>,
    /// PBS fingerprint (normalized) → encryption key display name.
    encryption_key_by_fingerprint: HashMap<String, String>,
    encryption_keys: Vec<EncryptionKey>,
    profile_id: Option<i64>,
    profile_paths: Vec<String>,
    snapshots: Vec<PbsSnapshotInfo>,
    snapshot: Option<String>,
    browse_mode: BrowseMode,
    archive_items: Vec<ArchiveBrowseItem>,
    expanded_archives: HashSet<String>,
    archive_preview: HashMap<String, Vec<CatalogEntry>>,
    archive_preview_loading: HashSet<String>,
    archive: Option<String>,
    browse_path: String,
    catalog_entries: Vec<CatalogEntry>,
    /// Checked items for restore (files and folders); kept across folder navigation.
    selected_entries: HashMap<String, CatalogEntry>,
    catalog_loading: bool,
    restore_in_progress: bool,
    restore_pulse_source: Option<glib::SourceId>,
    /// Ignores stale `load_snapshots` callbacks when requests overlap.
    snapshot_load_seq: u64,
    active_mounts: Vec<ActiveMount>,
    mounting_ids: HashSet<String>,
    /// Whether the current profile has Datastore.Prune or Datastore.Modify permission.
    can_delete: bool,
    /// Whether the current profile has Datastore.Read or Datastore.Modify permission.
    can_restore: bool,
}

pub fn build_page(parent: &ApplicationWindow, toast_overlay: &ToastOverlay) -> gtk::Widget {
    let state = Rc::new(RefCell::new(PageState {
        profile_ids: Vec::new(),
        profile_names: Vec::new(),
        encryption_key_by_fingerprint: HashMap::new(),
        encryption_keys: Vec::new(),
        profile_id: None,
        profile_paths: Vec::new(),
        snapshots: Vec::new(),
        snapshot: None,
        browse_mode: BrowseMode::Archives,
        archive_items: Vec::new(),
        expanded_archives: HashSet::new(),
        archive_preview: HashMap::new(),
        archive_preview_loading: HashSet::new(),
        archive: None,
        browse_path: String::new(),
        catalog_entries: Vec::new(),
        selected_entries: HashMap::new(),
        catalog_loading: false,
        restore_in_progress: false,
        restore_pulse_source: None,
        snapshot_load_seq: 0,
        active_mounts: Vec::new(),
        mounting_ids: HashSet::new(),
        can_delete: false,
        can_restore: true,
    }));
    let suppress_check_toggles = Rc::new(Cell::new(false));
    let suppress_profile_notify = Rc::new(Cell::new(false));

    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(16)
        .margin_end(16)
        .hexpand(true)
        .vexpand(true)
        .build();
    outer.set_widget_name("restore-page");

    let status_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_bottom(4)
        .visible(false)
        .build();
    status_box.set_widget_name("restore-status-box");

    let status_header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let status_spinner = gtk::Spinner::builder().spinning(false).build();
    status_spinner.set_widget_name("restore-status-spinner");
    let status_label = gtk::Label::builder()
        .label(&tr("Restore running…"))
        .css_classes(["title-4"])
        .xalign(0.0)
        .hexpand(true)
        .build();
    status_header.append(&status_spinner);
    status_header.append(&status_label);

    let status_detail = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    status_detail.set_widget_name("restore-status-detail");

    let status_progress = gtk::ProgressBar::builder()
        .pulse_step(0.08)
        .show_text(false)
        .build();
    status_progress.set_widget_name("restore-status-progress");

    status_box.append(&status_header);
    status_box.append(&status_detail);
    status_box.append(&status_progress);
    outer.append(&status_box);

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .resize_start_child(false)
        .resize_end_child(true)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .position(380)
        .hexpand(true)
        .vexpand(true)
        .build();
    paned.set_widget_name("restore-paned");

    // --- Left: profile picker + snapshot list ---
    let source_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Select a snapshot"))
        .build();

    let profile_combo = libadwaita::ComboRow::builder().build();
    profile_combo.set_title("");
    source_group.add(&profile_combo);

    let snapshots_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(["boxed-list"])
        .build();
    snapshots_list.set_widget_name("restore-snapshots-list");

    let snapshots_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .min_content_height(260)
        .min_content_width(300)
        .child(&snapshots_list)
        .build();

    let left_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .vexpand(true)
        .hexpand(true)
        .width_request(320)
        .margin_end(10)
        .build();
    left_panel.append(&source_group);
    left_panel.append(&snapshots_scroll);

    // --- Right: file browser + destination ---
    let right = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .vexpand(true)
        .hexpand(true)
        .width_request(480)
        .margin_start(10)
        .build();

    let back_btn = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text(&tr("Go up"))
        .sensitive(false)
        .build();
    back_btn.set_widget_name("restore-back-btn");

    let path_label = gtk::Label::builder()
        .label(&format_browse_path(BrowseMode::Archives, None, ""))
        .css_classes(["title-4"])
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .build();
    path_label.set_widget_name("restore-path-label");

    let nav_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_bottom(4)
        .build();
    nav_bar.append(&back_btn);
    nav_bar.append(&path_label);

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text(&tr("Search in this folder"))
        .hexpand(true)
        .build();
    search_entry.set_widget_name("restore-search");

    let select_all_btn = gtk::Button::builder()
        .icon_name("edit-select-all-symbolic")
        .tooltip_text(&tr("Check all files and folders in this folder"))
        .css_classes(["flat"])
        .build();
    select_all_btn.set_widget_name("restore-select-all-btn");

    let clear_sel_btn = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text(&tr("Uncheck all selected items"))
        .css_classes(["flat"])
        .build();
    clear_sel_btn.set_widget_name("restore-clear-sel-btn");

    let selection_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["linked"])
        .build();
    selection_box.append(&select_all_btn);
    selection_box.append(&clear_sel_btn);

    let filter_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_bottom(8)
        .build();
    filter_bar.append(&search_entry);
    filter_bar.append(&selection_box);

    let stack = gtk::Stack::builder().vexpand(true).build();
    stack.set_widget_name("restore-browser-stack");

    let files_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    files_list.set_widget_name("restore-file-list");

    let files_scroll = gtk::ScrolledWindow::builder()
        .child(&files_list)
        .vexpand(true)
        .min_content_height(280)
        .min_content_width(400)
        .build();
    stack.add_named(&files_scroll, Some("browser"));

    let files_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .vexpand(true)
        .build();
    files_panel.set_widget_name("restore-files-card");
    files_panel.append(&nav_bar);
    files_panel.append(&filter_bar);
    files_panel.append(&stack);

    let idle_page = StatusPage::builder()
        .icon_name("folder-download-symbolic")
        .title(&tr("Select a snapshot"))
        .description(&tr("Choose a snapshot on the left to browse files and restore individual items."))
        .vexpand(true)
        .valign(gtk::Align::Center)
        .build();

    let browse_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .vexpand(true)
        .build();
    browse_panel.append(
        &gtk::Label::builder()
            .label(&tr("Files in backup"))
            .css_classes(["heading"])
            .xalign(0.0)
            .build(),
    );
    browse_panel.append(&files_panel);

    let right_stack = gtk::Stack::builder().vexpand(true).build();
    right_stack.set_widget_name("restore-right-stack");
    right_stack.add_named(&idle_page, Some("idle"));
    right_stack.add_named(&browse_panel, Some("browse"));
    right_stack.set_visible_child_name("idle");

    let target_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Restore destination"))
        .description(&tr("Local folder where recovered files will be written"))
        .build();

    let target_row = libadwaita::EntryRow::builder()
        .title(&tr("Folder on this computer"))
        .show_apply_button(false)
        .build();
    target_row.set_widget_name("restore-target-entry");

    let browse_btn = gtk::Button::builder()
        .icon_name("folder-symbolic")
        .tooltip_text(&tr("Choose folder…"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    target_row.add_suffix(&browse_btn);

    let overwrite_row = libadwaita::SwitchRow::builder()
        .title(&tr("Overwrite existing files"))
        .subtitle(&tr("Replace files that already exist in the target folder"))
        .active(false)
        .build();

    let original_path_row = libadwaita::SwitchRow::builder()
        .title(&tr("Restore to original location"))
        .subtitle(&tr("Use the backup source path that matches the selected archive (when available)"))
        .active(false)
        .build();
    original_path_row.set_widget_name("restore-original-path");

    target_group.add(&target_row);
    target_group.add(&original_path_row);
    target_group.add(&overwrite_row);

    let restore_full_btn = gtk::Button::builder()
        .label(&tr("Restore entire archive"))
        .build();
    restore_full_btn.set_widget_name("restore-full-btn");

    let restore_selected_btn = gtk::Button::builder()
        .label(&tr("Restore selected"))
        .css_classes(["suggested-action"])
        .sensitive(false)
        .build();
    restore_selected_btn.set_widget_name("restore-selected-btn");

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .build();
    actions.append(&restore_full_btn);
    actions.append(&restore_selected_btn);

    right.append(&right_stack);
    right.append(&target_group);
    right.append(&actions);

    paned.set_start_child(Some(&left_panel));
    paned.set_end_child(Some(&right));
    bind_restore_paned_split(&paned);
    outer.append(&paned);

    // --- Events ---
    profile_combo.connect_notify_local(Some("selected"), {
        let profile_combo = profile_combo.clone();
        let page = outer.clone();
        let toast = toast_overlay.clone();
        let state = state.clone();
        let suppress_profile_notify = suppress_profile_notify.clone();
        move |_, _| {
            if suppress_profile_notify.get() {
                return;
            }
            let idx = profile_combo.selected();
            let st = state.borrow();
            let Some(&id) = st.profile_ids.get(idx as usize) else {
                return;
            };
            drop(st);
            reset_browse_state(&state);
            state.borrow_mut().profile_id = Some(id);
            update_right_panel(page.upcast_ref(), &state);
            load_profile_paths(id, state.clone());
            load_snapshots(page.upcast_ref(), &toast, state.clone(), id);
        }
    });

    snapshots_list.connect_row_selected({
        let state = state.clone();
        let page = outer.clone();
        let path_label = path_label.clone();
        let back_btn = back_btn.clone();
        move |_, row| {
            let path = row.and_then(snapshot_path_from_row);
            let mut st = state.borrow_mut();
            st.snapshot = path.clone();
            st.archive = None;
            st.browse_mode = BrowseMode::Archives;
            st.browse_path.clear();
            st.catalog_entries.clear();
            st.expanded_archives.clear();
            st.archive_preview.clear();
            st.archive_preview_loading.clear();
            if let Some(ref snap_path) = path {
                if let Some(snap) = st.snapshots.iter().find(|s| &s.path == snap_path) {
                    st.archive_items = resolve_archive_items(snap, &st.profile_paths);
                } else {
                    st.archive_items.clear();
                }
            } else {
                st.archive_items.clear();
            }
            drop(st);
            update_path_ui(page.upcast_ref(), &path_label, &back_btn, &state);
            update_right_panel(page.upcast_ref(), &state);
            if path.is_some() {
                refresh_active_mounts(state.clone(), page.clone().upcast());
            } else {
                repopulate_file_list_from_state(page.upcast_ref(), &state, "");
            }
            update_action_buttons(page.upcast_ref(), &state);
        }
    });

    back_btn.connect_clicked({
        let state = state.clone();
        let page = outer.clone();
        let toast = toast_overlay.clone();
        let stack = stack.clone();
        let path_label = path_label.clone();
        let back_btn = back_btn.clone();
        move |_| {
            let st = state.borrow();
            if st.browse_mode == BrowseMode::Inside && st.browse_path.is_empty() {
                drop(st);
                enter_archive_list(page.upcast_ref(), &path_label, &back_btn, &state);
                return;
            }
            let parent = parent_path(&st.browse_path);
            drop(st);
            let Some(parent) = parent else {
                return;
            };
            state.borrow_mut().browse_path = parent;
            update_path_ui(page.upcast_ref(), &path_label, &back_btn, &state);
            load_catalog(page.upcast_ref(), &toast, &stack, state.clone(), false);
        }
    });

    search_entry.connect_search_changed({
        let state = state.clone();
        let page = outer.clone();
        let search_entry = search_entry.clone();
        move |_| {
            repopulate_file_list_from_state(page.upcast_ref(), &state, search_entry.text().as_str());
            update_action_buttons(page.upcast_ref(), &state);
        }
    });

    files_list.connect_row_activated({
        let state = state.clone();
        let page = outer.clone();
        let toast = toast_overlay.clone();
        let stack = stack.clone();
        let path_label = path_label.clone();
        let back_btn = back_btn.clone();
        move |_list, row| {
            let row_name = row.widget_name().to_string();
            if row_name == "catalog-parent" {
                let parent = {
                    let st = state.borrow();
                    parent_path(&st.browse_path)
                };
                let Some(parent) = parent else {
                    return;
                };
                state.borrow_mut().browse_path = parent;
                update_path_ui(page.upcast_ref(), &path_label, &back_btn, &state);
                load_catalog(page.upcast_ref(), &toast, &stack, state.clone(), false);
                return;
            }
            if let Some(archive_name) = archive_from_row_name(&row_name) {
                enter_archive(
                    page.upcast_ref(),
                    &toast,
                    &stack,
                    &path_label,
                    &back_btn,
                    &state,
                    &archive_name,
                );
                return;
            }
            let name = row
                .child()
                .and_then(|c| c.downcast::<libadwaita::ActionRow>().ok())
                .map(|r| r.title().to_string());
            let Some(name) = name else {
                return;
            };
            let (is_dir, new_path, open_archive) = {
                let st = state.borrow();
                if st.browse_mode == BrowseMode::Archives {
                    let mut hit: Option<(bool, String, String)> = None;
                    for (archive_name, entries) in &st.archive_preview {
                        if let Some(entry) = entries.iter().find(|e| e.name == name) {
                            hit = Some((entry.is_dir, entry.path.clone(), archive_name.clone()));
                            break;
                        }
                    }
                    let Some((is_dir, path, archive_name)) = hit else {
                        return;
                    };
                    if !is_dir {
                        return;
                    }
                    (true, path, Some(archive_name))
                } else {
                    let is_dir = st
                        .catalog_entries
                        .iter()
                        .find(|e| e.name == name)
                        .map(|e| e.is_dir)
                        .unwrap_or(false);
                    if !is_dir {
                        return;
                    }
                    let new_path = if st.browse_path.is_empty() {
                        name
                    } else {
                        format!("{}/{}", st.browse_path, name)
                    };
                    (is_dir, new_path, None)
                }
            };
            if let Some(archive_name) = open_archive {
                enter_archive(
                    page.upcast_ref(),
                    &toast,
                    &stack,
                    &path_label,
                    &back_btn,
                    &state,
                    &archive_name,
                );
                state.borrow_mut().browse_path = new_path.clone();
                update_path_ui(page.upcast_ref(), &path_label, &back_btn, &state);
                if !new_path.is_empty() {
                    load_catalog(page.upcast_ref(), &toast, &stack, state.clone(), false);
                }
                return;
            }
            if !is_dir {
                return;
            }
            state.borrow_mut().browse_path = new_path;
            update_path_ui(page.upcast_ref(), &path_label, &back_btn, &state);
            load_catalog(page.upcast_ref(), &toast, &stack, state.clone(), false);
        }
    });

    select_all_btn.connect_clicked({
        let page = outer.clone();
        let state = state.clone();
        let search_entry = search_entry.clone();
        let suppress = suppress_check_toggles.clone();
        move |_| {
            let query = search_entry.text().to_string();
            let st = state.borrow();
            let mut visible = filter_entries(&st.catalog_entries, &query);
            if st.browse_mode == BrowseMode::Archives {
                for item in filter_archive_items(&st.archive_items, &query) {
                    if st.expanded_archives.contains(&item.archive_name) {
                        if let Some(children) = st.archive_preview.get(&item.archive_name) {
                            visible.extend(filter_entries(children, &query));
                        }
                    }
                }
            }
            drop(st);
            select_entries_in_view(&mut state.borrow_mut().selected_entries, &visible);
            if let Some(list) = file_list(page.upcast_ref()) {
                let selected = state.borrow().selected_entries.clone();
                sync_visible_checks(&list, &selected, &suppress);
            }
            update_action_buttons(page.upcast_ref(), &state);
        }
    });

    clear_sel_btn.connect_clicked({
        let page = outer.clone();
        let state = state.clone();
        let suppress = suppress_check_toggles.clone();
        move |_| {
            state.borrow_mut().selected_entries.clear();
            if let Some(list) = file_list(page.upcast_ref()) {
                sync_visible_checks(&list, &state.borrow().selected_entries, &suppress);
            }
            update_action_buttons(page.upcast_ref(), &state);
        }
    });

    target_row.connect_changed({
        let page = outer.clone();
        let state = state.clone();
        move |_| {
            update_action_buttons(page.upcast_ref(), &state);
        }
    });

    original_path_row.connect_active_notify({
        let original_path_row = original_path_row.clone();
        let target_row = target_row.clone();
        let state = state.clone();
        let page = outer.clone();
        move |_| {
            if original_path_row.is_active() {
                apply_original_restore_target(&target_row, &state);
            }
            update_action_buttons(page.upcast_ref(), &state);
        }
    });

    browse_btn.connect_clicked({
        let parent = parent.clone();
        let target_row = target_row.clone();
        let page = outer.clone();
        let state = state.clone();
        move |_| {
            choose_target_folder(&parent, &target_row, page.upcast_ref(), &state);
        }
    });

    let original_path_for_restore = original_path_row.clone();

    restore_selected_btn.connect_clicked({
        let toast = toast_overlay.clone();
        let state = state.clone();
        let target_row = target_row.clone();
        let overwrite_row = overwrite_row.clone();
        let original_path_row = original_path_for_restore.clone();
        let page = outer.clone();
        move |_| {
            let entries = selected_catalog_entries(&state.borrow().selected_entries);
            if entries.is_empty() {
                return;
            }
            let patterns = dedupe_patterns(patterns_for_entries(&entries));
            start_restore(
                page.upcast_ref(),
                &toast,
                &state,
                target_row.text().to_string(),
                overwrite_row.is_active(),
                original_path_row.is_active(),
                patterns,
            );
        }
    });

    restore_full_btn.connect_clicked({
        let page = outer.clone();
        let toast = toast_overlay.clone();
        let state = state.clone();
        let target_row = target_row.clone();
        let overwrite_row = overwrite_row.clone();
        let original_path_row = original_path_for_restore;
        move |_| {
            start_restore(
                page.upcast_ref(),
                &toast,
                &state,
                target_row.text().to_string(),
                overwrite_row.is_active(),
                original_path_row.is_active(),
                Vec::new(),
            );
        }
    });

    let page_widget = outer.clone().upcast::<gtk::Widget>();
    RESTORE_CTX.with(|slot| {
        *slot.borrow_mut() = Some(RestoreCtx {
            page: page_widget,
            toast: toast_overlay.clone(),
            profile_combo: profile_combo.clone(),
            state: state.clone(),
            suppress_check_toggles: suppress_check_toggles.clone(),
            suppress_profile_notify: suppress_profile_notify.clone(),
        });
    });

    outer.upcast()
}

pub fn refresh() {
    let ctx = RESTORE_CTX.with(|slot| slot.borrow().clone());
    let Some(ctx) = ctx else {
        return;
    };
    load_profile_list(
        &ctx.profile_combo,
        &ctx.page,
        &ctx.toast,
        ctx.state,
        ctx.suppress_profile_notify,
    );
}

const RESTORE_PANE_START_MIN: i32 = 320;
const RESTORE_PANE_END_MIN: i32 = 460;

/// Set a comfortable 38/62 split once the paned has a real width (avoids squashed columns).
fn bind_restore_paned_split(paned: &gtk::Paned) {
    let paned_weak = paned.downgrade();
    let initial_split_applied = Rc::new(Cell::new(false));
    paned.connect_notify_local(Some("width"), move |p, _| {
        if initial_split_applied.get() {
            return;
        }
        let w = p.width();
        if w < RESTORE_PANE_START_MIN + RESTORE_PANE_END_MIN + 40 {
            return;
        }
        let Some(paned) = paned_weak.upgrade() else {
            return;
        };
        let max_pos = w - RESTORE_PANE_END_MIN;
        let pos = ((w as f64) * 0.38) as i32;
        paned.set_position(pos.clamp(RESTORE_PANE_START_MIN, max_pos));
        initial_split_applied.set(true);
    });
}

fn reset_browse_state(state: &Rc<RefCell<PageState>>) {
    let mut st = state.borrow_mut();
    st.snapshot = None;
    st.browse_mode = BrowseMode::Archives;
    st.archive_items.clear();
    st.expanded_archives.clear();
    st.archive_preview.clear();
    st.archive_preview_loading.clear();
    st.archive = None;
    st.browse_path.clear();
    st.catalog_entries.clear();
    st.selected_entries.clear();
}

fn update_path_ui(page: &gtk::Widget, path_label: &gtk::Label, back_btn: &gtk::Button, state: &Rc<RefCell<PageState>>) {
    let st = state.borrow();
    path_label.set_label(&format_browse_path(
        st.browse_mode,
        st.archive.as_deref(),
        &st.browse_path,
    ));
    let back_sensitive = match st.browse_mode {
        BrowseMode::Archives => false,
        BrowseMode::Inside => true,
    };
    back_btn.set_sensitive(back_sensitive);
    if let Some(search) = find_child_by_name(page, "restore-search")
        .and_then(|w| w.downcast::<gtk::SearchEntry>().ok())
    {
        let placeholder = match st.browse_mode {
            BrowseMode::Archives => tr("Search backup paths"),
            BrowseMode::Inside => tr("Search in this folder"),
        };
        search.set_placeholder_text(Some(&placeholder));
    }
}

fn enter_archive_list(
    page: &gtk::Widget,
    path_label: &gtk::Label,
    back_btn: &gtk::Button,
    state: &Rc<RefCell<PageState>>,
) {
    {
        let mut st = state.borrow_mut();
        st.browse_mode = BrowseMode::Archives;
        st.archive = None;
        st.browse_path.clear();
        st.catalog_entries.clear();
    }
    update_path_ui(page, path_label, back_btn, state);
    repopulate_file_list_from_state(page, state, "");
    update_action_buttons(page, state);
}

fn enter_archive(
    page: &gtk::Widget,
    toast: &ToastOverlay,
    stack: &gtk::Stack,
    path_label: &gtk::Label,
    back_btn: &gtk::Button,
    state: &Rc<RefCell<PageState>>,
    archive_name: &str,
) {
    {
        let mut st = state.borrow_mut();
        st.browse_mode = BrowseMode::Inside;
        st.archive = Some(archive_name.to_string());
        st.browse_path.clear();
        st.catalog_entries.clear();
    }
    update_path_ui(page, path_label, back_btn, state);
    load_catalog(page, toast, stack, state.clone(), false);
}

fn toggle_archive_expanded(
    page: &gtk::Widget,
    toast: &ToastOverlay,
    state: &Rc<RefCell<PageState>>,
    archive_name: &str,
    expanded: bool,
    search_query: &str,
) {
    {
        let mut st = state.borrow_mut();
        if expanded {
            st.expanded_archives.insert(archive_name.to_string());
        } else {
            st.expanded_archives.remove(archive_name);
        }
    }
    if expanded && !state.borrow().archive_preview.contains_key(archive_name) {
        load_archive_preview(page, toast, state.clone(), archive_name.to_string());
    } else {
        repopulate_file_list_from_state(page, state, search_query);
    }
}

fn load_archive_preview(
    page: &gtk::Widget,
    toast: &ToastOverlay,
    state: Rc<RefCell<PageState>>,
    archive_name: String,
) {
    let st = state.borrow();
    let Some(profile_id) = st.profile_id else {
        return;
    };
    let Some(snapshot) = st.snapshot.clone() else {
        return;
    };
    drop(st);

    state
        .borrow_mut()
        .archive_preview_loading
        .insert(archive_name.clone());
    repopulate_file_list_from_state(page, &state, "");

    let encryption_key_id = encryption_key_id_for_snapshot(&state.borrow());
    let request = ListCatalogRequest {
        profile_id,
        snapshot,
        archive_name: archive_name.clone(),
        parent_path: String::new(),
        force_refresh: false,
        encryption_key_id,
    };

    let page = page.clone();
    let toast = toast.clone();
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::list_catalog(&proxy, &request).await
        },
        move |result| {
            state
                .borrow_mut()
                .archive_preview_loading
                .remove(&archive_name);
            match result {
                Ok(response) => {
                    state
                        .borrow_mut()
                        .archive_preview
                        .insert(archive_name.clone(), response.entries);
                }
                Err(err) => {
                    show_toast(
                        &toast,
                        &tr_fmt(
                            "Could not load preview for {archive}: {err}",
                            &[
                                ("archive", &archive_name),
                                ("err", &err.to_string()),
                            ],
                        ),
                    );
                    state.borrow_mut().expanded_archives.remove(&archive_name);
                }
            }
            repopulate_file_list_from_state(page.upcast_ref(), &state, "");
        },
    );
}

fn file_list(page: &gtk::Widget) -> Option<gtk::ListBox> {
    find_child_by_name(page, "restore-file-list").and_then(|w| w.downcast().ok())
}

fn right_stack(page: &gtk::Widget) -> Option<gtk::Stack> {
    find_child_by_name(page, "restore-right-stack").and_then(|w| w.downcast().ok())
}

fn update_right_panel(page: &gtk::Widget, state: &Rc<RefCell<PageState>>) {
    let st = state.borrow();
    let Some(stack) = right_stack(page) else {
        return;
    };
    if st.snapshot.is_some() {
        stack.set_visible_child_name("browse");
    } else {
        stack.set_visible_child_name("idle");
    }
}

fn update_action_buttons(page: &gtk::Widget, state: &Rc<RefCell<PageState>>) {
    let st = state.borrow();
    let target_ok = find_child_by_name(page, "restore-target-entry")
        .and_then(|w| w.downcast::<libadwaita::EntryRow>().ok())
        .map(|e| !e.text().trim().is_empty())
        .unwrap_or(false);
    let snap_ok = st.snapshot.is_some() && st.profile_id.is_some();
    let selected_count = st.selected_entries.len();

    if let Some(btn) = find_child_by_name(page, "restore-selected-btn")
        .and_then(|w| w.downcast::<gtk::Button>().ok())
    {
        btn.set_sensitive(
            snap_ok
                && st.can_restore
                && target_ok
                && selected_count > 0
                && !st.catalog_loading
                && !st.restore_in_progress,
        );
        if !st.can_restore {
            btn.set_tooltip_text(Some(&tr("No Datastore.Read permission on this profile")));
        } else {
            btn.set_tooltip_text(None);
        }
        btn.set_label(&tr_fmt(
            "Restore selected ({count})",
            &[("count", &selected_count.to_string())],
        ));
    }
    if let Some(btn) = find_child_by_name(page, "restore-full-btn")
        .and_then(|w| w.downcast::<gtk::Button>().ok())
    {
        let inside = st.browse_mode == BrowseMode::Inside && st.archive.is_some();
        btn.set_sensitive(
            snap_ok && st.can_restore && inside && target_ok && !st.catalog_loading && !st.restore_in_progress,
        );
        if !st.can_restore {
            btn.set_tooltip_text(Some(&tr("No Datastore.Read permission on this profile")));
        } else {
            btn.set_tooltip_text(None);
        }
    }
}

fn filter_archive_items(items: &[ArchiveBrowseItem], query: &str) -> Vec<ArchiveBrowseItem> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|item| {
            item.source_path.to_lowercase().contains(&q)
                || item.archive_name.to_lowercase().contains(&q)
        })
        .cloned()
        .collect()
}

fn wire_archive_row(
    row: &gtk::ListBoxRow,
    item: ArchiveBrowseItem,
    state: Rc<RefCell<PageState>>,
    page: gtk::Widget,
    toast: ToastOverlay,
) {
    let archive_name = item.archive_name.clone();
    if let Some(expand) = find_child_by_name(row.upcast_ref(), ARCHIVE_EXPAND_NAME)
        .and_then(|w| w.downcast::<gtk::ToggleButton>().ok())
    {
        let page = page.clone();
        let toast = toast.clone();
        let state = state.clone();
        expand.connect_toggled(move |btn| {
            let query = find_child_by_name(page.upcast_ref(), "restore-search")
                .and_then(|w| w.downcast::<gtk::SearchEntry>().ok())
                .map(|e| e.text().to_string())
                .unwrap_or_default();
            toggle_archive_expanded(
                page.upcast_ref(),
                &toast,
                &state,
                &archive_name,
                btn.is_active(),
                &query,
            );
        });
    }

    if let Some(mount_btn) = find_child_by_name(row.upcast_ref(), ARCHIVE_MOUNT_BTN)
        .and_then(|w| w.downcast::<gtk::Button>().ok())
    {
        let item = item.clone();
        let state = state.clone();
        let page = page.clone();
        let toast = toast.clone();
        mount_btn.connect_clicked(move |_| {
            let st = state.borrow();
            let Some(profile_id) = st.profile_id else {
                return;
            };
            let Some(snapshot) = st.snapshot.clone() else {
                return;
            };
            let mount_id = mount_session_id(profile_id, &snapshot, &item.archive_name);
            if let Some(mount) = active_mount_for_archive(
                &st.active_mounts,
                profile_id,
                &snapshot,
                &item.archive_name,
            ) {
                open_path_in_file_manager(&mount.mount_point);
                return;
            }
            let encryption_key_id = encryption_key_id_for_snapshot(&st);
            drop(st);
            let Some(parent) = window::main_window() else {
                return;
            };
            state.borrow_mut().mounting_ids.insert(mount_id);
            repopulate_file_list_from_state(&page, &state, "");
            let on_done = Rc::new({
                let state = state.clone();
                let page = page.clone();
                move || {
                    state.borrow_mut().mounting_ids.clear();
                    refresh_active_mounts(state.clone(), page.clone());
                }
            });
            confirm_and_mount_archive(
                &parent,
                &toast,
                profile_id,
                snapshot,
                item.clone(),
                encryption_key_id,
                on_done,
            );
        });
    }
}

fn refresh_active_mounts(state: Rc<RefCell<PageState>>, page: gtk::Widget) {
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::list_active_mounts(&proxy).await
        },
        move |result| {
            if let Ok(mounts) = result {
                state.borrow_mut().active_mounts = mounts;
            }
            repopulate_file_list_from_state(&page, &state, "");
        },
    );
}

fn load_profile_paths(profile_id: i64, state: Rc<RefCell<PageState>>) {
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::get_profile(&proxy, profile_id).await
        },
        move |result| {
            if let Ok(profile) = result {
                let mut st = state.borrow_mut();
                st.profile_paths = profile.paths;
                if let Some(ref snap_path) = st.snapshot {
                    if let Some(snap) = st.snapshots.iter().find(|s| &s.path == snap_path) {
                        let snap = snap.clone();
                        let paths = st.profile_paths.clone();
                        st.archive_items = resolve_archive_items(&snap, &paths);
                    }
                }
                drop(st);
                if let Some(ctx) = RESTORE_CTX.with(|slot| slot.borrow().clone()) {
                    repopulate_file_list_from_state(&ctx.page, &state, "");
                }
            }
        },
    );
}

fn load_profile_list(
    profile_combo: &libadwaita::ComboRow,
    page: &gtk::Widget,
    toast: &ToastOverlay,
    state: Rc<RefCell<PageState>>,
    suppress_profile_notify: Rc<Cell<bool>>,
) {
    let profile_combo = profile_combo.clone();
    let page = page.clone();
    let toast = toast.clone();
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            let profiles = dbus_client::list_profiles(&proxy).await?;
            let keys = dbus_client::list_encryption_keys(&proxy).await.unwrap_or_default();
            Ok((profiles, keys))
        },
        move |result| match result {
            Ok((profiles, keys)) => {
                if profiles.is_empty() {
                    show_snapshots_hint(&page, &tr("No backup profiles configured yet."));
                    return;
                }
                let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
                let id = profiles[0].id;
                suppress_profile_notify.set(true);
                profile_combo.set_model(Some(&gtk::StringList::new(&names)));
                profile_combo.set_selected(0);
                suppress_profile_notify.set(false);
                let mut st = state.borrow_mut();
                st.profile_ids = profiles.iter().map(|p| p.id).collect();
                st.profile_names = profiles.iter().map(|p| p.name.clone()).collect();
                st.encryption_key_by_fingerprint = encryption_fingerprint_name_map(&keys);
                st.encryption_keys = keys;
                st.profile_id = Some(id);
                drop(st);
                load_profile_paths(id, state.clone());
                load_snapshots(&page, &toast, state, id);
            }
            Err(err) => show_toast(&toast, &tr_fmt("Daemon error: {err}", &[("err", &err.to_string())])),
        },
    );
}

fn load_snapshots(
    page: &gtk::Widget,
    toast: &ToastOverlay,
    state: Rc<RefCell<PageState>>,
    profile_id: i64,
) {
    let list = find_child_by_name(page, "restore-snapshots-list");
    let Some(list) = list.and_then(|w| w.downcast::<gtk::ListBox>().ok()) else {
        return;
    };
    clear_list_box(&list);
    update_right_panel(page, &state);

    let load_seq = {
        let mut st = state.borrow_mut();
        st.snapshot_load_seq = st.snapshot_load_seq.wrapping_add(1);
        st.snapshot_load_seq
    };

    let toast = toast.clone();
    let page = page.clone();
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            let snapshots = dbus_client::list_snapshots(&proxy, profile_id).await?;
            let keys = dbus_client::list_encryption_keys(&proxy).await.unwrap_or_default();
            let perms = dbus_client::check_snapshot_permissions(&proxy, profile_id)
                .await
                .unwrap_or_else(|_| backuppilot_core::DatastorePermissions::none());
            Ok((snapshots, keys, perms))
        },
        move |result| match result {
            Ok((snapshots, keys, perms)) => {
                if state.borrow().snapshot_load_seq != load_seq {
                    return;
                }
                let mut st = state.borrow_mut();
                st.snapshots = snapshots.clone();
                st.encryption_key_by_fingerprint = encryption_fingerprint_name_map(&keys);
                st.encryption_keys = keys;
                st.can_delete = perms.can_delete;
                st.can_restore = perms.can_restore;
                drop(st);
                reset_browse_state(&state);
                clear_list_box(&list);
                if snapshots.is_empty() {
                    show_snapshots_hint(list.upcast_ref(), &tr("No snapshots found for this profile."));
                    return;
                }
                let st = state.borrow();
                let cd = st.can_delete;
                for snap in snapshots {
                    list.append(&snapshot_row(
                        &snap,
                        &snapshot_encryption_for_snap(&snap, &st),
                        cd,
                        profile_id,
                        page.clone(),
                        toast.clone(),
                        state.clone(),
                    ));
                }
                drop(st);
                update_action_buttons(page.upcast_ref(), &state);
            }
            Err(err) => {
                if state.borrow().snapshot_load_seq != load_seq {
                    return;
                }
                let summary = summarize_pbs_error(&err.to_string());
                show_snapshots_hint(
                    list.upcast_ref(),
                    &tr_fmt(
                        "Failed to load snapshots: {err}",
                        &[("err", &summary)],
                    ),
                );
                show_toast(
                    &toast,
                    &tr_fmt("Failed to load snapshots: {err}", &[("err", &summary)]),
                );
            }
        },
    );
}

fn encryption_key_id_for_snapshot(state: &PageState) -> Option<i64> {
    let snap_path = state.snapshot.as_ref()?;
    let snap = state.snapshots.iter().find(|s| &s.path == snap_path)?;
    if !snap.encrypted {
        return None;
    }
    let fp = snap.fingerprint.as_deref()?;
    state.encryption_keys.iter().find_map(|k| {
        k.fingerprint
            .as_ref()
            .filter(|kfp| fingerprints_match(fp, kfp))
            .map(|_| k.id)
    })
}

fn encryption_fingerprint_name_map(keys: &[EncryptionKey]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for key in keys {
        if let Some(fp) = key.fingerprint.as_deref() {
            map.insert(normalize_fingerprint(fp), key.name.clone());
        }
    }
    map
}

fn snapshot_encryption_for_snap(snap: &PbsSnapshotInfo, state: &PageState) -> SnapshotEncryptionInfo {
    if !snap.encrypted {
        return SnapshotEncryptionInfo {
            encrypted: false,
            key_name: None,
        };
    }
    let key_name = snap.fingerprint.as_ref().and_then(|fp| {
        state
            .encryption_key_by_fingerprint
            .get(&normalize_fingerprint(fp))
            .cloned()
    });
    SnapshotEncryptionInfo {
        encrypted: true,
        key_name,
    }
}

fn snapshot_disk_tooltip(snap: &PbsSnapshotInfo, enc: &SnapshotEncryptionInfo) -> Option<String> {
    if !enc.encrypted {
        return Some(tr("Not encrypted"));
    }
    Some(match enc.key_name.as_deref() {
        Some(name) => tr_fmt("Encrypted with key «{name}»", &[("name", name)]),
        None if snap.fingerprint.is_some() => tr("Encrypted , key not stored in BackupPilot"),
        None => tr("Encrypted backup"),
    })
}

fn snapshot_row(
    snap: &PbsSnapshotInfo,
    enc: &SnapshotEncryptionInfo,
    can_delete: bool,
    profile_id: i64,
    page: gtk::Widget,
    toast: ToastOverlay,
    state: Rc<RefCell<PageState>>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&snapshot_row_key(&snap.path));
    let display = snapshot_display_name(&snap.path);
    let action = libadwaita::ActionRow::builder()
        .title(&display)
        .subtitle(&format_size(snap.size_bytes))
        .activatable(true)
        .build();
    action.set_tooltip_text(Some(&snap.path));

    let disk_icon = gtk::Image::builder()
        .icon_name("drive-harddisk-symbolic")
        .pixel_size(16)
        .valign(gtk::Align::Center)
        .build();
    if enc.encrypted {
        disk_icon.add_css_class("success");
    }
    if let Some(tip) = snapshot_disk_tooltip(snap, enc) {
        disk_icon.set_tooltip_text(Some(&tip));
    }
    action.add_prefix(&disk_icon);

    // Delete button
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(vec!["flat".to_string()])
        .build();

    let snap_path = snap.path.clone();
    let snap_protected = snap.protected;

    if snap_protected {
        delete_btn.set_sensitive(false);
        delete_btn.set_tooltip_text(Some(&tr("Protected -- cannot delete")));
    } else if !can_delete {
        delete_btn.set_sensitive(false);
        delete_btn.set_tooltip_text(Some(&tr("No Datastore.Prune permission on this profile")));
    } else {
        delete_btn.set_tooltip_text(Some(&tr("Delete snapshot")));
        let snap_path_clone = snap_path.clone();
        let display_clone = display.clone();
        let page_clone = page.clone();
        let toast_clone = toast.clone();
        let state_clone = state.clone();
        delete_btn.connect_clicked(move |_| {
            confirm_and_delete_snapshot(
                profile_id,
                snap_path_clone.clone(),
                display_clone.clone(),
                page_clone.clone(),
                toast_clone.clone(),
                state_clone.clone(),
            );
        });
    }
    action.add_suffix(&delete_btn);

    row.set_child(Some(&action));
    row
}

fn confirm_and_delete_snapshot(
    profile_id: i64,
    snapshot_path: String,
    display: String,
    page: gtk::Widget,
    toast: ToastOverlay,
    state: Rc<RefCell<PageState>>,
) {
    let heading = tr("Delete snapshot?");
    let body = tr_fmt(
        "Delete snapshot \"{name}\"? This cannot be undone.",
        &[("name", &display)],
    );
    let alert = libadwaita::AlertDialog::builder()
        .heading(&heading)
        .body(&body)
        .build();
    alert.add_response("cancel", &tr("Cancel"));
    alert.add_response("delete", &tr("Delete"));
    alert.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");

    let page_for_present = page.clone();
    alert.connect_response(None::<&str>, move |_, response| {
        if response != "delete" {
            return;
        }
        let snap_path = snapshot_path.clone();
        let page_inner = page.clone();
        let toast_inner = toast.clone();
        let state_inner = state.clone();
        dbus_runtime::spawn(
            async move {
                let proxy = connect().await?;
                dbus_client::delete_snapshot(&proxy, profile_id, &snap_path).await
            },
            move |result| {
                match result {
                    Ok(()) => {
                        show_toast(&toast_inner, &tr("Snapshot deleted."));
                        // Reload snapshot list
                        if let Some(pid) = state_inner.borrow().profile_id {
                            load_snapshots(&page_inner, &toast_inner, state_inner.clone(), pid);
                        }
                    }
                    Err(err) => {
                        show_toast(
                            &toast_inner,
                            &tr_fmt("Failed to delete snapshot: {err}", &[("err", &err.to_string())]),
                        );
                    }
                }
            },
        );
    });

    if let Some(window) = page_for_present
        .root()
        .and_then(|r| r.downcast::<ApplicationWindow>().ok())
    {
        alert.present(Some(&window));
    }
}

fn snapshot_row_key(path: &str) -> String {
    format!("snap|{path}")
}

fn snapshot_path_from_row(row: &gtk::ListBoxRow) -> Option<String> {
    let name = row.widget_name();
    name.strip_prefix("snap|").map(str::to_string)
}

fn load_catalog(
    page: &gtk::Widget,
    toast: &ToastOverlay,
    stack: &gtk::Stack,
    state: Rc<RefCell<PageState>>,
    force_refresh: bool,
) {
    let st = state.borrow();
    let Some(profile_id) = st.profile_id else {
        return;
    };
    let Some(snapshot) = st.snapshot.clone() else {
        return;
    };
    let Some(archive) = st.archive.clone() else {
        return;
    };
    let parent_path = st.browse_path.clone();
    let encryption_key_id = encryption_key_id_for_snapshot(&st);
    drop(st);

    stack.set_visible_child_name("browser");
    state.borrow_mut().catalog_loading = true;
    show_catalog_loading(page);
    set_browse_controls_sensitive(page, false);
    update_action_buttons(page, &state);
    let request = ListCatalogRequest {
        profile_id,
        snapshot,
        archive_name: archive,
        parent_path,
        force_refresh,
        encryption_key_id,
    };

    let toast = toast.clone();
    let page = page.clone();
    let stack = stack.clone();
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::list_catalog(&proxy, &request).await
        },
        move |result| {
            state.borrow_mut().catalog_loading = false;
            set_browse_controls_sensitive(page.upcast_ref(), true);
            match result {
                Ok(response) => {
                    if state.borrow().archive.is_none()
                        && !response.suggested_archive.is_empty()
                    {
                        state.borrow_mut().archive = Some(response.suggested_archive.clone());
                    }
                    state.borrow_mut().catalog_entries = response.entries.clone();
                    let parent_path = response.parent_path.clone();
                    state.borrow_mut().browse_path = parent_path.clone();
                    if let Some(path_label) = find_child_by_name(page.upcast_ref(), "restore-path-label")
                        .and_then(|w| w.downcast::<gtk::Label>().ok())
                    {
                        if let Some(back_btn) = find_child_by_name(page.upcast_ref(), "restore-back-btn")
                            .and_then(|w| w.downcast::<gtk::Button>().ok())
                        {
                            update_path_ui(page.upcast_ref(), &path_label, &back_btn, &state);
                        }
                    }
                    let query = find_child_by_name(page.upcast_ref(), "restore-search")
                        .and_then(|w| w.downcast::<gtk::SearchEntry>().ok())
                        .map(|e| e.text().to_string())
                        .unwrap_or_default();
                    repopulate_file_list_from_state(page.upcast_ref(), &state, &query);
                    stack.set_visible_child_name("browser");
                }
                Err(err) => {
                    stack.set_visible_child_name("browser");
                    if let Some(list) = file_list(page.upcast_ref()) {
                        clear_list_box(&list);
                        append_info_row(
                            &list,
                            &tr_fmt("Failed to load files: {err}", &[("err", &err.to_string())]),
                        );
                    }
                    show_toast(
                        &toast,
                        &tr_fmt("Failed to load files: {err}", &[("err", &err.to_string())]),
                    );
                }
            }
            update_action_buttons(page.upcast_ref(), &state);
        },
    );
}

fn show_catalog_loading(page: &gtk::Widget) {
    let Some(list) = file_list(page) else {
        return;
    };
    clear_list_box(&list);

    let row = gtk::ListBoxRow::builder()
        .selectable(false)
        .activatable(false)
        .build();
    row.set_widget_name("restore-catalog-loading-row");

    let center = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(56)
        .margin_bottom(56)
        .margin_start(32)
        .margin_end(32)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();

    let spinner = gtk::Spinner::builder()
        .spinning(true)
        .width_request(32)
        .height_request(32)
        .build();

    let title = gtk::Label::builder()
        .label(&tr("Reading backup catalog…"))
        .css_classes(["title-4"])
        .justify(gtk::Justification::Center)
        .wrap(true)
        .max_width_chars(36)
        .build();

    let detail = gtk::Label::builder()
        .label(&tr("For large backups this can take a few minutes."))
        .css_classes(["dim-label"])
        .justify(gtk::Justification::Center)
        .wrap(true)
        .max_width_chars(42)
        .build();

    center.append(&spinner);
    center.append(&title);
    center.append(&detail);
    row.set_child(Some(&center));
    list.append(&row);

    if let Some(path_label) = find_child_by_name(page, "restore-path-label")
        .and_then(|w| w.downcast::<gtk::Label>().ok())
    {
        path_label.set_label(&tr("Loading file list…"));
    }
}

fn set_browse_controls_sensitive(page: &gtk::Widget, sensitive: bool) {
    for name in [
        "restore-back-btn",
        "restore-search",
        "restore-select-all-btn",
        "restore-clear-sel-btn",
    ] {
        if let Some(widget) = find_child_by_name(page, name) {
            widget.set_sensitive(sensitive);
        }
    }
}

fn wire_row_checkbox(
    row: &gtk::ListBoxRow,
    entry: CatalogEntry,
    state: Rc<RefCell<PageState>>,
    page: gtk::Widget,
    suppress_toggled: Rc<Cell<bool>>,
) {
    let Some(check) = row_check_button(row) else {
        return;
    };
    let key = catalog_row_key(&entry.archive, &entry.path);
    check.connect_toggled(move |cb| {
        if suppress_toggled.get() {
            return;
        }
        let active = cb.is_active();
        let mut st = state.borrow_mut();
        if active {
            st.selected_entries.insert(key.clone(), entry.clone());
        } else {
            st.selected_entries.remove(&key);
        }
        drop(st);
        update_action_buttons(page.upcast_ref(), &state);
    });
}

fn repopulate_file_list_from_state(page: &gtk::Widget, state: &Rc<RefCell<PageState>>, search_query: &str) {
    let Some(list) = file_list(page) else {
        return;
    };
    repopulate_file_list(page, &list, state, search_query);
}

fn repopulate_file_list(
    page: &gtk::Widget,
    list: &gtk::ListBox,
    state: &Rc<RefCell<PageState>>,
    search_query: &str,
) {
    clear_list_box(list);
    let st = state.borrow();
    let toast = RESTORE_CTX.with(|slot| slot.borrow().as_ref().map(|c| c.toast.clone()));

    if st.browse_mode == BrowseMode::Archives {
        let items = filter_archive_items(&st.archive_items, search_query);
        if items.is_empty() && search_query.is_empty() {
            append_info_row(
                list,
                &tr("No backup archives in this snapshot. Check profile backup paths."),
            );
            return;
        }
        if items.is_empty() {
            append_info_row(list, &tr("No backup paths match your search."));
            return;
        }
        for item in &items {
            let expanded = st.expanded_archives.contains(&item.archive_name);
            let loading = st.archive_preview_loading.contains(&item.archive_name);
            let mount_state = st
                .profile_id
                .and_then(|pid| {
                    st.snapshot.as_ref().map(|snap| {
                        mount_state_for_archive(
                            &st.active_mounts,
                            pid,
                            snap,
                            &item.archive_name,
                            &st.mounting_ids,
                        )
                    })
                })
                .unwrap_or(ArchiveMountBtnState::Mount);
            let row = build_archive_row(item, expanded, loading, mount_state);
            if let Some(ref toast) = toast {
                wire_archive_row(
                    &row,
                    item.clone(),
                    state.clone(),
                    page.clone(),
                    toast.clone(),
                );
            }
            list.append(&row);
            if expanded {
                if let Some(children) = st.archive_preview.get(&item.archive_name) {
                    let children = filter_entries(children, search_query);
                    if children.is_empty() && !loading {
                        append_info_row(
                            list,
                            &tr("No files in this archive root (catalog empty or still loading)."),
                        );
                    }
                    for entry in &children {
                        let key = catalog_row_key(&entry.archive, &entry.path);
                        let selected = st.selected_entries.contains_key(&key);
                        let child_row = build_archive_preview_row(entry, selected);
                        wire_row_checkbox(
                            &child_row,
                            entry.clone(),
                            state.clone(),
                            page.clone(),
                            suppress_check_toggles(),
                        );
                        list.append(&child_row);
                    }
                }
            }
        }
        return;
    }

    if !st.browse_path.is_empty() {
        list.append(&build_parent_row());
    }
    let entries = filter_entries(&st.catalog_entries, search_query);
    let show_empty = entries.is_empty();
    for entry in &entries {
        let key = catalog_row_key(&entry.archive, &entry.path);
        let selected = st.selected_entries.contains_key(&key);
        let row = build_file_row(entry, selected);
        wire_row_checkbox(
            &row,
            entry.clone(),
            state.clone(),
            page.clone(),
            suppress_check_toggles(),
        );
        list.append(&row);
    }
    if show_empty && search_query.is_empty() {
        append_info_row(
            list,
            &tr("No files in this folder. Go up or pick another snapshot."),
        );
    } else if show_empty {
        append_info_row(list, &tr("No files match your search."));
    }
}

fn profile_display_name(state: &PageState) -> String {
    let st = state;
    let Some(id) = st.profile_id else {
        return String::new();
    };
    st.profile_ids
        .iter()
        .position(|&pid| pid == id)
        .and_then(|i| st.profile_names.get(i))
        .cloned()
        .unwrap_or_default()
}

fn stop_restore_pulse(state: &Rc<RefCell<PageState>>) {
    if let Some(source) = state.borrow_mut().restore_pulse_source.take() {
        source.remove();
    }
}

fn start_restore_pulse(page: &gtk::Widget, state: &Rc<RefCell<PageState>>) {
    stop_restore_pulse(state);
    let page = page.clone();
    let state_for_timer = state.clone();
    let source = glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
        if !state_for_timer.borrow().restore_in_progress {
            return glib::ControlFlow::Break;
        }
        if let Some(bar) = find_child_by_name(page.upcast_ref(), "restore-status-progress")
            .and_then(|w| w.downcast::<gtk::ProgressBar>().ok())
        {
            bar.pulse();
        }
        glib::ControlFlow::Continue
    });
    state.borrow_mut().restore_pulse_source = Some(source);
}

fn set_restore_in_progress(
    page: &gtk::Widget,
    state: &Rc<RefCell<PageState>>,
    active: bool,
    target_dir: &str,
) {
    {
        let mut st = state.borrow_mut();
        st.restore_in_progress = active;
        if let Some(box_) = find_child_by_name(page, "restore-status-box") {
            box_.set_visible(active);
        }
        if let Some(spinner) = find_child_by_name(page, "restore-status-spinner")
            .and_then(|w| w.downcast::<gtk::Spinner>().ok())
        {
            spinner.set_spinning(active);
        }
        if active {
            if let Some(detail) = find_child_by_name(page, "restore-status-detail")
                .and_then(|w| w.downcast::<gtk::Label>().ok())
            {
                let name = profile_display_name(&st);
                let text = if name.is_empty() {
                    tr_fmt(
                        "Writing files to {target}. This may take a while for large restores.",
                        &[("target", target_dir)],
                    )
                } else {
                    tr_fmt(
                        "Writing files from «{name}» to {target}. This may take a while.",
                        &[("name", &name), ("target", target_dir)],
                    )
                };
                detail.set_label(&text);
            }
            if let Some(bar) = find_child_by_name(page, "restore-status-progress")
                .and_then(|w| w.downcast::<gtk::ProgressBar>().ok())
            {
                bar.set_fraction(0.0);
            }
        }
        drop(st);
        if active {
            start_restore_pulse(page, state);
        } else {
            stop_restore_pulse(state);
        }
    }
    update_action_buttons(page, state);
}

fn apply_original_restore_target(target_row: &libadwaita::EntryRow, state: &Rc<RefCell<PageState>>) {
    let st = state.borrow();
    let archive = st
        .archive
        .as_deref()
        .or_else(|| st.archive_items.first().map(|a| a.archive_name.as_str()));
    let Some(archive) = archive else {
        return;
    };
    let rel = if st.browse_mode == BrowseMode::Inside {
        st.browse_path.as_str()
    } else {
        ""
    };
    if let Some(dir) = original_restore_target_dir(&st.profile_paths, archive, rel) {
        target_row.set_text(dir.to_string_lossy().as_ref());
    }
}

fn resolve_restore_target(
    state: &PageState,
    manual_target: &str,
    use_original: bool,
    patterns: &[String],
) -> Option<String> {
    if use_original {
        let archive = if patterns.is_empty() {
            state.archive.as_deref()
        } else {
            state
                .selected_entries
                .values()
                .next()
                .map(|e| e.archive.as_str())
        };
        let archive = archive.or_else(|| state.archive_items.first().map(|a| a.archive_name.as_str()))?;
        let rel = patterns
            .first()
            .map(|p| p.as_str())
            .unwrap_or_else(|| {
                if state.browse_mode == BrowseMode::Inside {
                    state.browse_path.as_str()
                } else {
                    ""
                }
            });
        return original_restore_target_dir(&state.profile_paths, archive, rel)
            .map(|p| p.to_string_lossy().into_owned());
    }
    let trimmed = manual_target.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn start_restore(
    page: &gtk::Widget,
    toast: &ToastOverlay,
    state: &Rc<RefCell<PageState>>,
    target_dir: String,
    overwrite: bool,
    use_original_path: bool,
    patterns: Vec<String>,
) {
    let st = state.borrow();
    let (Some(profile_id), Some(snapshot)) = (st.profile_id, st.snapshot.clone()) else {
        return;
    };
    if st.restore_in_progress {
        return;
    }
    let target_trimmed = resolve_restore_target(&st, &target_dir, use_original_path, &patterns);
    drop(st);
    let Some(target_trimmed) = target_trimmed else {
        show_toast(
            toast,
            &tr("Choose a restore folder or enable restore to original location."),
        );
        return;
    };

    set_restore_in_progress(page, state, true, &target_trimmed);

    let archive = if patterns.is_empty() {
        state.borrow().archive.clone().unwrap_or_default()
    } else {
        let entries = selected_catalog_entries(&state.borrow().selected_entries);
        if entries.is_empty() {
            state.borrow().archive.clone().unwrap_or_default()
        } else {
            let first = entries[0].archive.clone();
            if entries.iter().all(|e| e.archive == first) {
                first
            } else {
                set_restore_in_progress(page, state, false, "");
                show_toast(
                    toast,
                    &tr("Selected files must belong to the same backup archive."),
                );
                return;
            }
        }
    };

    if archive.is_empty() {
        set_restore_in_progress(page, state, false, "");
        show_toast(toast, &tr("No backup archive selected. Reload the file list."));
        return;
    }

    let patterns_retry = patterns.clone();
    let encryption_key_id = encryption_key_id_for_snapshot(&state.borrow());
    let request = RestoreArchiveRequest {
        profile_id,
        snapshot,
        archive_name: archive,
        target_dir: target_trimmed.clone(),
        overwrite,
        patterns,
        encryption_key_id,
    };

    let page = page.clone();
    let state = state.clone();
    let toast = toast.clone();
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::restore_archive(&proxy, &request).await
        },
        move |result| {
            set_restore_in_progress(page.upcast_ref(), &state, false, "");
            match result {
                Ok(res) if res.started => {
                    let msg = res
                        .message
                        .unwrap_or_else(|| tr("Restore started in the background."));
                    let t = libadwaita::Toast::new(&msg);
                    t.set_timeout(8);
                    toast.add_toast(t);
                }
                Ok(res) if !res.conflicts.is_empty() => {
                    let page_retry = page.clone();
                    let toast_retry = toast.clone();
                    let state_retry = state.clone();
                    let target_retry = target_trimmed.clone();
                    let patterns_retry = patterns_retry.clone();
                    show_restore_conflict_dialog(
                        page.upcast_ref(),
                        &res.conflicts,
                        move || {
                            start_restore(
                                page_retry.upcast_ref(),
                                &toast_retry,
                                &state_retry,
                                target_retry,
                                true,
                                false,
                                patterns_retry,
                            );
                        },
                    );
                }
                Ok(res) => {
                    let msg = res
                        .message
                        .unwrap_or_else(|| tr("Restore could not be started"));
                    show_toast(&toast, &msg);
                }
                Err(err) => show_toast(
                    &toast,
                    &tr_fmt("Restore failed: {err}", &[("err", &err.to_string())]),
                ),
            }
        },
    );
}

fn choose_target_folder(
    parent: &ApplicationWindow,
    entry: &libadwaita::EntryRow,
    page: &gtk::Widget,
    state: &Rc<RefCell<PageState>>,
) {
    let dialog = gtk::FileDialog::builder()
        .title(&tr("Choose restore target folder"))
        .modal(true)
        .build();
    let entry = entry.clone();
    let page = page.clone();
    let state = state.clone();
    dialog.select_folder(
        Some(parent),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            if let Ok(folder) = result {
                if let Some(path) = folder.path() {
                    entry.set_text(&path.display().to_string());
                }
            }
            update_action_buttons(page.upcast_ref(), &state);
        },
    );
}

fn append_info_row(list: &gtk::ListBox, text: &str) {
    let row = gtk::ListBoxRow::new();
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_margin_start(12);
    label.set_margin_end(12);
    label.set_margin_top(10);
    label.set_margin_bottom(10);
    label.set_wrap(true);
    label.add_css_class("dim-label");
    row.set_child(Some(&label));
    list.append(&row);
}

fn show_snapshots_hint(page_or_list: &gtk::Widget, text: &str) {
    if let Some(list) = page_or_list.clone().downcast::<gtk::ListBox>().ok() {
        clear_list_box(&list);
        append_info_row(&list, text);
    } else if let Some(list) = find_child_by_name(page_or_list, "restore-snapshots-list")
        .and_then(|w| w.downcast::<gtk::ListBox>().ok())
    {
        clear_list_box(&list);
        append_info_row(&list, text);
    }
}

fn show_restore_conflict_dialog(
    page: &gtk::Widget,
    conflicts: &[String],
    on_overwrite: impl FnOnce() + 'static,
) {
    let preview: Vec<_> = conflicts.iter().take(8).cloned().collect();
    let mut body = tr_fmt(
        "{count} file(s) already exist at the target. Overwrite them?",
        &[("count", &conflicts.len().to_string())],
    );
    if !preview.is_empty() {
        body.push_str("\n\n");
        body.push_str(&preview.join("\n"));
        if conflicts.len() > preview.len() {
            body.push_str("\n…");
        }
    }

    let alert = libadwaita::AlertDialog::builder()
        .heading(&tr("Files already exist"))
        .body(&body)
        .build();
    alert.add_response("cancel", &tr("Cancel"));
    alert.add_response("overwrite", &tr("Overwrite"));
    alert.set_response_appearance("overwrite", libadwaita::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");

    let on_overwrite = std::cell::RefCell::new(Some(on_overwrite));
    alert.connect_response(None::<&str>, move |_, response| {
        if response == "overwrite" {
            if let Some(callback) = on_overwrite.borrow_mut().take() {
                callback();
            }
        }
    });

    if let Some(window) = page
        .root()
        .and_then(|r| r.downcast::<ApplicationWindow>().ok())
    {
        alert.present(Some(&window));
    }
}

/// Shorten verbose PBS stderr for UI (drop usage blocks).
fn summarize_pbs_error(err: &str) -> String {
    let mut lines = err
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| !l.starts_with("Usage:"))
        .filter(|l| !l.starts_with("Optional parameters:"))
        .filter(|l| !l.starts_with("--"))
        .filter(|l| !l.starts_with('<'))
        .filter(|l| !l.chars().next().is_some_and(|c| c.is_whitespace()));
    let first = lines.next().unwrap_or(err).to_string();
    if first.len() > 240 {
        format!("{}…", &first[..240])
    } else {
        first
    }
}

fn show_toast(toast: &ToastOverlay, message: &str) {
    let t = libadwaita::Toast::new(&crate::util::escape_pango_markup(message));
    t.set_timeout(8);
    toast.add_toast(t);
}
