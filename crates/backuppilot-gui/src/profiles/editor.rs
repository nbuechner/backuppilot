use std::cell::RefCell;
use std::rc::Rc;

use backuppilot_core::app_settings::load_app_settings;
use backuppilot_core::list_saved_connections;
use backuppilot_core::pbs_repository::PbsRepositoryParts;
use backuppilot_core::profile::{
    default_backup_id, BackupConditions, BackupProfile, CredentialVerifyInput, ExecutionContext,
    HealthCheck, NewProfile, Schedule, ScheduleType,
};
use backuppilot_core::cli_available_sync;
use backuppilot_i18n::{tr, tr_fmt};
use gtk::glib;
use gtk::prelude::*;
use libadwaita::prelude::{
    ActionRowExt, AdwDialogExt, AdwWindowExt, AlertDialogExt, ComboRowExt, PreferencesGroupExt,
};

use crate::advanced_ui;
use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::form_fields::{PasswordEntryField, TextEntryRow};
use super::{build_info_callout, preflight_panel, string_list_editor};

thread_local! {
    static EDITOR_WINDOW: RefCell<Option<libadwaita::Window>> = const { RefCell::new(None) };
}

fn dismiss_editor_window(window: &libadwaita::Window) {
    EDITOR_WINDOW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|w| w.as_ptr() == window.as_ptr()) {
            *slot = None;
        }
    });
    window.destroy();
}

pub fn dismiss_any_editor_window() {
    EDITOR_WINDOW.with(|slot| {
        if let Some(window) = slot.borrow_mut().take() {
            window.destroy();
        }
    });
}

struct EditorWidgets {
    name: TextEntryRow,
    enabled: libadwaita::SwitchRow,
    pbs_host: TextEntryRow,
    pbs_user: TextEntryRow,
    pbs_token: PasswordEntryField,
    pbs_datastore: TextEntryRow,
    namespace: TextEntryRow,
    server_fingerprint: TextEntryRow,
    backup_id: TextEntryRow,
    paths_items: Rc<RefCell<Vec<String>>>,
    excludes_items: Rc<RefCell<Vec<String>>>,
    schedule_row: libadwaita::ComboRow,
    schedule_time: TextEntryRow,
    schedule_weekday: libadwaita::ComboRow,
    schedule_cron: TextEntryRow,
    require_ac: libadwaita::SwitchRow,
    require_pbs: libadwaita::SwitchRow,
    require_network: libadwaita::ComboRow,
    /// Combo index 1..=n maps to this (index 0 = any network).
    network_names: Vec<String>,
    require_vpn: libadwaita::SwitchRow,
    warn_days: libadwaita::SpinRow,
    critical_days: libadwaita::SpinRow,
    bandwidth_limit: libadwaita::ComboRow,
    encryption_key_row: libadwaita::ComboRow,
    encryption_key_ids: std::rc::Rc<std::cell::RefCell<Vec<Option<i64>>>>,
    encryption_warning: gtk::Box,
    execution_row: libadwaita::ComboRow,
    host_cli_available: std::rc::Rc<std::cell::Cell<bool>>,
    /// Token from DB when editing (password field may not expose unchanged secrets).
    saved_api_token: Option<String>,
}

pub fn open(
    parent: &libadwaita::ApplicationWindow,
    existing: Option<BackupProfile>,
    on_saved: impl Fn() + 'static,
) {
    if let Some(profile) = existing {
        let parent = parent.clone();
        let on_saved = std::rc::Rc::new(on_saved);
        dbus_runtime::spawn(
            async move {
                let proxy = connect().await?;
                dbus_client::get_profile(&proxy, profile.id).await
            },
            move |result| match result {
                Ok(fresh) => present(parent.upcast_ref(), Some(fresh), on_saved),
                Err(err) => tracing::warn!(%err, "failed to load profile for editing"),
            },
        );
        return;
    }

    present(parent, None, std::rc::Rc::new(on_saved));
}

