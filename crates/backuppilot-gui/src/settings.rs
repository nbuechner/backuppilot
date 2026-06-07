use std::cell::{Cell, RefCell};
use std::rc::Rc;

use backuppilot_core::app_settings::{
    validate_health_check, validate_log_retention, AppSettings, ColorScheme, LogRetentionSettings,
    NotificationSettings, TraySettings, UiLanguage, UpdateChannel, UpdateSettings,
};
use backuppilot_core::profile::HealthCheck;
use backuppilot_core::{load_app_settings, wipe_all_local_data};
use backuppilot_i18n::{tr, tr_fmt};
use gtk::glib;
use gtk::prelude::*;
use libadwaita::prelude::{
    ActionRowExt, AdwDialogExt, AlertDialogExt, ComboRowExt, PreferencesGroupExt,
};
use libadwaita::{ApplicationWindow, Toast, ToastOverlay};

use crate::autostart;
use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::profiles::editor;
use crate::restore;
use crate::advanced_ui;
use crate::settings_apply;
use crate::updates;
use crate::util::escape_pango_markup;
use crate::window;

thread_local! {
    static SETTINGS_WIDGETS: RefCell<Option<Rc<SettingsWidgets>>> = const { RefCell::new(None) };
}

pub fn clear_widgets() {
    SETTINGS_WIDGETS.with(|slot| *slot.borrow_mut() = None);
}

struct SettingsWidgets {
    login_row: libadwaita::SwitchRow,
    require_ac: libadwaita::SwitchRow,
    require_pbs: libadwaita::SwitchRow,
    warn_days: libadwaita::SpinRow,
    critical_days: libadwaita::SpinRow,
    log_retain_days: libadwaita::SpinRow,
    notify_enabled: libadwaita::SwitchRow,
    notify_progress: libadwaita::SwitchRow,
    notify_success: libadwaita::SwitchRow,
    notify_failure: libadwaita::SwitchRow,
    notify_skipped: libadwaita::SwitchRow,
    notify_restore: libadwaita::SwitchRow,
    notify_health: libadwaita::SwitchRow,
    notify_update: libadwaita::SwitchRow,
    show_tray: libadwaita::SwitchRow,
    pause_all: libadwaita::SwitchRow,
    close_to_tray: libadwaita::SwitchRow,
    language_row: libadwaita::ComboRow,
    color_scheme_row: libadwaita::ComboRow,
    update_check: libadwaita::SwitchRow,
    update_channel_row: libadwaita::ComboRow,
    update_notify: libadwaita::SwitchRow,
    update_check_now: libadwaita::ActionRow,
    advanced_mode: gtk::Switch,
}

pub fn build_page(parent: &ApplicationWindow, toast_overlay: &ToastOverlay) -> gtk::Widget {
    let settings = load_app_settings();
    let widgets = Rc::new(build_widgets(&settings));
    SETTINGS_WIDGETS.with(|slot| {
        *slot.borrow_mut() = Some(widgets.clone());
    });

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    page.append(&advanced_ui::build_settings_advanced_strip(&widgets.advanced_mode));
    page.append(&build_startup_group(&widgets));
    page.append(&build_appearance_group(&widgets));

    let conditions_group = build_conditions_group(&widgets);
    let health_group = build_health_group(&widgets);
    let log_group = build_log_group(&widgets);
    let notifications_group = build_notifications_group(&widgets);
    let tray_group = build_tray_group(&widgets);
    let updates_group = if backuppilot_core::app_update_checks_enabled() {
        Some(build_updates_group(&widgets))
    } else {
        None
    };

    let mut advanced_sections: Vec<gtk::Widget> = vec![
        conditions_group.clone().upcast(),
        health_group.clone().upcast(),
        log_group.clone().upcast(),
    ];
    page.append(&conditions_group);
    page.append(&health_group);
    page.append(&log_group);
    page.append(&notifications_group);
    page.append(&tray_group);
    if let Some(updates_group) = updates_group {
        advanced_sections.push(updates_group.clone().upcast());
        page.append(&updates_group);
    }
    let reset_group = build_reset_group(parent, toast_overlay);
    advanced_sections.push(reset_group.clone().upcast());
    page.append(&reset_group);

    advanced_ui::bind_advanced_mode_switch(&widgets.advanced_mode, &advanced_sections);
    apply_settings_advanced_row_visibility(&widgets, advanced_ui::advanced_mode_enabled());
    widgets.advanced_mode.connect_active_notify({
        let widgets = widgets.clone();
        move |sw| {
            apply_settings_advanced_row_visibility(&widgets, sw.is_active());
            update_notify_sensitivity(&widgets);
        }
    });

    let toast = toast_overlay.clone();
    let reverting = Rc::new(Cell::new(false));

    connect_login_row(&widgets.login_row, toast.clone(), reverting.clone());
    connect_persist_rows(&widgets, toast, reverting);
    if backuppilot_core::app_update_checks_enabled() {
        updates::connect_check_button(&widgets.update_check_now);
    }

    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&page)
        .build()
        .upcast()
}

