//! Human-readable activity log lines and detail views.

use backuppilot_core::profile::{ActivityLogEntry, BackupRun, RunStatus};
use backuppilot_core::{display_backup_error_message, should_show_backup_failure_detail};
use backuppilot_i18n::{tr, tr_fmt};
use chrono::{DateTime, Local, Utc};
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::util;

/// How an activity row behaves in the list.
#[derive(Debug, Clone, Copy)]
pub struct ActivityRowMode {
    pub open_detail_on_activate: bool,
    pub show_copy_button: bool,
}

impl ActivityRowMode {
    pub const DASHBOARD: Self = Self {
        open_detail_on_activate: true,
        show_copy_button: false,
    };

    pub const LOG_PAGE: Self = Self {
        open_detail_on_activate: true,
        show_copy_button: true,
    };
}

const ACTIVITY_SUBTITLE_MAX_CHARS: usize = 200;

fn truncate_ui_line(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str("…");
    out
}

pub fn build_activity_row(
    entry: &ActivityLogEntry,
    parent: Option<&libadwaita::ApplicationWindow>,
    mode: ActivityRowMode,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let (_icon, css) = status_icon(entry.run.status);
    let title = if entry.is_system {
        system_activity_title(entry)
    } else {
        activity_title(&entry.profile_name, &entry.run)
    };
    let subtitle = truncate_ui_line(
        &(if entry.is_system {
            system_activity_subtitle(entry)
        } else {
            activity_subtitle(&entry.run)
        }),
        ACTIVITY_SUBTITLE_MAX_CHARS,
    );
    let copy_text = activity_copy_text(entry, &title, &subtitle);

    let action = libadwaita::ActionRow::builder()
        .title(&title)
        .subtitle(&subtitle)
        .activatable(mode.open_detail_on_activate || mode.show_copy_button)
        .build();
    action.add_prefix(&crate::util::status_prefix_label(css));

    if mode.show_copy_button {
        let copy_btn = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(&tr("Copy to clipboard"))
            .build();
        copy_btn.add_css_class("flat");

        let copy_for_btn = copy_text.clone();
        copy_btn.connect_clicked(move |_| {
            copy_activity_to_clipboard(&copy_for_btn);
        });
        action.add_suffix(&copy_btn);
    }

    if mode.open_detail_on_activate {
        if let Some(parent) = parent {
            let parent = parent.clone();
            let entry = entry.clone();
            action.connect_activated(move |_| {
                crate::debug::log_ui("click", "activity detail");
                present_activity_detail(&parent, &entry);
            });
        }
    } else if mode.show_copy_button {
        let copy_for_row = copy_text;
        action.connect_activated(move |_| {
            copy_activity_to_clipboard(&copy_for_row);
        });
    }

    row.set_child(Some(&action));
    row
}

/// Full debug text for the detail popup.
pub fn activity_detail_debug_text(entry: &ActivityLogEntry) -> String {
    let run = &entry.run;
    let mut lines = Vec::new();

    if entry.is_system {
        lines.push(tr_fmt(
            "Type: system event ({kind})",
            &[(
                "kind",
                entry.system_kind.as_deref().unwrap_or("unknown"),
            )],
        ));
    } else {
        lines.push(tr_fmt(
            "Profile: {name} (id {id})",
            &[("name", &entry.profile_name), ("id", &entry.profile_id.to_string())],
        ));
    }

    lines.push(tr_fmt("Run id: {id}", &[("id", &run.id.to_string())]));
    lines.push(tr_fmt(
        "Status: {status}",
        &[("status", &run_status_label(run.status))],
    ));
    lines.push(tr_fmt(
        "Started (UTC): {ts}",
        &[("ts", &run.started_at.to_rfc3339())],
    ));
    if let Some(finished) = run.finished_at {
        lines.push(tr_fmt(
            "Finished (UTC): {ts}",
            &[("ts", &finished.to_rfc3339())],
        ));
        let secs = (finished - run.started_at).num_seconds();
        lines.push(tr_fmt("Duration: {secs} s", &[("secs", &secs.to_string())]));
    }
    if run.bytes_uploaded > 0 {
        lines.push(tr_fmt(
            "Bytes uploaded: {bytes}",
            &[("bytes", &run.bytes_uploaded.to_string())],
        ));
    }
    if let Some(snap) = run.snapshot_id.as_deref().filter(|s| !s.is_empty()) {
        lines.push(tr_fmt("Snapshot id: {snap}", &[("snap", snap)]));
    }

    if let Some(msg) = run.error_message.as_deref().filter(|m| !m.is_empty()) {
        lines.push(String::new());
        lines.push(tr("Error / details:"));
        lines.push(msg.to_string());
    }

    lines.join("\n")
}