fn present(
    parent: &libadwaita::ApplicationWindow,
    existing: Option<BackupProfile>,
    on_saved: std::rc::Rc<dyn Fn()>,
) {
    dismiss_any_editor_window();

    let is_edit = existing.is_some();
    let profile_id = existing.as_ref().map(|p| p.id);
    let title = if is_edit {
        tr("Edit profile")} else {
        tr("New profile")};

    let window = libadwaita::Window::builder()
        .transient_for(parent)
        .modal(true)
        .default_width(600)
        .default_height(640)
        .build();

    let widgets = build_form(existing.as_ref());
    refresh_execution_row(
        &widgets.execution_row,
        widgets.host_cli_available.get(),
        execution_context_from_profile(existing.as_ref()),
    );

    dbus_runtime::spawn(
        async move {
            let proxy = dbus_client::connect().await?;
            dbus_client::host_cli_available(&proxy).await
        },
        {
            let execution_row = widgets.execution_row.clone();
            let host_cli_available = widgets.host_cli_available.clone();
            let selected = execution_context_from_profile(existing.as_ref());
            move |result| {
                let available = result.unwrap_or(host_cli_available.get());
                host_cli_available.set(available);
                refresh_execution_row(&execution_row, available, selected);
            }
        },
    );

    let advanced_switch = gtk::Switch::builder()
        .active(advanced_ui::advanced_mode_enabled())
        .valign(gtk::Align::Center)
        .build();
    let (form, test_connection_row, test_connection_btn, preflight_btn, preflight_list) =
        build_scrolled_form(
            &widgets,
            window.upcast_ref(),
            profile_id,
            existing.as_ref(),
            &advanced_switch,
        );

    let header = libadwaita::HeaderBar::new();
    let title_label = gtk::Label::builder()
        .label(&title)
        .css_classes(["title"])
        .build();
    header.set_title_widget(Some(&title_label));

    let cancel_btn = gtk::Button::with_label(&tr("Cancel"));
    let save_btn = gtk::Button::with_label(&tr("Save"));
    save_btn.add_css_class("suggested-action");
    header.pack_start(&cancel_btn);
    header.pack_end(&save_btn);
    header.pack_end(&advanced_ui::build_compact_advanced_toggle(&advanced_switch));

    if is_edit {
        let delete_btn = gtk::Button::with_label(&tr("Delete"));
        delete_btn.add_css_class("destructive-action");
        header.pack_start(&delete_btn);

        let window_del = window.clone();
        let on_saved_del = on_saved.clone();
        delete_btn.connect_clicked(move |_| {
            let Some(id) = profile_id else { return };
            confirm_delete(&window_del, id, {
                let window = window_del.clone();
                let on_saved = on_saved_del.clone();
                move || {
                    on_saved();
                    dismiss_editor_window(&window);
                }
            });
        });
    }

    let window_ref = window.clone();
    window.connect_close_request(move |window| {
        dismiss_editor_window(window);
        glib::Propagation::Stop
    });
    window_ref.connect_destroy(|_| {
        EDITOR_WINDOW.with(|slot| {
            *slot.borrow_mut() = None;
        });
    });

    EDITOR_WINDOW.with(|slot| {
        *slot.borrow_mut() = Some(window.clone());
    });

    let toolbar = libadwaita::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&form));
    window.set_content(Some(&toolbar));

    let widgets_rc = std::rc::Rc::new(widgets);
    let selected_key = existing.as_ref().and_then(|p| p.encryption_key_id);
    refill_encryption_key_combo(
        &widgets_rc.encryption_key_row,
        &widgets_rc.encryption_key_ids,
        &widgets_rc.encryption_warning,
        selected_key,
    );

    wire_connection_test(&window, &widgets_rc, &test_connection_row, &test_connection_btn);

    if let Some(id) = profile_id {
        let preflight_list = preflight_list.clone();
        let preflight_btn = preflight_btn.clone();
        let preflight_btn_click = preflight_btn.clone();
        preflight_btn.connect_clicked(move |_| {
            preflight_btn_click.set_sensitive(false);
            let list = preflight_list.clone();
            let btn = preflight_btn_click.clone();
            dbus_runtime::spawn(
                async move {
                    let proxy = connect().await?;
                    dbus_client::run_preflight(&proxy, id).await
                },
                move |result| {
                    btn.set_sensitive(true);
                    match result {
                        Ok(report) => preflight_panel::fill_list(&list, &report.checks),
                        Err(err) => tracing::warn!(%err, "preflight failed"),
                    }
                },
            );
        });
    }

    cancel_btn.connect_clicked({
        let window = window.clone();
        move |_| dismiss_editor_window(&window)
    });

    save_btn.connect_clicked({
        let window = window.clone();
        let widgets = widgets_rc.clone();
        let on_saved = on_saved.clone();
        move |_| {
            let data = match collect_form(&widgets) {
                Ok(d) => d,
                Err(msg) => {
                    show_error_dialog(&window, &msg, &tr("Could not save"));
                    return;
                }
            };

            let on_saved = on_saved.clone();
            let window = window.clone();
            let window_for_error = window.clone();
            dbus_runtime::spawn(
                async move { save_profile(profile_id, data).await },
                move |result| {
                    match result {
                        Ok(()) => {
                            on_saved();
                            dismiss_editor_window(&window);
                        }
                        Err(err) => {
                            show_error_dialog(
                                &window_for_error,
                                &tr_fmt(
                                    "Failed to save profile: {err}",
                                    &[("err", &err.to_string())],
                                ),
                                &tr("Could not save"),
                            );
                        }
                    }
                },
            );
        }
    });

    window.present();
}