fn build_widgets(settings: &AppSettings) -> SettingsWidgets {
    let login_row = libadwaita::SwitchRow::builder()
        .title(&tr("Start at login"))
        .subtitle(&tr("Show the tray icon and run scheduled backups after you sign in. The main window stays closed until you open it from the tray."))
        .active(autostart::start_at_login_active())
        .build();

    let require_ac = libadwaita::SwitchRow::builder()
        .title(&tr("Only on AC power"))
        .subtitle(&tr("Applies to all backup profiles."))
        .active(settings.conditions.require_ac_power)
        .build();

    let require_pbs = libadwaita::SwitchRow::builder()
        .title(&tr("Check backup server reachability"))
        .subtitle(&tr("Skip backups when the backup server cannot be reached before the run starts."))
        .active(settings.conditions.require_server_reachable)
        .build();

    let warn_adj = gtk::Adjustment::new(
        settings.health_check.warn_after_days as f64,
        1.0,
        365.0,
        1.0,
        5.0,
        0.0,
    );
    let warn_days = libadwaita::SpinRow::builder()
        .title(&tr("Warn after (days)"))
        .subtitle(&tr("Show a warning when no successful backup has run for this long."))
        .adjustment(&warn_adj)
        .build();

    let crit_adj = gtk::Adjustment::new(
        settings.health_check.critical_after_days as f64,
        1.0,
        365.0,
        1.0,
        5.0,
        0.0,
    );
    let critical_days = libadwaita::SpinRow::builder()
        .title(&tr("Critical after (days)"))
        .subtitle(&tr("Mark backup health as critical after this many days without success."))
        .adjustment(&crit_adj)
        .build();

    let log_retain_adj = gtk::Adjustment::new(
        settings.log_retention.retain_days as f64,
        0.0,
        3650.0,
        1.0,
        30.0,
        0.0,
    );
    let log_retain_days = libadwaita::SpinRow::builder()
        .title(&tr("Keep log entries (days)"))
        .subtitle(&tr("Backup runs and system events older than this are removed automatically. Set to 0 to keep all entries."))
        .adjustment(&log_retain_adj)
        .build();

    let notify_enabled = libadwaita::SwitchRow::builder()
        .title(&tr("Desktop notifications"))
        .subtitle(&tr("Show GNOME/libnotify messages from BackupPilot."))
        .active(settings.notifications.enabled)
        .build();

    let notify_progress = libadwaita::SwitchRow::builder()
        .title(&tr("Backup in progress"))
        .subtitle(&tr("Live progress in the GNOME notification center while a backup is running."))
        .active(settings.notifications.notify_on_backup_progress)
        .sensitive(settings.notifications.enabled)
        .build();

    let notify_success = libadwaita::SwitchRow::builder()
        .title(&tr("Successful backups"))
        .active(settings.notifications.notify_on_success)
        .sensitive(settings.notifications.enabled)
        .build();

    let notify_failure = libadwaita::SwitchRow::builder()
        .title(&tr("Failed backups"))
        .active(settings.notifications.notify_on_failure)
        .sensitive(settings.notifications.enabled)
        .build();

    let notify_skipped = libadwaita::SwitchRow::builder()
        .title(&tr("Skipped backups"))
        .subtitle(&tr("For example when preflight checks fail or all backups are paused."))
        .active(settings.notifications.notify_on_skipped)
        .sensitive(settings.notifications.enabled)
        .build();

    let notify_restore = libadwaita::SwitchRow::builder()
        .title(&tr("Restore finished"))
        .active(settings.notifications.notify_on_restore)
        .sensitive(settings.notifications.enabled)
        .build();

    let notify_health = libadwaita::SwitchRow::builder()
        .title(&tr("Overdue backups"))
        .subtitle(&tr("Desktop notification when backup health is warning or critical."))
        .active(settings.notifications.notify_on_health_warning)
        .sensitive(settings.notifications.enabled)
        .build();

    let notify_update = libadwaita::SwitchRow::builder()
        .title(&tr("Updates available"))
        .subtitle(&tr("Desktop notification when a newer release is available."))
        .active(settings.notifications.notify_on_update)
        .sensitive(settings.notifications.enabled)
        .build();

    let show_tray = libadwaita::SwitchRow::builder()
        .title(&tr("Show tray icon"))
        .subtitle(&tr("Restart BackupPilot to apply changes to the tray icon."))
        .active(settings.tray.show_tray_icon)
        .build();

    let pause_all = libadwaita::SwitchRow::builder()
        .title(&tr("Pause all backups"))
        .subtitle(&tr("Scheduled and manual backups are skipped until turned off."))
        .active(settings.tray.pause_all_backups)
        .build();

    let close_to_tray = libadwaita::SwitchRow::builder()
        .title(&tr("Close window to tray"))
        .subtitle(&tr("Closing the main window hides it; the app keeps running in the background."))
        .active(settings.tray.close_to_tray)
        .build();

    let language_row = language_combo_row(settings.appearance.language);
    let color_scheme_row = color_scheme_combo_row(settings.appearance.color_scheme);

    let advanced_mode = gtk::Switch::builder()
        .active(settings.appearance.advanced_mode)
        .valign(gtk::Align::Center)
        .build();

    let update_check = libadwaita::SwitchRow::builder()
        .title(&tr("Check for updates automatically"))
        .subtitle(&tr("Checked daily by the background service against GitLab releases."))
        .active(settings.updates.check_automatically)
        .build();

    let update_channel_row = update_channel_combo_row(settings.updates.channel);
    let update_notify = libadwaita::SwitchRow::builder()
        .title(&tr("Notify when an update is available"))
        .active(settings.updates.notify_when_available)
        .build();

    let update_check_now = libadwaita::ActionRow::builder()
        .title(&tr("Check for updates now"))
        .subtitle(&tr("Uses GitLab releases for this application."))
        .activatable(true)
        .build();
    update_check_now.add_suffix(&gtk::Image::from_icon_name("system-software-update-symbolic"));

    SettingsWidgets {
        login_row,
        require_ac,
        require_pbs,
        warn_days,
        critical_days,
        log_retain_days,
        notify_enabled,
        notify_progress,
        notify_success,
        notify_failure,
        notify_skipped,
        notify_restore,
        notify_health,
        notify_update,
        show_tray,
        pause_all,
        close_to_tray,
        language_row,
        color_scheme_row,
        update_check,
        update_channel_row,
        update_notify,
        update_check_now,
        advanced_mode,
    }
}

