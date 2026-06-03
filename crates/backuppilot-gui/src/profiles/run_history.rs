//! Backup run history for a single profile (popup from the profiles page).

use backuppilot_core::profile::{BackupRun, RunStatus};
use backuppilot_i18n::{tr, tr_fmt};
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::activity_log;
use crate::dbus_client::{self, connect};
use crate::dbus_runtime;

const HISTORY_LIMIT: u32 = 50;

pub fn build_group() -> (libadwaita::PreferencesGroup, gtk::ListBox) {
    let group = libadwaita::PreferencesGroup::new();

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.set_widget_name("profile-run-history-list");

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&list)
        .build();

    group.add(&scroll);
    group.set_vexpand(true);
    group.set_hexpand(true);

    (group, list)
}

pub fn fill_list(list: &gtk::ListBox, profile_name: &str, runs: &[BackupRun]) {
    crate::util::clear_list_box(list);

    if runs.is_empty() {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::builder()
            .label(&tr("No backup runs recorded yet."))
            .xalign(0.0)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(8)
            .build();
        label.add_css_class("dim-label");
        row.set_child(Some(&label));
        list.append(&row);
        return;
    }

    for run in runs {
        let row = gtk::ListBoxRow::new();
        let (icon, css) = status_icon(run.status);
        let title = activity_log::activity_title(profile_name, run);
        let subtitle = activity_log::run_summary_subtitle(run);
        let action = libadwaita::ActionRow::builder()
            .title(&title)
            .subtitle(&subtitle)
            .activatable(false)
            .build();
        action.add_prefix(
            &gtk::Image::builder()
                .icon_name(icon)
                .pixel_size(18)
                .css_classes([css])
                .build(),
        );
        row.set_child(Some(&action));
        list.append(&row);
    }
}

fn status_icon(status: RunStatus) -> (&'static str, &'static str) {
    match status {
        RunStatus::Success => ("emblem-ok-symbolic", "success"),
        RunStatus::Failed => ("dialog-error-symbolic", "error"),
        RunStatus::Skipped => ("dialog-warning-symbolic", "warning"),
        RunStatus::Cancelled => ("process-stop-symbolic", "warning"),
        RunStatus::Running => ("view-refresh-symbolic", "accent"),
        RunStatus::Pending => ("clock-symbolic", "dim-label"),
    }
}

pub fn load_history(list: &gtk::ListBox, profile_id: i64, profile_name: &str) {
    let list = list.clone();
    let profile_name = profile_name.to_string();
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::list_runs_for_profile(&proxy, profile_id, HISTORY_LIMIT).await
        },
        move |result| match result {
            Ok(runs) => fill_list(&list, &profile_name, &runs),
            Err(err) => {
                tracing::warn!(%err, "failed to load profile run history");
                fill_list(&list, &profile_name, &[]);
            }
        },
    );
}

/// Modal window with recent backup runs for one profile.
pub fn open_dialog(
    parent: &libadwaita::ApplicationWindow,
    profile_id: i64,
    profile_name: &str,
) {
    let title = tr_fmt("Backup history {name}", &[("name", profile_name)]);

    let window = libadwaita::Window::builder()
        .title(&title)
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .default_height(480)
        .build();

    let header = libadwaita::HeaderBar::new();
    let title_label = gtk::Label::builder()
        .label(&title)
        .css_classes(["title"])
        .build();
    header.set_title_widget(Some(&title_label));

    let close_btn = gtk::Button::with_label(&tr("Close"));
    header.pack_end(&close_btn);

    let (group, list) = build_group();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .vexpand(true)
        .build();
    content.append(&group);

    let toolbar = libadwaita::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    window.set_content(Some(&toolbar));

    let window_close = window.clone();
    close_btn.connect_clicked(move |_| window_close.destroy());

    load_history(&list, profile_id, profile_name);
    window.present();
}