pub fn present_activity_detail(parent: &libadwaita::ApplicationWindow, entry: &ActivityLogEntry) {
    let title = if entry.is_system {
        system_activity_title(entry)
    } else {
        activity_title(&entry.profile_name, &entry.run)
    };
    let body = activity_detail_debug_text(entry);

    let dialog = libadwaita::Window::builder()
        .title(&tr("Log entry"))
        .transient_for(parent)
        .modal(true)
        .default_width(560)
        .default_height(420)
        .build();

    let header = libadwaita::HeaderBar::new();

    let copy_btn = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text(&tr("Copy to clipboard"))
        .build();
    let copy_body = body.clone();
    copy_btn.connect_clicked(move |_| {
        copy_activity_to_clipboard(&copy_body);
    });
    header.pack_start(&copy_btn);

    let title_label = gtk::Label::builder()
        .label(&title)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    header.set_title_widget(Some(&title_label));

    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text(&tr("Close"))
        .build();
    close.connect_clicked({
        let dialog = dialog.clone();
        move |_| dialog.close()
    });
    header.pack_end(&close);

    let text_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(true)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(12)
        .right_margin(12)
        .top_margin(12)
        .bottom_margin(12)
        .build();
    text_view.buffer().set_text(&body);

    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&text_view)
        .build();

    let toolbar = libadwaita::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroll));
    dialog.set_content(Some(&toolbar));
    dialog.present();
}

fn run_status_label(status: RunStatus) -> String {
    match status {
        RunStatus::Pending => tr("pending"),
        RunStatus::Running => tr("running"),
        RunStatus::Success => tr("successful"),
        RunStatus::Failed => tr("failed"),
        RunStatus::Skipped => tr("skipped"),
        RunStatus::Cancelled => tr("cancelled"),
    }
}

fn copy_activity_to_clipboard(text: &str) {
    if util::copy_text_to_clipboard(text) {
        util::show_toast(&tr("Copied to clipboard"));
    } else {
        util::show_toast(&tr("Could not copy to clipboard"));
    }
}

