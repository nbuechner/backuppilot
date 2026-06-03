//! Full backup activity log with filters and detail popup.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use backuppilot_core::profile::{ActivityLogEntry, RunStatus};
use backuppilot_i18n::tr;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::activity_log::{self, ActivityRowMode};
use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::util::clear_list_box;

const LOG_FETCH_LIMIT: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusFilter {
    All,
    Success,
    Failed,
    Skipped,
    Cancelled,
    InProgress,
    System,
}

struct LogPageState {
    all_entries: Vec<ActivityLogEntry>,
    status_filter: StatusFilter,
    profile_index: u32,
    profile_names: Vec<String>,
    search: String,
}

struct LogPageCtx {
    list: gtk::ListBox,
    status_combo: libadwaita::ComboRow,
    profile_combo: libadwaita::ComboRow,
    state: Rc<RefCell<LogPageState>>,
    parent: libadwaita::ApplicationWindow,
}

thread_local! {
    static LOG_CTX: RefCell<Option<LogPageCtx>> = const { RefCell::new(None) };
    static SUPPRESS_LOG_FILTER: Cell<bool> = const { Cell::new(false) };
}

pub fn build_page(parent: &libadwaita::ApplicationWindow, _toast: &libadwaita::ToastOverlay) -> gtk::Widget {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .vexpand(true)
        .hexpand(true)
        .build();

    let filters = libadwaita::PreferencesGroup::builder()
        .title(&tr("Filter"))
        .build();

    let status_labels = status_filter_labels();
    let status_refs: Vec<&str> = status_labels.iter().map(String::as_str).collect();
    let status_combo = libadwaita::ComboRow::builder()
        .title(&tr("Status"))
        .model(&gtk::StringList::new(&status_refs))
        .selected(0)
        .build();

    let profile_combo = libadwaita::ComboRow::builder()
        .title(&tr("Profile"))
        .model(&gtk::StringList::new(&[tr("All profiles").as_str()]))
        .selected(0)
        .build();

    let search_entry = libadwaita::EntryRow::builder()
        .title(&tr("Search"))
        .show_apply_button(false)
        .build();
    search_entry.set_text("");

    filters.add(&status_combo);
    filters.add(&profile_combo);
    filters.add(&search_entry);

    let list_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Backup log"))
        .description(&tr("Click an entry for full details (timestamps, snapshot id, error text)."))
        .build();

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&list)
        .build();

    list_group.add(&scroll);
    list_group.set_vexpand(true);
    list_group.set_hexpand(true);

    page.append(&filters);
    page.append(&list_group);

    let state = Rc::new(RefCell::new(LogPageState {
        all_entries: Vec::new(),
        status_filter: StatusFilter::All,
        profile_index: 0,
        profile_names: Vec::new(),
        search: String::new(),
    }));

    LOG_CTX.with(|slot| {
        *slot.borrow_mut() = Some(LogPageCtx {
            list: list.clone(),
            status_combo: status_combo.clone(),
            profile_combo: profile_combo.clone(),
            state: state.clone(),
            parent: parent.clone(),
        });
    });

    let state_ref = state.clone();
    status_combo.connect_notify_local(Some("selected"), move |combo, _| {
        let idx = combo.selected();
        state_ref.borrow_mut().status_filter = status_filter_from_index(idx);
        apply_filters_from_ctx();
    });

    let state_ref = state.clone();
    profile_combo.connect_notify_local(Some("selected"), move |combo, _| {
        state_ref.borrow_mut().profile_index = combo.selected();
        apply_filters_from_ctx();
    });

    let state_ref = state.clone();
    search_entry.connect_changed(move |entry| {
        state_ref.borrow_mut().search = entry.text().to_string();
        apply_filters_from_ctx();
    });

    show_loading(&list);
    page.upcast()
}

pub fn refresh() {
    LOG_CTX.with(|slot| {
        let borrowed = slot.borrow();
        let Some(ctx) = borrowed.as_ref() else {
            return;
        };
        show_loading(&ctx.list);
        let list = ctx.list.clone();
        dbus_runtime::spawn(
            async move {
                let proxy = connect().await?;
                dbus_client::list_recent_activity(&proxy, LOG_FETCH_LIMIT).await
            },
            move |result| match result {
                Ok(entries) => on_entries_loaded(&list, entries),
                Err(err) => show_list_error(&list, &err.to_string()),
            },
        );
    });
}

fn on_entries_loaded(_list: &gtk::ListBox, entries: Vec<ActivityLogEntry>) {
    LOG_CTX.with(|slot| {
        let borrowed = slot.borrow();
        let Some(ctx) = borrowed.as_ref() else {
            return;
        };
        let names = profile_names_from_entries(&entries);
        {
            let mut state = ctx.state.borrow_mut();
            state.all_entries = entries;
            state.profile_names = names.clone();
            state.profile_index = 0;
            state.status_filter = StatusFilter::All;
        }
        rebuild_profile_combo(&ctx.profile_combo, &names);
        ctx.status_combo.set_selected(0);
        apply_filtered_list(&ctx.list, &ctx.parent, &ctx.state.borrow());
    });
}