fn build_form(existing: Option<&BackupProfile>) -> EditorWidgets {
    let defaults = existing.map(|p| p.to_new_profile());

    let name = TextEntryRow::new(
        &tr("Name"),
        defaults.as_ref().map(|d| d.name.as_str()).unwrap_or(""),
    );

    let enabled = libadwaita::SwitchRow::builder()
        .title(&tr("Active"))
        .subtitle(&tr("Back up automatically on a schedule"))
        .active(defaults.as_ref().is_none_or(|d| d.enabled))
        .build();

    let pbs_defaults = defaults.as_ref().and_then(|d| PbsRepositoryParts::parse(&d.repository).ok());
    let saved_api_token = existing.and_then(|p| {
        if p.api_token_configured {
            backuppilot_core::load_stored_api_token(p.id).or_else(|| {
                pbs_defaults
                    .as_ref()
                    .filter(|parts| !parts.token.is_empty())
                    .map(|parts| parts.token.clone())
            })
        } else {
            pbs_defaults.as_ref().map(|parts| parts.token.clone())
        }
    });

    let pbs_host = TextEntryRow::new(
        &tr("Server address"),
        pbs_defaults.as_ref().map(|p| p.host.as_str()).unwrap_or(""),
    );

    let pbs_user = TextEntryRow::new(
        &tr("Username"),
        pbs_defaults.as_ref().map(|p| p.user.as_str()).unwrap_or(""),
    );

    let default_token_secret = pbs_defaults
        .as_ref()
        .map(|p| p.token.as_str())
        .unwrap_or("")
        .to_string();

    let pbs_token = PasswordEntryField::new(&tr("API token"), &default_token_secret);

    let pbs_datastore = TextEntryRow::new(
        &tr("Datastore"),
        pbs_defaults
            .as_ref()
            .map(|p| p.datastore.as_str())
            .unwrap_or(""),
    );

    let namespace = TextEntryRow::new(
        &tr("Namespace (optional)"),
        defaults
            .as_ref()
            .and_then(|d| d.namespace.as_deref())
            .unwrap_or(""),
    );

    let server_fingerprint = TextEntryRow::new(
        &tr("Server TLS fingerprint (optional)"),
        defaults
            .as_ref()
            .and_then(|d| d.server_fingerprint.as_deref())
            .unwrap_or(""),
    );

    let backup_id_default = defaults
        .as_ref()
        .map(|d| d.backup_id.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(default_backup_id);
    let backup_id = TextEntryRow::new(&tr("Name on the backup server"), &backup_id_default);

    let paths_items = Rc::new(RefCell::new(
        defaults
            .as_ref()
            .map(|d| d.paths.clone())
            .unwrap_or_default(),
    ));
    let excludes_items = Rc::new(RefCell::new(
        defaults
            .as_ref()
            .map(|d| d.excludes.clone())
            .unwrap_or_default(),
    ));

    let default_schedule = defaults
        .as_ref()
        .map(|d| d.schedule.schedule_type)
        .unwrap_or(ScheduleType::Daily);
    let schedule_row = schedule_combo_row(default_schedule);

    let schedule_time = TextEntryRow::new(
        &tr("Time of day"),
        defaults
            .as_ref()
            .and_then(|d| d.schedule.time.as_deref())
            .unwrap_or("12:00"),
    );

    let default_weekday = defaults
        .as_ref()
        .and_then(|d| d.schedule.weekday)
        .unwrap_or(1);
    let schedule_weekday = weekday_combo_row(default_weekday);

    let schedule_cron = TextEntryRow::new(
        &tr("Cron schedule"),
        defaults
            .as_ref()
            .and_then(|d| d.schedule.cron_expr.as_deref())
            .unwrap_or("0 12 * * *"),
    );

    let app_defaults = load_app_settings();
    let conditions = defaults
        .as_ref()
        .map(|d| d.conditions.clone())
        .unwrap_or_else(|| app_defaults.conditions.clone());
    let health = defaults
        .as_ref()
        .map(|d| d.health_check.clone())
        .unwrap_or_else(|| app_defaults.health_check.clone());

    let require_ac = libadwaita::SwitchRow::builder()
        .title(&tr("Only on AC power"))
        .subtitle(&tr("Skip backup on battery power."))
        .active(conditions.require_ac_power)
        .build();

    let (require_network, network_names) =
        network_combo_row(conditions.require_network.first().map(String::as_str));

    let require_vpn = libadwaita::SwitchRow::builder()
        .title(&tr("Only when VPN is active"))
        .subtitle(&tr("Requires an active VPN connection in NetworkManager."))
        .active(conditions.require_vpn)
        .build();

    let require_pbs = libadwaita::SwitchRow::builder()
        .title(&tr("PBS must be reachable"))
        .subtitle(&tr("DNS, port, and login are checked before each backup."))
        .active(conditions.require_server_reachable)
        .build();

    let warn_adj = gtk::Adjustment::new(
        health.warn_after_days as f64,
        1.0,
        365.0,
        1.0,
        5.0,
        0.0,
    );
    let warn_days = libadwaita::SpinRow::builder()
        .title(&tr("Warning after (days)"))
        .adjustment(&warn_adj)
        .build();

    let crit_adj = gtk::Adjustment::new(
        health.critical_after_days as f64,
        1.0,
        365.0,
        1.0,
        5.0,
        0.0,
    );
    let critical_days = libadwaita::SpinRow::builder()
        .title(&tr("Critical after (days)"))
        .adjustment(&crit_adj)
        .build();

    let bandwidth_limit = bandwidth_combo_row(conditions.bandwidth_limit_kib_s);

    let encryption_key_row = encryption_key_combo_placeholder();
    let encryption_key_ids = std::rc::Rc::new(std::cell::RefCell::new(vec![None]));
    let encryption_warning = build_info_callout(
        &tr("Encryption key required for restore"),
        &tr("Without this key and its password, encrypted backups cannot be restored , not even by server administrators. Store a backup copy under Encryption in the sidebar."),
    );
    encryption_warning.set_visible(
        defaults
            .as_ref()
            .and_then(|d| d.encryption_key_id)
            .is_some(),
    );

    let host_cli_available = std::rc::Rc::new(std::cell::Cell::new(cli_available_sync()));
    let execution_row = execution_combo_row(conditions.execution_context, host_cli_available.get());

    EditorWidgets {
        name,
        enabled,
        pbs_host,
        pbs_user,
        pbs_token,
        pbs_datastore,
        namespace,
        server_fingerprint,
        backup_id,
        paths_items,
        excludes_items,
        schedule_row,
        schedule_time,
        schedule_weekday,
        schedule_cron,
        require_ac,
        require_pbs,
        require_network,
        network_names,
        require_vpn,
        warn_days,
        critical_days,
        bandwidth_limit,
        encryption_key_row,
        encryption_key_ids,
        encryption_warning,
        execution_row,
        host_cli_available,
        saved_api_token,
    }
}

fn execution_context_from_profile(profile: Option<&BackupProfile>) -> ExecutionContext {
    profile
        .map(|p| p.conditions.execution_context)
        .unwrap_or_default()
}

fn refresh_execution_row(
    row: &libadwaita::ComboRow,
    host_cli_available: bool,
    selected: ExecutionContext,
) {
    let desktop = tr("Desktop (background service)");
    let host = tr("Host CLI (backuppilot-cli)");
    let (labels, index) = if host_cli_available {
        (
            vec![desktop.as_str(), host.as_str()],
            match selected {
                ExecutionContext::Desktop => 0,
                ExecutionContext::HostCli => 1,
            },
        )
    } else {
        (
            vec![desktop.as_str()],
            0,
        )
    };
    let model = gtk::StringList::new(&labels);
    row.set_model(Some(&model));
    row.set_selected(index);
    if !host_cli_available {
        row.set_subtitle(&tr(
            "Install backuppilot-cli on the host to enable privileged or unattended backups.",
        ));
    } else {
        row.set_subtitle(&tr(
            "Use Host CLI for paths outside your user account or systemd/cron schedules.",
        ));
    }
}

fn execution_combo_row(
    selected: ExecutionContext,
    host_cli_available: bool,
) -> libadwaita::ComboRow {
    let row = libadwaita::ComboRow::builder()
        .title(&tr("Execution"))
        .build();
    refresh_execution_row(&row, host_cli_available, selected);
    row
}

fn execution_context_at_index(index: u32, host_cli_available: bool) -> ExecutionContext {
    if host_cli_available && index == 1 {
        ExecutionContext::HostCli
    } else {
        ExecutionContext::Desktop
    }
}

fn build_scrolled_form(
    widgets: &EditorWidgets,
    parent_window: &gtk::Window,
    profile_id: Option<i64>,
    _existing: Option<&BackupProfile>,
    advanced_switch: &gtk::Switch,
) -> (
    gtk::Widget,
    libadwaita::ActionRow,
    gtk::Button,
    gtk::Button,
    gtk::ListBox,
) {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .vexpand(true)
        .build();

    let general = libadwaita::PreferencesGroup::builder()
        .title(&tr("General"))
        .build();
    general.add(&widgets.name.row);
    general.add(&widgets.enabled);

    let execution_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("How backups run"))
        .description(&tr(
            "Desktop uses the BackupPilot background service. Host CLI runs via backuppilot-cli on the system (for root paths or scheduled jobs without a logged-in session).",
        ))
        .build();
    execution_group.add(&widgets.execution_row);

    let pbs = libadwaita::PreferencesGroup::builder()
        .title(&tr("Backup server"))
        .description(&tr(
            "Connection to PBS. For self-signed TLS, enter the fingerprint from proxmox-backup-manager cert info on the server.",
        ))
        .build();
    pbs.add(&widgets.pbs_host.row);
    pbs.add(&widgets.pbs_user.row);
    pbs.add(&widgets.pbs_token.row);
    pbs.add(&widgets.pbs_datastore.row);
    pbs.add(&widgets.namespace.row);
    pbs.add(&widgets.server_fingerprint.row);

    let (test_connection_row, test_connection_btn) = build_test_connection_row();
    pbs.add(&test_connection_row);

    let identity = libadwaita::PreferencesGroup::builder()
        .title(&tr("This computer"))
        .description(&tr("How this PC appears in the backup. Leave empty to use the computer name."))
        .build();
    identity.add(&widgets.backup_id.row);

    let source = libadwaita::PreferencesGroup::builder()
        .title(&tr("Folders to back up"))
        .description(&tr("Directories on this computer that are sent to PBS."))
        .build();
    source.add(&string_list_editor::build_backup_paths_block(
        parent_window,
        widgets.paths_items.clone(),
    ));

    let excludes_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Skip these files"))
        .description(&tr("Files matching these patterns are not included in the backup."))
        .build();
    excludes_group.add(&string_list_editor::build_excludes_block(
        parent_window,
        widgets.excludes_items.clone(),
    ));

    let schedule_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("When to back up"))
        .build();
    schedule_group.add(&widgets.schedule_row);
    schedule_group.add(&widgets.schedule_time.row);
    schedule_group.add(&widgets.schedule_weekday);
    schedule_group.add(&widgets.schedule_cron.row);

    let conditions_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Backup conditions"))
        .description(&tr("When this profile may run automatically."))
        .build();
    conditions_group.add(&widgets.require_ac);
    conditions_group.add(&widgets.require_pbs);
    conditions_group.add(&widgets.require_network);
    conditions_group.add(&widgets.require_vpn);
    conditions_group.add(&widgets.bandwidth_limit);

    let encryption_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Backup encryption"))
        .description(&tr("Encrypt new backups with a PBS key. Existing snapshots stay as they are."))
        .build();
    encryption_group.add(&widgets.encryption_key_row);
    encryption_group.add(&widgets.encryption_warning);

    let health_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Backup health"))
        .description(&tr("When to show warning or critical status on the dashboard."))
        .build();
    health_group.add(&widgets.warn_days);
    health_group.add(&widgets.critical_days);

    widgets.encryption_key_row.connect_notify_local(Some("selected"), {
        let banner = widgets.encryption_warning.clone();
        let ids = widgets.encryption_key_ids.clone();
        move |row, _| {
            let idx = row.selected() as usize;
            let active = ids.borrow().get(idx).copied().flatten().is_some();
            banner.set_visible(active);
        }
    });

    bind_schedule_visibility(
        &widgets.schedule_row,
        &widgets.schedule_time.row,
        &widgets.schedule_weekday,
        &widgets.schedule_cron.row,
    );

    let (preflight_group, preflight_list, preflight_btn) = preflight_panel::build_group();

    let mut advanced_sections: Vec<gtk::Widget> = vec![
        excludes_group.clone().upcast(),
        conditions_group.clone().upcast(),
        encryption_group.clone().upcast(),
        health_group.clone().upcast(),
    ];
    if profile_id.is_some() {
        advanced_sections.push(preflight_group.clone().upcast());
    }

    page.append(&general);
    page.append(&execution_group);
    page.append(&pbs);
    page.append(&identity);
    page.append(&source);
    page.append(&schedule_group);
    page.append(&excludes_group);
    page.append(&conditions_group);
    page.append(&encryption_group);
    page.append(&health_group);
    if profile_id.is_some() {
        page.append(&preflight_group);
    }

    advanced_ui::bind_advanced_mode_switch(&advanced_switch, &advanced_sections);
    let namespace_row = widgets.namespace.row.clone();
    let schedule_row_vis = widgets.schedule_row.clone();
    let schedule_time_vis = widgets.schedule_time.row.clone();
    let schedule_weekday_vis = widgets.schedule_weekday.clone();
    let schedule_cron_vis = widgets.schedule_cron.row.clone();
    let sync_profile_advanced_rows = Rc::new(move |on: bool| {
        namespace_row.set_visible(on);
        apply_schedule_visibility(
            &schedule_row_vis,
            &schedule_time_vis,
            &schedule_weekday_vis,
            &schedule_cron_vis,
        );
    });
    sync_profile_advanced_rows(advanced_ui::advanced_mode_enabled());
    advanced_switch.connect_active_notify({
        let sync = sync_profile_advanced_rows.clone();
        move |sw| {
            sync(sw.is_active());
        }
    });

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&page)
        .build();
    (
        scrolled.upcast(),
        test_connection_row,
        test_connection_btn,
        preflight_btn,
        preflight_list,
    )
}