/// Full line for clipboard (title and full error/details when present).
pub fn activity_copy_text(entry: &ActivityLogEntry, title: &str, subtitle: &str) -> String {
    if let Some(msg) = entry.run.error_message.as_deref().filter(|m| !m.is_empty()) {
        format!("{title}\n\n{msg}")
    } else {
        format!("{title}\n{subtitle}")
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

fn system_activity_title(entry: &ActivityLogEntry) -> String {
    match entry.system_kind.as_deref() {
        Some("pbs_client_install") => match entry.run.status {
            RunStatus::Success => tr("Proxmox Backup Client installed successfully"),
            RunStatus::Failed => tr("Proxmox Backup Client installation failed"),
            RunStatus::Running => tr("Proxmox Backup Client installation in progress"),
            RunStatus::Skipped => tr("Proxmox Backup Client installation skipped"),
            RunStatus::Pending => tr("Proxmox Backup Client installation pending"),
            RunStatus::Cancelled => tr("Proxmox Backup Client installation cancelled"),
        },
        Some("snapshot_mount") => match entry.run.status {
            RunStatus::Success => tr("Backup mounted read-only in file manager"),
            RunStatus::Failed => tr("Backup mount failed"),
            _ => tr("Backup mount"),
        },
        Some("snapshot_unmount") => match entry.run.status {
            RunStatus::Success => tr("Backup mount disconnected"),
            RunStatus::Failed => tr("Backup mount disconnect failed"),
            _ => tr("Backup mount disconnect"),
        },
        Some("restore_started") => tr("File restore started"),
        Some("restore_finished") => match entry.run.status {
            RunStatus::Success => tr("File restore completed"),
            RunStatus::Failed => tr("File restore failed"),
            _ => tr("File restore"),
        },
        _ => entry.profile_name.clone(),
    }
}

fn failure_subtitle_reason(run: &BackupRun) -> Option<String> {
    display_backup_error_message(run.error_message.as_deref())
        .filter(|msg| should_show_backup_failure_detail(msg))
}

fn system_activity_subtitle(entry: &ActivityLogEntry) -> String {
    let when = format_when(entry.run.finished_at.unwrap_or(entry.run.started_at));
    match entry.run.status {
        RunStatus::Failed => {
            if let Some(msg) = failure_subtitle_reason(&entry.run) {
                tr_fmt("{when} , {reason}", &[("when", &when), ("reason", &msg)])
            } else {
                when
            }
        }
        RunStatus::Success => when,
        _ => when,
    }
}

pub fn activity_title(profile_name: &str, run: &BackupRun) -> String {
    match run.status {
        RunStatus::Success => tr_fmt(
            "Backup «{name}» completed successfully",
            &[("name", profile_name)],
        ),
        RunStatus::Failed => tr_fmt("Backup «{name}» failed", &[("name", profile_name)]),
        RunStatus::Skipped => tr_fmt("Backup «{name}» was skipped", &[("name", profile_name)]),
        RunStatus::Cancelled => tr_fmt("Backup «{name}» was cancelled", &[("name", profile_name)]),
        RunStatus::Running => tr_fmt("Backup «{name}» is running", &[("name", profile_name)]),
        RunStatus::Pending => tr_fmt("Backup «{name}» is starting", &[("name", profile_name)]),
    }
}

/// Short line for the overview job list (last finished or current run).
pub fn run_summary_subtitle(run: &BackupRun) -> String {
    let when = format_when(run.finished_at.unwrap_or(run.started_at));
    let status = run_status_label(run.status);
    match run.status {
        RunStatus::Failed => {
            if let Some(msg) = failure_subtitle_reason(run) {
                tr_fmt("{status}, {when} , {msg}", &[("status", &status), ("when", &when), ("msg", &msg)])
            } else {
                tr_fmt("{status}, {when}", &[("status", &status), ("when", &when)])
            }
        }
        RunStatus::Skipped | RunStatus::Cancelled => {
            if let Some(msg) = run.error_message.as_deref().filter(|m| !m.is_empty()) {
                tr_fmt("{status}, {when} , {msg}", &[("status", &status), ("when", &when), ("msg", msg)])
            } else {
                tr_fmt("{status}, {when}", &[("status", &status), ("when", &when)])
            }
        }
        RunStatus::Success if run.bytes_uploaded > 0 => tr_fmt(
            "{status}, {when} , {size}",
            &[
                ("status", &status),
                ("when", &when),
                ("size", &format_size(run.bytes_uploaded)),
            ],
        ),
        _ => tr_fmt("{status}, {when}", &[("status", &status), ("when", &when)]),
    }
}

pub fn activity_subtitle(run: &BackupRun) -> String {
    let when = format_when(run.finished_at.unwrap_or(run.started_at));
    match run.status {
        RunStatus::Failed => {
            if let Some(msg) = failure_subtitle_reason(run) {
                tr_fmt("{when} , {reason}", &[("when", &when), ("reason", &msg)])
            } else {
                when
            }
        }
        RunStatus::Skipped => {
            if let Some(msg) = run.error_message.as_deref() {
                tr_fmt("{when} , {reason}", &[("when", &when), ("reason", msg)])
            } else {
                tr_fmt("{when} , not started (preflight or schedule)", &[("when", &when)])
            }
        }
        RunStatus::Cancelled => {
            if let Some(msg) = run.error_message.as_deref() {
                tr_fmt("{when} , {reason}", &[("when", &when), ("reason", msg)])
            } else {
                when
            }
        }
        RunStatus::Success => {
            let size_part = if run.bytes_uploaded > 0 {
                tr_fmt(
                    "{size} saved to backup server",
                    &[("size", &format_size(run.bytes_uploaded))],
                )
            } else {
                String::new()
            };
            if let Some(snap) = run.snapshot_id.as_deref().filter(|s| !s.is_empty()) {
                if size_part.is_empty() {
                    tr_fmt("{when} , snapshot {snap}", &[("when", &when), ("snap", snap)])
                } else {
                    tr_fmt(
                        "{when} , {size_part}, snapshot {snap}",
                        &[("when", &when), ("size_part", &size_part), ("snap", snap)],
                    )
                }
            } else if !size_part.is_empty() {
                tr_fmt("{when} , {size_part}", &[("when", &when), ("size_part", &size_part)])
            } else {
                when
            }
        }
        RunStatus::Running | RunStatus::Pending => when,
    }
}

fn format_when(dt: DateTime<Utc>) -> String {
    let local = dt.with_timezone(&Local);
    let now = Local::now();
    let time = local.format("%H:%M").to_string();
    if local.date_naive() == now.date_naive() {
        tr_fmt("Today at {time}", &[("time", &time)])
    } else if local.date_naive() == now.date_naive() - chrono::Duration::days(1) {
        tr_fmt("Yesterday at {time}", &[("time", &time)])
    } else {
        local.format("%d.%m.%Y, %H:%M").to_string()
    }
}

fn format_size(bytes: u64) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn success_title_is_plain_language() {
        let run = BackupRun {
            id: 1,
            profile_id: 1,
            started_at: Utc.with_ymd_and_hms(2026, 5, 19, 10, 0, 0).unwrap(),
            finished_at: Some(Utc.with_ymd_and_hms(2026, 5, 19, 10, 5, 0).unwrap()),
            status: RunStatus::Success,
            error_message: None,
            bytes_uploaded: 1024,
            snapshot_id: Some("2026-05-19T10:05:00Z".into()),
        };
        let entry = ActivityLogEntry {
            profile_id: 1,
            profile_name: "Home".into(),
            run,
            is_system: false,
            system_kind: None,
        };
        let detail = activity_detail_debug_text(&entry);
        assert!(detail.contains("Home"));
        assert!(detail.contains("2026-05-19T10:05:00Z"));
    }
}