fn build_startup_group(widgets: &SettingsWidgets) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Startup"))
        .build();
    group.add(&widgets.login_row);
    group
}

fn build_conditions_group(widgets: &SettingsWidgets) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Backup conditions"))
        .description(&tr("Default conditions and health thresholds for all profiles (applied when you save settings)."))
        .build();
    group.add(&widgets.require_ac);
    group.add(&widgets.require_pbs);
    group
}

fn build_health_group(widgets: &SettingsWidgets) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Health checks"))
        .description(&tr("When to warn about missing successful backups across all profiles."))
        .build();
    group.add(&widgets.warn_days);
    group.add(&widgets.critical_days);
    group
}

fn build_log_group(widgets: &SettingsWidgets) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Activity log"))
        .description(&tr("Limits how much backup history is stored locally. The background service removes older entries once per day."))
        .build();
    group.add(&widgets.log_retain_days);
    group
}

fn build_notifications_group(widgets: &SettingsWidgets) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Notifications"))
        .build();
    group.add(&widgets.notify_enabled);
    group.add(&widgets.notify_progress);
    group.add(&widgets.notify_success);
    group.add(&widgets.notify_failure);
    group.add(&widgets.notify_skipped);
    group.add(&widgets.notify_restore);
    group.add(&widgets.notify_health);
    if backuppilot_core::app_update_checks_enabled() {
        group.add(&widgets.notify_update);
    }
    group
}