fn build_test_connection_row() -> (libadwaita::ActionRow, gtk::Button) {
    crate::util::preferences_action_row_with_button(
        &tr("Test connection"),
        &tr("Check whether login details work."),
        &tr("Test…"),
    )
}

fn wire_connection_test(
    window: &libadwaita::Window,
    widgets: &std::rc::Rc<EditorWidgets>,
    test_row: &libadwaita::ActionRow,
    test_btn: &gtk::Button,
) {
    let window = window.clone();
    let widgets = widgets.clone();
    let row_for_test = test_row.clone();
    let run = std::rc::Rc::new(move || run_connection_test(&window, &widgets, &row_for_test));
    test_btn.connect_clicked(move |_| run());
}

fn collect_pbs_credentials(w: &EditorWidgets) -> Result<CredentialVerifyInput, String> {
    let pbs_host = w.pbs_host.text().trim().to_string();
    if pbs_host.is_empty() {
        return Err(tr("PBS hostname is required."));
    }

    let pbs_user = w.pbs_user.text().trim().to_string();
    if pbs_user.is_empty() {
        return Err(tr("PBS username is required."));
    }

    let pbs_datastore = w.pbs_datastore.text().trim().to_string();
    if pbs_datastore.is_empty() {
        return Err(tr("Datastore is required."));
    }
    if pbs_datastore.len() < 3 {
        return Err(tr("Datastore name must be at least 3 characters (required by PBS)."));
    }

    let pbs_token = resolve_api_token(w)?;

    let repository = PbsRepositoryParts {
        user: pbs_user,
        host: pbs_host,
        token: pbs_token,
        datastore: pbs_datastore,
    }
    .to_storage();

    let namespace = w.namespace.text().trim().to_string();
    let namespace = if namespace.is_empty() {
        None
    } else {
        Some(namespace)
    };

    let server_fp = w.server_fingerprint.text().trim().to_string();
    let server_fingerprint = if server_fp.is_empty() {
        None
    } else {
        Some(server_fp)
    };

    Ok(CredentialVerifyInput {
        repository,
        namespace,
        server_fingerprint,
    })
}