fn profile_names_from_entries(entries: &[ActivityLogEntry]) -> Vec<String> {
    let mut names: Vec<String> = entries
        .iter()
        .filter(|e| !e.is_system)
        .map(|e| e.profile_name.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

fn rebuild_profile_combo(combo: &libadwaita::ComboRow, names: &[String]) {
    SUPPRESS_LOG_FILTER.set(true);
    let all = tr("All profiles");
    let mut items = vec![all.as_str()];
    items.extend(names.iter().map(String::as_str));
    combo.set_model(Some(&gtk::StringList::new(&items)));
    combo.set_selected(0);
    SUPPRESS_LOG_FILTER.set(false);
}

fn apply_filters_from_ctx() {
    if SUPPRESS_LOG_FILTER.get() {
        return;
    }
    LOG_CTX.with(|slot| {
        let borrowed = slot.borrow();
        let Some(ctx) = borrowed.as_ref() else {
            return;
        };
        let state = ctx.state.borrow();
        apply_filtered_list(&ctx.list, &ctx.parent, &state);
    });
}

fn apply_filtered_list(
    list: &gtk::ListBox,
    parent: &libadwaita::ApplicationWindow,
    state: &LogPageState,
) {
    let filtered: Vec<&ActivityLogEntry> = state
        .all_entries
        .iter()
        .filter(|e| matches_status_filter(e, state.status_filter))
        .filter(|e| matches_profile_filter(e, state))
        .filter(|e| matches_search(e, &state.search))
        .collect();

    clear_list_box(list);
    if filtered.is_empty() {
        let row = gtk::ListBoxRow::new();
        let message = if state.all_entries.is_empty() {
            tr("No log entries yet.")
        } else {
            tr("No log entries match the current filters.")
        };
        let label = gtk::Label::builder()
            .label(&message)
            .xalign(0.0)
            .wrap(true)
            .margin_start(12)
            .margin_end(12)
            .margin_top(10)
            .margin_bottom(10)
            .css_classes(["dim-label"])
            .build();
        row.set_child(Some(&label));
        list.append(&row);
        return;
    }

    for entry in filtered {
        list.append(&activity_log::build_activity_row(
            entry,
            Some(parent),
            ActivityRowMode::LOG_PAGE,
        ));
    }
}

fn matches_status_filter(entry: &ActivityLogEntry, filter: StatusFilter) -> bool {
    match filter {
        StatusFilter::All => true,
        StatusFilter::System => entry.is_system,
        StatusFilter::Success => !entry.is_system && entry.run.status == RunStatus::Success,
        StatusFilter::Failed => !entry.is_system && entry.run.status == RunStatus::Failed,
        StatusFilter::Skipped => !entry.is_system && entry.run.status == RunStatus::Skipped,
        StatusFilter::Cancelled => !entry.is_system && entry.run.status == RunStatus::Cancelled,
        StatusFilter::InProgress => {
            !entry.is_system
                && matches!(
                    entry.run.status,
                    RunStatus::Running | RunStatus::Pending
                )
        }
    }
}

fn matches_profile_filter(entry: &ActivityLogEntry, state: &LogPageState) -> bool {
    if state.profile_index == 0 {
        return true;
    }
    let idx = (state.profile_index as usize).saturating_sub(1);
    if idx >= state.profile_names.len() {
        return true;
    }
    state
        .profile_names
        .get(idx)
        .is_some_and(|name| &entry.profile_name == name)
}

fn matches_search(entry: &ActivityLogEntry, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let title = if entry.is_system {
        entry.profile_name.to_lowercase()
    } else {
        activity_log::activity_title(&entry.profile_name, &entry.run).to_lowercase()
    };
    let subtitle = activity_log::activity_subtitle(&entry.run).to_lowercase();
    let err = entry
        .run
        .error_message
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    title.contains(&q) || subtitle.contains(&q) || err.contains(&q)
}

fn status_filter_labels() -> Vec<String> {
    vec![
        tr("All"),
        tr("Successful"),
        tr("Failed"),
        tr("Skipped"),
        tr("Cancelled"),
        tr("In progress"),
        tr("System events"),
    ]
}

fn status_filter_from_index(index: u32) -> StatusFilter {
    match index {
        1 => StatusFilter::Success,
        2 => StatusFilter::Failed,
        3 => StatusFilter::Skipped,
        4 => StatusFilter::Cancelled,
        5 => StatusFilter::InProgress,
        6 => StatusFilter::System,
        _ => StatusFilter::All,
    }
}

fn show_loading(list: &gtk::ListBox) {
    clear_list_box(list);
    let row = gtk::ListBoxRow::new();
    let label = gtk::Label::builder()
        .label(&tr("Loading log…"))
        .xalign(0.0)
        .margin_start(12)
        .margin_end(12)
        .margin_top(10)
        .margin_bottom(10)
        .css_classes(["dim-label"])
        .build();
    row.set_child(Some(&label));
    list.append(&row);
}

fn show_list_error(list: &gtk::ListBox, err: &str) {
    clear_list_box(list);
    let row = gtk::ListBoxRow::new();
    let label = gtk::Label::builder()
        .label(&format!("{} {err}", tr("Could not load log:")))
        .xalign(0.0)
        .wrap(true)
        .margin_start(12)
        .margin_end(12)
        .css_classes(["error"])
        .build();
    row.set_child(Some(&label));
    list.append(&row);
}