fn build_tray_group(widgets: &SettingsWidgets) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(&escape_pango_markup(&tr("Tray & behavior")))
        .build();
    group.add(&widgets.show_tray);
    group.add(&widgets.pause_all);
    group.add(&widgets.close_to_tray);
    group
}

fn build_appearance_group(widgets: &SettingsWidgets) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(&escape_pango_markup(&tr("Language & appearance")))
        .build();
    group.add(&widgets.language_row);
    group.add(&widgets.color_scheme_row);
    group
}

fn apply_settings_advanced_row_visibility(widgets: &SettingsWidgets, advanced: bool) {
    let detail_rows: [&libadwaita::SwitchRow; 8] = [
        &widgets.notify_progress,
        &widgets.notify_success,
        &widgets.notify_failure,
        &widgets.notify_skipped,
        &widgets.notify_restore,
        &widgets.notify_health,
        &widgets.notify_update,
        &widgets.pause_all,
    ];
    for row in detail_rows {
        row.set_visible(advanced);
    }
}

fn build_reset_group(parent: &ApplicationWindow, toast: &ToastOverlay) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Reset"))
        .description(&tr("Remove all local BackupPilot data from this computer."))
        .build();

    let row = libadwaita::ActionRow::builder()
        .title(&tr("Reset all data"))
        .subtitle(&tr("Deletes settings, profiles, backup history, caches and autostart configuration. This cannot be undone."))
        .activatable(false)
        .build();

    let reset_btn = gtk::Button::builder()
        .label(&tr("Reset…"))
        .css_classes(["flat", "destructive-action"])
        .valign(gtk::Align::Center)
        .vexpand(false)
        .hexpand(false)
        .build();

    let parent = parent.clone();
    let toast = toast.clone();
    reset_btn.connect_clicked(move |_| {
        confirm_reset_all_data(&parent, &toast);
    });
    row.add_suffix(&reset_btn);
    group.add(&row);
    group
}

fn confirm_reset_all_data(parent: &ApplicationWindow, toast: &ToastOverlay) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(&tr("Reset BackupPilot?"))
        .body(&tr("All profiles, backup history, settings and cached data on this computer will be permanently deleted."))
        .build();
    alert.add_response("cancel", &tr("Cancel"));
    alert.add_response("reset", &tr("Reset everything"));
    alert.set_response_appearance("reset", libadwaita::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");

    let parent_for_reset = parent.clone();
    let toast = toast.clone();
    alert.connect_response(None::<&str>, move |_, response| {
        if response == "reset" {
            run_reset_all_data(&parent_for_reset, &toast);
        }
    });

    alert.present(Some(parent));
}

fn run_reset_all_data(parent: &ApplicationWindow, toast: &ToastOverlay) {
    let parent = parent.clone();
    let toast = toast.clone();
    dbus_runtime::spawn(
        async {
            match connect().await {
                Ok(proxy) => dbus_client::reset_all_data(&proxy).await,
                Err(_) => wipe_all_local_data().map_err(|e| backuppilot_ipc::IpcError::failure(e.to_string())),
            }
        },
        move |result| {
            match result {
                Ok(()) => on_reset_succeeded(&parent, &toast),
                Err(err) => show_reset_error(&parent, &err.to_string()),
            }
        },
    );
}

fn on_reset_succeeded(parent: &ApplicationWindow, toast: &ToastOverlay) {
    editor::dismiss_any_editor_window();
    settings_apply::apply_loaded();
    SETTINGS_WIDGETS.with(|slot| {
        if let Some(widgets) = slot.borrow().as_ref() {
            reload_widgets(widgets);
            widgets.login_row.set_active(false);
        }
    });
    window::refresh_dashboard_public();
    window::refresh_profiles_public();
    restore::refresh();
    toast.add_toast(Toast::new(&tr("All local data was reset.")));
    let _ = parent;
}