fn resolve_api_token(w: &EditorWidgets) -> Result<String, String> {
    // PasswordEntryRow often returns " until edited; fall back to the value loaded from the profile.
    let from_field = w.pbs_token.text();
    let token = if from_field.is_empty() {
        w.saved_api_token
            .clone()
            .filter(|t| !t.is_empty())
            .unwrap_or_default()
    } else {
        from_field
    };
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(tr("API token is required."));
    }
    Ok(token)
}

fn collect_form(w: &EditorWidgets) -> Result<NewProfile, String> {
    let name = w.name.text().trim().to_string();
    if name.is_empty() {
        return Err(tr("Profile name is required."));
    }

    let pbs = collect_pbs_credentials(w)?;
    let repository = pbs.repository;
    let namespace = pbs.namespace;
    let server_fingerprint = pbs.server_fingerprint;

    let backup_id = w.backup_id.text().trim().to_string();
    let backup_id = if backup_id.is_empty() {
        default_backup_id()
    } else {
        backup_id
    };

    let paths = w.paths_items.borrow().clone();
    if paths.is_empty() {
        return Err(tr("At least one backup path is required."));
    }

    let excludes = w.excludes_items.borrow().clone();

    let schedule_type = schedule_type_at_index(w.schedule_row.selected());
    let time_str = w.schedule_time.text().trim().to_string();
    let schedule = Schedule {
        schedule_type,
        time: match schedule_type {
            ScheduleType::Daily | ScheduleType::Weekly => {
                if time_str.is_empty() {
                    Some("12:00".into())
                } else {
                    Some(time_str)
                }
            }
            _ => None,
        },
        weekday: match schedule_type {
            ScheduleType::Weekly => Some(weekday_at_index(w.schedule_weekday.selected())),
            _ => None,
        },
        cron_expr: match schedule_type {
            ScheduleType::Custom => {
                let expr = w.schedule_cron.text().trim().to_string();
                if expr.is_empty() {
                    return Err(tr("Cron schedule is required for advanced mode."));
                }
                Some(expr)
            }
            _ => None,
        },
    };

    let warn_after_days = w.warn_days.value() as u32;
    let critical_after_days = w.critical_days.value() as u32;
    if critical_after_days < warn_after_days {
        return Err(tr("Critical threshold must be greater than or equal to the warning threshold."));
    }

    let bandwidth_limit_kib_s = bandwidth_at_combo_index(w.bandwidth_limit.selected());

    let require_network: Vec<String> = selected_network_name(w.require_network.selected(), &w.network_names)
        .into_iter()
        .collect();

    let execution_context =
        execution_context_at_index(w.execution_row.selected(), w.host_cli_available.get());
    if execution_context == ExecutionContext::HostCli && !w.host_cli_available.get() {
        return Err(tr(
            "Host CLI is not installed. Install backuppilot-cli on the system or choose Desktop execution.",
        ));
    }

    Ok(NewProfile {
        name,
        enabled: w.enabled.is_active(),
        repository,
        namespace,
        backup_id,
        paths,
        excludes,
        schedule,
        conditions: BackupConditions {
            execution_context,
            require_ac_power: w.require_ac.is_active(),
            require_network,
            require_vpn: w.require_vpn.is_active(),
            require_server_reachable: w.require_pbs.is_active(),
            bandwidth_limit_kib_s,
        },
        health_check: HealthCheck {
            warn_after_days,
            critical_after_days,
        },
        encryption_key_id: w.encryption_key_ids
            .borrow()
            .get(w.encryption_key_row.selected() as usize)
            .copied()
            .flatten(),
        server_fingerprint,
    })
}

