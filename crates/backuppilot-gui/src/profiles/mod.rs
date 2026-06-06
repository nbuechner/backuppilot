pub mod editor;
mod profile_io;
mod preflight_panel;
mod string_list_editor;
mod run_history;

use std::sync::Once;

use backuppilot_core::pbs_repository::PbsRepositoryParts;
use backuppilot_core::profile::{BackupProfile, ProfileStatus};
use backuppilot_i18n::{tr, tr_fmt};
use gtk::prelude::*;
use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, ToastOverlay};

const CLOUD_BACKUP_URL: &str = "https://www.backup-as-a-service.cloud/";

use crate::backup_actions;
use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::util::{clear_list_box, find_child_by_name};
use crate::window::{in_progress_status_text, run_status_label};

pub fn build_page(
    parent: &libadwaita::ApplicationWindow,
    toast_overlay: &ToastOverlay,
) -> gtk::Widget {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    page.set_widget_name("profiles-page");

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    let add_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text(&tr("Add profile"))
        .css_classes(["suggested-action"])
        .build();

    let import_btn = gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .tooltip_text(&tr("Import profile from YAML"))
        .build();

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&spacer);
    header.append(&import_btn);
    header.append(&add_btn);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.set_widget_name("profiles-list");

    let refresh = {
        let page = page.clone();
        let parent = parent.clone();
        let toast = toast_overlay.clone();
        move || refresh_list(page.upcast_ref::<gtk::Widget>(), &parent, &toast)
    };

    add_btn.connect_clicked({
        let parent = parent.clone();
        let refresh = refresh.clone();
        move |_| {
            editor::open(&parent, None, refresh.clone());
        }
    });

    import_btn.connect_clicked({
        let parent = parent.clone();
        let toast = toast_overlay.clone();
        let refresh = refresh.clone();
        move |_| {
            profile_io::import_profile_yaml(&parent, &toast, refresh.clone());
        }
    });

    page.append(&header);
    page.append(&list);
    page.append(&build_cloud_backup_promo(parent));

    // Gesamte Seite scrollen, damit die Promo-Box unter der (ggf. kurzen) Liste sichtbar bleibt.
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&page)
        .build();

    scroll.upcast()
}

const PARTNER_INFO_CSS: &str = r#"
.backuppilot-partner-info {
  background-color: alpha(@window_fg_color, 0.05);
  border: 1px solid alpha(@borders, 0.65);
  border-radius: 10px;
  padding: 10px 6px 4px 6px;
}
.backuppilot-partner-info .partner-info-icon {
  opacity: 0.55;
}
.backuppilot-partner-info.health-warning {
  border-color: alpha(@warning_color, 0.4);
}
.backuppilot-partner-info.health-warning .partner-info-icon {
  color: @warning_color;
  opacity: 1;
}
.backuppilot-partner-info.health-critical {
  border-color: alpha(@error_color, 0.4);
}
.backuppilot-partner-info.health-critical .partner-info-icon {
  color: @error_color;
  opacity: 1;
}
"#;

/// Rounded info callout (same look as the Backup-as-a-Service box on the profiles page).
pub(crate) fn build_info_callout(title: &str, body: &str) -> gtk::Box {
    build_info_callout_with_icon("dialog-information-symbolic", None, title, body)
}

/// Info callout with a custom icon and optional extra CSS classes on the frame.
pub(crate) fn build_info_callout_with_icon(
    icon_name: &str,
    frame_class: Option<&str>,
    title: &str,
    body: &str,
) -> gtk::Box {
    ensure_partner_info_styles();
    let wrapper = match frame_class {
        Some(extra) => gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["backuppilot-partner-info", extra])
            .margin_top(4)
            .margin_bottom(4)
            .build(),
        None => gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["backuppilot-partner-info"])
            .margin_top(4)
            .margin_bottom(4)
            .build(),
    };
    wrapper.append(&build_callout_header(icon_name, title, body));
    wrapper
}

pub(crate) fn build_info_callout_header(title: &str, body: &str) -> gtk::Widget {
    build_callout_header("dialog-information-symbolic", title, body)
}

fn build_callout_header(icon_name: &str, title: &str, body: &str) -> gtk::Widget {
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(10)
        .margin_bottom(10)
        .build();

    header.append(&callout_icon(icon_name, 22));

    let labels = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();

    labels.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["title-4"])
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .build(),
    );
    labels.append(
        &gtk::Label::builder()
            .label(body)
            .xalign(0.0)
            .css_classes(["dim-label"])
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .build(),
    );

    header.append(&labels);
    header.upcast()
}

fn ensure_partner_info_styles() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(PARTNER_INFO_CSS);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}