fn show_reset_error(parent: &ApplicationWindow, detail: &str) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(&tr("Reset failed"))
        .body(&tr_fmt("Could not reset: {detail}", &[("detail", detail)]))
        .build();
    alert.add_response("ok", &tr("OK"));
    alert.present(Some(parent));
}

fn build_updates_group(widgets: &SettingsWidgets) -> libadwaita::PreferencesGroup {
    let description = if backuppilot_core::is_flatpak_runtime() {
        tr("Checks GitLab once per day for new releases. When an update is available, you can open the release page to download a Flatpak bundle (no automatic install).")
    } else {
        tr("Checks GitLab once per day for new stable or beta releases. When an update is available, you are asked before .deb or .rpm packages are downloaded and installed.")
    };
    let group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Updates"))
        .description(&description)
        .build();
    group.add(&widgets.update_check);
    group.add(&widgets.update_channel_row);
    group.add(&widgets.update_notify);
    group.add(&widgets.update_check_now);
    group
}

fn language_combo_row(selected: UiLanguage) -> libadwaita::ComboRow {
    let model = gtk::StringList::new(&[
        &tr("System default"),
        &tr("German"),
        &tr("English"),
        &tr("French"),
        &tr("Italian"),
    ]);
    let row = libadwaita::ComboRow::builder()
        .title(&tr("Language"))
        .subtitle(&tr(
            "The interface updates immediately. Notifications and the background service use the new language after a full restart.",
        ))
        .model(&model)
        .build();
    row.set_selected(settings_apply::language_dropdown_index(selected));
    row
}

fn color_scheme_combo_row(selected: ColorScheme) -> libadwaita::ComboRow {
    let model = gtk::StringList::new(&[
        &tr("System default"),
        &tr("Light"),
        &tr("Dark"),
    ]);
    let row = libadwaita::ComboRow::builder()
        .title(&tr("Appearance"))
        .model(&model)
        .build();
    row.set_selected(settings_apply::color_scheme_dropdown_index(selected));
    row
}

fn update_channel_combo_row(selected: UpdateChannel) -> libadwaita::ComboRow {
    let model = gtk::StringList::new(&[&tr("Stable"), &tr("Beta (preview)")]);
    let row = libadwaita::ComboRow::builder()
        .title(&tr("Release channel"))
        .model(&model)
        .build();
    row.set_selected(settings_apply::update_channel_dropdown_index(selected));
    row
}

fn connect_login_row(
    login_row: &libadwaita::SwitchRow,
    toast: ToastOverlay,
    reverting: Rc<Cell<bool>>,
) {
    login_row.connect_active_notify({
        let row = login_row.clone();
        let reverting = reverting.clone();
        move |_| {
            if reverting.get() {
                return;
            }
            let enabled = row.is_active();
            if let Err(message) = autostart::set_start_at_login(enabled) {
                reverting.set(true);
                row.set_active(!enabled);
                reverting.set(false);
                toast.add_toast(Toast::new(&message));
            }
        }
    });
}