fn encryption_key_combo_placeholder() -> libadwaita::ComboRow {
    let none = tr("(No encryption)");
    let model = gtk::StringList::new(&[none.as_str()]);
    libadwaita::ComboRow::builder()
        .title(&tr("Encryption key"))
        .subtitle(&tr("Manage keys under Encryption in the sidebar"))
        .model(&model)
        .selected(0)
        .build()
}

fn refill_encryption_key_combo(
    row: &libadwaita::ComboRow,
    ids_slot: &std::rc::Rc<std::cell::RefCell<Vec<Option<i64>>>>,
    banner: &gtk::Box,
    selected: Option<i64>,
) {
    let row = row.clone();
    let ids_slot = ids_slot.clone();
    let banner = banner.clone();
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::list_encryption_keys(&proxy).await
        },
        move |result| {
            let keys = result.unwrap_or_default();
            let none = tr("(No encryption)");
            let mut ids = vec![None];
            let mut labels = vec![none];
            for key in keys {
                ids.push(Some(key.id));
                labels.push(key.name);
            }
            let mut index = 0u32;
            if let Some(sel) = selected {
                if let Some(pos) = ids.iter().position(|id| *id == Some(sel)) {
                    index = pos as u32;
                }
            }
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let model = gtk::StringList::new(&refs);
            row.set_model(Some(&model));
            row.set_selected(index);
            *ids_slot.borrow_mut() = ids;
            banner.set_visible(
                ids_slot
                    .borrow()
                    .get(row.selected() as usize)
                    .copied()
                    .flatten()
                    .is_some(),
            );
        },
    );
}