fn build_cloud_backup_promo(parent: &ApplicationWindow) -> gtk::Widget {
    ensure_partner_info_styles();

    let wrapper = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["backuppilot-partner-info"])
        .margin_top(12)
        .build();

    wrapper.append(&build_info_callout_header(
        &tr("Modern backup. Without compromise."),
        &tr("Proxmox Backup as a Service , hosted in Switzerland, operated by experts."),
    ));

    let group = libadwaita::PreferencesGroup::new();

    let link_row = libadwaita::ActionRow::builder()
        .title(&tr("Backup as a Service by OneSystems GmbH"))
        .subtitle(CLOUD_BACKUP_URL)
        .activatable(true)
        .build();
    link_row.add_suffix(
        &gtk::Image::builder()
            .icon_name("external-link-symbolic")
            .pixel_size(16)
            .css_classes(["dim-label"])
            .build(),
    );
    let parent_link = parent.clone();
    link_row.connect_activated(move |_| open_cloud_backup_url(&parent_link));
    group.add(&link_row);

    wrapper.append(&group);
    wrapper.upcast()
}

fn callout_icon(icon_name: &str, size: i32) -> gtk::Image {
    gtk::Image::builder()
        .icon_name(icon_name)
        .pixel_size(size)
        .css_classes(["partner-info-icon", "dim-label"])
        .valign(gtk::Align::Start)
        .build()
}

fn open_cloud_backup_url(parent: &ApplicationWindow) {
    let launcher = gtk::UriLauncher::new(CLOUD_BACKUP_URL);
    let parent = parent.clone();
    launcher.launch(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            if let Err(err) = result {
                tracing::warn!(%err, "failed to open cloud backup website");
            }
        },
    );
}

pub fn refresh_list(
    page: &gtk::Widget,
    parent: &libadwaita::ApplicationWindow,
    toast_overlay: &ToastOverlay,
) {
    let list = find_child_by_name(page, "profiles-list");
    let page = page.clone();
    let parent = parent.clone();
    let toast_overlay = toast_overlay.clone();

    dbus_runtime::spawn(
        async move { daemon_list_profiles_and_statuses().await },
        move |result| {
            let Some(list) = list.and_then(|w| w.downcast::<gtk::ListBox>().ok()) else {
                return;
            };
            clear_list_box(&list);

            let (profiles, statuses) = match result {
                Ok(p) => p,
                Err(err) => {
                    append_error_row(&list, &tr_fmt("Error: {err}", &[("err", &err.to_string())]));
                    return;
                }
            };

            let status_by_id: std::collections::HashMap<i64, ProfileStatus> = statuses
                .into_iter()
                .map(|s| (s.profile_id, s))
                .collect();

            if profiles.is_empty() {
                let row = gtk::ListBoxRow::new();
                let label = gtk::Label::new(Some(&tr("No profiles yet. Click + to create your first backup profile.")));
                label.set_xalign(0.0);
                label.set_margin_start(12);
                label.set_margin_end(12);
                label.set_margin_top(10);
                label.set_margin_bottom(10);
                label.set_wrap(true);
                row.set_child(Some(&label));
                list.append(&row);
                return;
            }

            let profiles_page_widget = page.clone();
            for profile in profiles {
                let page = page.clone();
                let parent = parent.clone();
                let toast_overlay = toast_overlay.clone();
                let profiles_page_widget = profiles_page_widget.clone();
                let status = status_by_id.get(&profile.id).cloned();
                list.append(&profile_row(
                    &profile,
                    status.as_ref(),
                    parent.clone(),
                    toast_overlay.clone(),
                    profiles_page_widget,
                    move || refresh_list(&page, &parent, &toast_overlay),
                ));
            }
        },
    );
}