fn connect_persist_rows(
    widgets: &SettingsWidgets,
    toast: ToastOverlay,
    reverting: Rc<Cell<bool>>,
) {
    let widgets = Rc::new(widgets_ref(widgets));
    let persist = {
        let widgets = widgets.clone();
        let toast = toast.clone();
        let reverting = reverting.clone();
        Rc::new(move || {
            if reverting.get() {
                return;
            }
            let previous = load_app_settings();
            match collect_and_save(&widgets) {
                Ok(settings) => {
                    let language_changed =
                        previous.appearance.language != settings.appearance.language;
                    settings_apply::apply(&settings);
                    if language_changed {
                        glib::idle_add_local_once(window::reload_ui_language);
                    }
                    dbus_runtime::spawn(
                        async move {
                            let proxy = crate::dbus_client::connect().await?;
                            crate::dbus_client::apply_global_defaults_to_profiles(&proxy).await
                        },
                        |result| {
                            if let Err(err) = result {
                                tracing::warn!(%err, "could not apply global defaults to profiles");
                            }
                        },
                    );
                    update_notify_sensitivity(&widgets);
                }
                Err(message) => {
                    toast.add_toast(Toast::new(&message));
                    reverting.set(true);
                    reload_widgets(&widgets);
                    reverting.set(false);
                }
            }
        })
    };

    widgets.advanced_mode.connect_active_notify({
        let persist = persist.clone();
        move |_| persist()
    });

    let mut switch_rows: Vec<&libadwaita::SwitchRow> = vec![
        &widgets.require_ac,
        &widgets.require_pbs,
        &widgets.notify_enabled,
        &widgets.notify_progress,
        &widgets.notify_success,
        &widgets.notify_failure,
        &widgets.notify_skipped,
        &widgets.notify_restore,
        &widgets.notify_health,
        &widgets.show_tray,
        &widgets.pause_all,
        &widgets.close_to_tray,
    ];
    if backuppilot_core::app_update_checks_enabled() {
        switch_rows.push(&widgets.notify_update);
        switch_rows.push(&widgets.update_check);
        switch_rows.push(&widgets.update_notify);
    }
    for row in switch_rows {
        row.connect_active_notify({
            let persist = persist.clone();
            move |_| {
                persist();
            }
        });
    }

    for spin in [
        &widgets.warn_days,
        &widgets.critical_days,
        &widgets.log_retain_days,
    ] {
        spin.connect_value_notify({
            let persist = persist.clone();
            move |_| {
                persist();
            }
        });
    }

    let mut combo_rows: Vec<&libadwaita::ComboRow> =
        vec![&widgets.language_row, &widgets.color_scheme_row];
    if backuppilot_core::app_update_checks_enabled() {
        combo_rows.push(&widgets.update_channel_row);
    }
    for combo in combo_rows {
        combo.connect_selected_notify({
            let persist = persist.clone();
            move |_| {
                persist();
            }
        });
    }

    widgets.notify_enabled.connect_active_notify({
        let widgets = widgets.clone();
        move |_| {
            update_notify_sensitivity(&widgets);
        }
    });
}

fn widgets_ref(w: &SettingsWidgets) -> SettingsWidgets {
    SettingsWidgets {
        login_row: w.login_row.clone(),
        require_ac: w.require_ac.clone(),
        require_pbs: w.require_pbs.clone(),
        warn_days: w.warn_days.clone(),
        critical_days: w.critical_days.clone(),
        log_retain_days: w.log_retain_days.clone(),
        notify_enabled: w.notify_enabled.clone(),
        notify_progress: w.notify_progress.clone(),
        notify_success: w.notify_success.clone(),
        notify_failure: w.notify_failure.clone(),
        notify_skipped: w.notify_skipped.clone(),
        notify_restore: w.notify_restore.clone(),
        notify_health: w.notify_health.clone(),
        notify_update: w.notify_update.clone(),
        show_tray: w.show_tray.clone(),
        pause_all: w.pause_all.clone(),
        close_to_tray: w.close_to_tray.clone(),
        language_row: w.language_row.clone(),
        color_scheme_row: w.color_scheme_row.clone(),
        update_check: w.update_check.clone(),
        update_channel_row: w.update_channel_row.clone(),
        update_notify: w.update_notify.clone(),
        update_check_now: w.update_check_now.clone(),
        advanced_mode: w.advanced_mode.clone(),
    }
}

fn update_notify_sensitivity(widgets: &SettingsWidgets) {
    let on = widgets.notify_enabled.is_active();
    let advanced = widgets.advanced_mode.is_active();
    for row in [
        &widgets.notify_progress,
        &widgets.notify_success,
        &widgets.notify_failure,
        &widgets.notify_skipped,
        &widgets.notify_restore,
        &widgets.notify_health,
        &widgets.notify_update,
    ] {
        row.set_sensitive(on && advanced);
    }
}