async fn save_profile(profile_id: Option<i64>, data: NewProfile) -> Result<(), zbus::Error> {
    let proxy = connect().await?;
    match profile_id {
        Some(id) => {
            dbus_client::update_profile(&proxy, id, &data).await?;
        }
        None => {
            dbus_client::create_profile(&proxy, &data).await?;
        }
    }
    Ok(())
}

fn run_connection_test(
    parent: &libadwaita::Window,
    widgets: &EditorWidgets,
    test_row: &libadwaita::ActionRow,
) {
    if !test_row.is_sensitive() {
        return;
    }
    let input = match collect_pbs_credentials(widgets) {
        Ok(input) => input,
        Err(msg) => {
            show_error_dialog(parent, &msg, &tr("Could not test connection"));
            return;
        }
    };

    let parent = parent.clone();
    let idle_subtitle = test_row
        .subtitle()
        .map(|s| s.to_string())
        .unwrap_or_else(|| tr("Check whether login details work."));
    test_row.set_sensitive(false);
    test_row.set_subtitle(&tr("Testing…"));
    let test_row = test_row.clone();

    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::verify_profile_credentials(&proxy, &input).await
        },
        move |result| {
            test_row.set_sensitive(true);
            test_row.set_subtitle(&idle_subtitle);

            match result {
                Ok(verify) if verify.ok => {
                    show_info_dialog(
                        &parent,
                        &tr("PBS accepted the credentials."),
                        &tr("Connection successful"),
                    );
                }
                Ok(verify) => {
                    let message =
                        verify.message.unwrap_or_else(|| tr("PBS rejected the credentials."));
                    show_error_dialog(&parent, &message, &tr("Could not test connection"));
                }
                Err(err) => {
                    show_error_dialog(
                        &parent,
                        &tr_fmt("Daemon error: {err}", &[("err", &err.to_string())]),
                        &tr("Could not test connection"),
                    );
                }
            }
        },
    );
}

fn confirm_delete(
    parent: &libadwaita::Window,
    profile_id: i64,
    on_deleted: impl Fn() + 'static,
) {
    let on_deleted = std::rc::Rc::new(on_deleted);
    let heading = tr("Delete profile?");
    let body = tr("This cannot be undone.");
    let alert = libadwaita::AlertDialog::builder()
        .heading(&heading)
        .body(&body)
        .build();
    alert.add_response("cancel", &tr("Cancel"));
    alert.add_response("delete", &tr("Delete"));
    alert.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");

    alert.connect_response(None::<&str>, move |_, response| {
        if response == "delete" {
            let on_deleted = on_deleted.clone();
            dbus_runtime::spawn(
                async move {
                    let proxy = connect().await?;
                    dbus_client::delete_profile(&proxy, profile_id).await?;
                    Ok::<(), zbus::Error>(())
                },
                move |result| {
                    let _ = result;
                    on_deleted();
                },
            );
        }
    });

    alert.present(Some(parent));
}

fn show_error_dialog(parent: &libadwaita::Window, message: &str, heading: &str) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(message)
        .build();
    alert.add_response("ok", &tr("OK"));
    alert.present(Some(parent));
}

fn show_info_dialog(parent: &libadwaita::Window, message: &str, heading: &str) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(message)
        .build();
    alert.add_response("ok", &tr("OK"));
    alert.present(Some(parent));
}

fn schedule_type_at_index(index: u32) -> ScheduleType {
    match index {
        1 => ScheduleType::Hourly,
        2 => ScheduleType::Weekly,
        3 => ScheduleType::OnLogin,
        4 => ScheduleType::Custom,
        _ => ScheduleType::Daily,
    }
}

fn network_combo_row(selected: Option<&str>) -> (libadwaita::ComboRow, Vec<String>) {
    let mut names = list_saved_connections();
    if let Some(sel) = selected.filter(|s| !s.is_empty()) {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(sel)) {
            names.push(sel.to_string());
            names.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
        }
    }

    let mut labels = vec![tr("Any network")];
    labels.extend(names.iter().cloned());

    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&refs);

    let selected_idx = selected
        .and_then(|sel| names.iter().position(|n| n.eq_ignore_ascii_case(sel)))
        .map(|i| (i + 1) as u32)
        .unwrap_or(0);

    let row = libadwaita::ComboRow::builder()
        .title(&tr("Network"))
        .subtitle(&tr("Only run backups on this Wi-Fi or wired connection."))
        .model(&model)
        .build();
    row.set_selected(selected_idx);
    (row, names)
}