fn profile_row(
    profile: &BackupProfile,
    status: Option<&ProfileStatus>,
    parent: libadwaita::ApplicationWindow,
    toast: ToastOverlay,
    profiles_page: gtk::Widget,
    on_change: impl Fn() + 'static + Clone,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .valign(gtk::Align::Center)
        .build();

    let info = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .build();

    let name = gtk::Label::builder()
        .label(&profile.name)
        .xalign(0.0)
        .css_classes(["title-4"])
        .build();

    let pbs_host = PbsRepositoryParts::parse(&profile.repository)
        .map(|p| p.host)
        .unwrap_or_else(|_| profile.repository.clone());

    let run_line = if let Some(s) = status {
        if s.backup_in_progress {
            in_progress_status_text(s)
        } else if let Some(run) = &s.last_run {
            let label = run_status_label(run);
            if let Some(msg) = &run.error_message {
                tr_fmt("Last run: {label} , {msg}", &[("label", &label), ("msg", msg)])
            } else {
                tr_fmt("Last run: {label}", &[("label", &label)])
            }
        } else {
            tr("No backup run yet")}
    } else {
        String::new()
    };

    let status_hint = if profile.enabled {
        tr("Enabled")} else {
        tr("Disabled")};
    let encryption_hint = if profile.encryption_key_id.is_some() {
        tr("Encrypted")} else {
        tr("Not encrypted")};
    let sub = gtk::Label::builder()
        .label(if run_line.is_empty() {
            format!(
                "{status_hint} · {encryption_hint} · {} · {pbs_host}",
                profile.backup_id
            )
        } else {
            format!(
                "{status_hint} · {encryption_hint} · {} · {pbs_host}\n{run_line}",
                profile.backup_id
            )
        })
        .xalign(0.0)
        .css_classes(["dim-label"])
        .wrap(true)
        .build();

    info.append(&name);
    info.append(&sub);

    let backup_in_progress = status.is_some_and(|s| s.backup_in_progress);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["linked"])
        .valign(gtk::Align::Center)
        .build();

    let export_btn = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .tooltip_text(&tr("Export profile to YAML"))
        .css_classes(["flat"])
        .build();

    let edit_btn = gtk::Button::builder()
        .icon_name("edit-symbolic")
        .tooltip_text(&tr("Edit profile"))
        .css_classes(["flat"])
        .build();

    let history_btn = gtk::Button::builder()
        .icon_name("document-open-recent-symbolic")
        .tooltip_text(&tr("Backup history"))
        .css_classes(["flat"])
        .build();

    let backup_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text(&tr("Start backup"))
        .css_classes(["flat"])
        .sensitive(!backup_in_progress)
        .build();

    let stop_btn = gtk::Button::builder()
        .icon_name("process-stop-symbolic")
        .tooltip_text(&tr("Cancel backup"))
        .css_classes(["flat", "destructive-action"])
        .visible(backup_in_progress)
        .sensitive(backup_in_progress)
        .build();

    let profile_id_export = profile.id;
    let profile_name_export = profile.name.clone();
    let parent_export = parent.clone();
    let toast_export = toast.clone();
    export_btn.connect_clicked(move |_| {
        profile_io::export_profile_yaml(
            &parent_export,
            &toast_export,
            profile_id_export,
            &profile_name_export,
        );
    });

    let profile_for_edit = profile.clone();
    let on_change_edit = on_change.clone();
    edit_btn.connect_clicked({
        let parent = parent.clone();
        let profile = profile_for_edit.clone();
        let on_change = on_change_edit.clone();
        move |_| editor::open(&parent, Some(profile.clone()), on_change.clone())
    });

    let profile_for_activate = profile.clone();
    let parent_activate = parent.clone();
    let on_change_activate = on_change.clone();
    row.connect_activate(move |_| {
        editor::open(
            &parent_activate,
            Some(profile_for_activate.clone()),
            on_change_activate.clone(),
        );
    });

    let profile_id = profile.id;
    let toast_backup = toast.clone();
    let parent_backup = parent.clone();
    backup_btn.connect_clicked({
        let profiles_page = profiles_page.clone();
        move |_| {
            backup_actions::start_backup(
                profile_id,
                &parent_backup,
                &toast_backup,
                &profiles_page,
            );
        }
    });

    let profile_id_stop = profile.id;
    let toast_stop = toast.clone();
    let profiles_page_stop = profiles_page.clone();
    stop_btn.connect_clicked(move |_| {
        backup_actions::cancel_backup(
            profile_id_stop,
            Some(&toast_stop),
            Some(&profiles_page_stop),
        );
    });

    let profile_id_history = profile.id;
    let profile_name_history = profile.name.clone();
    let parent_history = parent.clone();
    history_btn.connect_clicked(move |_| {
        run_history::open_dialog(&parent_history, profile_id_history, &profile_name_history);
    });

    actions.append(&export_btn);
    actions.append(&edit_btn);
    actions.append(&history_btn);
    actions.append(&backup_btn);
    actions.append(&stop_btn);

    let (enc_icon, enc_tooltip) = if profile.encryption_key_id.is_some() {
        (
            "security-high-symbolic",
            tr("Backup encryption is active for this profile"),
        )
    } else {
        (
            "security-low-symbolic",
            tr("Backups from this profile are not encrypted"),
        )
    };
    let enc_css = if profile.encryption_key_id.is_some() {
        &["success"][..]
    } else {
        &["dim-label"][..]
    };
    let encryption_icon = gtk::Image::builder()
        .icon_name(enc_icon)
        .pixel_size(16)
        .css_classes(enc_css)
        .tooltip_text(&enc_tooltip)
        .valign(gtk::Align::Center)
        .margin_end(4)
        .build();

    row_box.append(&info);
    row_box.append(&encryption_icon);
    row_box.append(&actions);
    row.set_child(Some(&row_box));
    row
}

fn append_error_row(list: &gtk::ListBox, message: &str) {
    let row = gtk::ListBoxRow::new();
    let label = gtk::Label::new(Some(message));
    label.set_xalign(0.0);
    label.set_margin_start(12);
    label.set_margin_end(12);
    label.set_margin_top(10);
    label.set_margin_bottom(10);
    label.set_wrap(true);
    row.set_child(Some(&label));
    list.append(&row);
}

async fn daemon_list_profiles_and_statuses(
) -> backuppilot_ipc::Result<(Vec<BackupProfile>, Vec<ProfileStatus>)> {
    let proxy = connect().await?;
    let profiles = dbus_client::list_profiles(&proxy).await?;
    let statuses = dbus_client::list_statuses(&proxy).await?;
    Ok((profiles, statuses))
}