fn collect_and_save(widgets: &SettingsWidgets) -> Result<AppSettings, String> {
    let health_check = HealthCheck {
        warn_after_days: widgets.warn_days.value() as u32,
        critical_after_days: widgets.critical_days.value() as u32,
    };
    validate_health_check(&health_check).map_err(|_| {
        tr("Critical threshold must be greater than or equal to the warning threshold.")})?;

    let log_retention = LogRetentionSettings {
        retain_days: widgets.log_retain_days.value() as u32,
    };
    validate_log_retention(&log_retention).map_err(|_| {
        tr("Log retention must be 0 (keep all) or between 7 and 3650 days.")})?;

    let mut settings = load_app_settings();
    settings.conditions.require_ac_power = widgets.require_ac.is_active();
    settings.conditions.require_server_reachable = widgets.require_pbs.is_active();
    settings.health_check = health_check;
    settings.log_retention = log_retention;
    settings.notifications = NotificationSettings {
        enabled: widgets.notify_enabled.is_active(),
        notify_on_backup_progress: widgets.notify_progress.is_active(),
        notify_on_success: widgets.notify_success.is_active(),
        notify_on_failure: widgets.notify_failure.is_active(),
        notify_on_skipped: widgets.notify_skipped.is_active(),
        notify_on_restore: widgets.notify_restore.is_active(),
        notify_on_health_warning: widgets.notify_health.is_active(),
        notify_on_update: widgets.notify_update.is_active(),
    };
    settings.tray = TraySettings {
        show_tray_icon: widgets.show_tray.is_active(),
        pause_all_backups: widgets.pause_all.is_active(),
        close_to_tray: widgets.close_to_tray.is_active(),
        ask_unmount_on_quit: settings.tray.ask_unmount_on_quit,
    };
    settings.appearance.language =
        settings_apply::language_from_dropdown(widgets.language_row.selected());
    settings.appearance.color_scheme =
        settings_apply::color_scheme_from_dropdown(widgets.color_scheme_row.selected());
    settings.appearance.advanced_mode = widgets.advanced_mode.is_active();
    settings.updates = UpdateSettings {
        check_automatically: widgets.update_check.is_active(),
        channel: settings_apply::update_channel_from_dropdown(widgets.update_channel_row.selected()),
        notify_when_available: widgets.update_notify.is_active(),
    };

    settings_apply::save(&settings)?;
    Ok(settings)
}

fn reload_widgets(widgets: &SettingsWidgets) {
    let settings = load_app_settings();
    widgets.require_ac.set_active(settings.conditions.require_ac_power);
    widgets
        .require_pbs
        .set_active(settings.conditions.require_server_reachable);
    widgets
        .warn_days
        .set_value(settings.health_check.warn_after_days as f64);
    widgets
        .critical_days
        .set_value(settings.health_check.critical_after_days as f64);
    widgets
        .log_retain_days
        .set_value(settings.log_retention.retain_days as f64);
    widgets
        .notify_enabled
        .set_active(settings.notifications.enabled);
    widgets
        .notify_progress
        .set_active(settings.notifications.notify_on_backup_progress);
    widgets
        .notify_success
        .set_active(settings.notifications.notify_on_success);
    widgets
        .notify_failure
        .set_active(settings.notifications.notify_on_failure);
    widgets
        .notify_skipped
        .set_active(settings.notifications.notify_on_skipped);
    widgets
        .notify_restore
        .set_active(settings.notifications.notify_on_restore);
    widgets
        .notify_health
        .set_active(settings.notifications.notify_on_health_warning);
    widgets
        .notify_update
        .set_active(settings.notifications.notify_on_update);
    widgets.show_tray.set_active(settings.tray.show_tray_icon);
    widgets.pause_all.set_active(settings.tray.pause_all_backups);
    widgets
        .close_to_tray
        .set_active(settings.tray.close_to_tray);
    widgets.language_row.set_selected(settings_apply::language_dropdown_index(
        settings.appearance.language,
    ));
    widgets.color_scheme_row.set_selected(settings_apply::color_scheme_dropdown_index(
        settings.appearance.color_scheme,
    ));
    widgets
        .advanced_mode
        .set_active(settings.appearance.advanced_mode);
    apply_settings_advanced_row_visibility(widgets, settings.appearance.advanced_mode);
    widgets
        .update_check
        .set_active(settings.updates.check_automatically);
    widgets.update_channel_row.set_selected(
        settings_apply::update_channel_dropdown_index(settings.updates.channel),
    );
    widgets
        .update_notify
        .set_active(settings.updates.notify_when_available);
    update_notify_sensitivity(widgets);
}