fn selected_network_name(index: u32, names: &[String]) -> Option<String> {
    if index == 0 {
        None
    } else {
        names.get((index as usize).saturating_sub(1)).cloned()
    }
}

fn bandwidth_presets() -> Vec<(Option<u32>, String)> {
    vec![
        (None, tr("No limit")),
        (Some(512), tr("Very slow (512 KiB/s)")),
        (Some(1024), tr("Slow (1 MiB/s)")),
        (Some(5120), tr("Medium (5 MiB/s)")),
        (Some(10240), tr("Fast cap (10 MiB/s)")),
    ]
}

fn bandwidth_combo_row(current: Option<u32>) -> libadwaita::ComboRow {
    let presets = bandwidth_presets();
    let labels: Vec<&str> = presets.iter().map(|(_, l)| l.as_str()).collect();
    let model = gtk::StringList::new(&labels);

    let selected = bandwidth_combo_index(current, &presets);
    let row = libadwaita::ComboRow::builder()
        .title(&tr("Upload speed"))
        .subtitle(&tr("Limits how fast data is sent to the backup server."))
        .model(&model)
        .build();
    row.set_selected(selected);
    row
}

fn bandwidth_combo_index(kib: Option<u32>, presets: &[(Option<u32>, String)]) -> u32 {
    match kib {
        None | Some(0) => 0,
        Some(v) => presets
            .iter()
            .position(|(k, _)| *k == Some(v))
            .unwrap_or_else(|| {
                presets
                    .iter()
                    .enumerate()
                    .skip(1)
                    .min_by_key(|(_, (k, _))| k.map(|x| x.abs_diff(v)).unwrap_or(u32::MAX))
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            }) as u32,
    }
}

fn bandwidth_at_combo_index(index: u32) -> Option<u32> {
    bandwidth_presets()
        .get(index as usize)
        .and_then(|(k, _)| *k)
}

fn bind_schedule_visibility(
    schedule_row: &libadwaita::ComboRow,
    schedule_time: &libadwaita::EntryRow,
    schedule_weekday: &libadwaita::ComboRow,
    schedule_cron: &libadwaita::EntryRow,
) {
    let update = {
        let schedule_row = schedule_row.clone();
        let schedule_time = schedule_time.clone();
        let schedule_weekday = schedule_weekday.clone();
        let schedule_cron = schedule_cron.clone();
        move || {
            apply_schedule_visibility(
                &schedule_row,
                &schedule_time,
                &schedule_weekday,
                &schedule_cron,
            )
        }
    };
    update();
    let update_for_signal = update.clone();
    schedule_row.connect_selected_notify(move |_| update_for_signal());
}

fn apply_schedule_visibility(
    schedule_row: &libadwaita::ComboRow,
    schedule_time: &libadwaita::EntryRow,
    schedule_weekday: &libadwaita::ComboRow,
    schedule_cron: &libadwaita::EntryRow,
) {
    let t = schedule_type_at_index(schedule_row.selected());
    let timed = matches!(t, ScheduleType::Daily | ScheduleType::Weekly);
    schedule_time.set_visible(timed);
    schedule_weekday.set_visible(t == ScheduleType::Weekly);
    schedule_cron.set_visible(t == ScheduleType::Custom && advanced_ui::advanced_mode_enabled());
}

fn weekday_combo_row(selected: u8) -> libadwaita::ComboRow {
    let labels = vec![
        tr("Monday"),
        tr("Tuesday"),
        tr("Wednesday"),
        tr("Thursday"),
        tr("Friday"),
        tr("Saturday"),
        tr("Sunday"),
    ];
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&refs);
    let row = libadwaita::ComboRow::builder()
        .title(&tr("Day of week"))
        .model(&model)
        .build();
    let idx = selected.saturating_sub(1).min(6) as u32;
    row.set_selected(idx);
    row
}

fn weekday_at_index(index: u32) -> u8 {
    (index + 1).min(7) as u8
}

fn schedule_combo_row(selected: ScheduleType) -> libadwaita::ComboRow {
    let types = [
        ScheduleType::Daily,
        ScheduleType::Hourly,
        ScheduleType::Weekly,
        ScheduleType::OnLogin,
        ScheduleType::Custom,
    ];
    let labels: Vec<String> = types.iter().map(|t| schedule_type_label(*t)).collect();
    let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let model = gtk::StringList::new(&refs);
    let idx = types
        .iter()
        .position(|t| *t == selected)
        .unwrap_or(0) as u32;
    let row = libadwaita::ComboRow::builder()
        .title(&tr("How often"))
        .model(&model)
        .build();
    row.set_selected(idx);
    row
}

fn schedule_type_label(t: ScheduleType) -> String {
    match t {
        ScheduleType::Daily => tr("Every day"),
        ScheduleType::Hourly => tr("Every hour"),
        ScheduleType::Weekly => tr("Once a week"),
        ScheduleType::OnLogin => tr("When you sign in"),
        ScheduleType::Custom => tr("Advanced"),
    }
}
