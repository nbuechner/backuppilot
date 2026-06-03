//! Preflight check UI in the profile editor.

use backuppilot_core::PreflightCheck;
use backuppilot_i18n::{tr, tr_fmt};
use gtk::prelude::*;
use libadwaita::prelude::*;

pub fn build_group() -> (libadwaita::PreferencesGroup, gtk::ListBox, gtk::Button) {
    let group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Preflight checks"))
        .description(&tr("DNS, network port, PBS login, paths, and backup conditions for this profile."))
        .build();

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(8)
        .build();
    list.set_widget_name("profile-preflight-list");

    let (row, run_btn) = crate::util::preferences_action_row_with_button(
        &tr("Check readiness"),
        &tr("Run all checks without starting a backup."),
        &tr("Run preflight now"),
    );
    group.add(&row);
    group.add(&list);

    (group, list, run_btn)
}

fn localized_label(check: &PreflightCheck) -> String {
    match check.id.as_str() {
        "enabled" => tr("Profile enabled"),
        "paths" => tr("Backup paths configured"),
        "path_exists" => tr("Backup path exists"),
        "path_readable" => tr("Read permission on backup path"),
        "path_writable" => tr("Read permission on backup path"),
        "pbs_client" => tr("proxmox-backup-client installed"),
        "api_token" => tr("API token stored"),
        "ac_power" => tr("On AC power"),
        "network" => tr("Required network active"),
        "vpn" => tr("VPN connection active"),
        "dns" => tr("PBS DNS resolution"),
        "tcp" => tr("PBS TCP port reachable"),
        "pbs_auth" => tr("PBS authentication"),
        _ => tr(&check.label),
    }
}

fn localized_detail(check: &PreflightCheck) -> String {
    let Some(detail) = &check.detail else {
        return if check.ok { tr("OK")} else { tr("Failed")};
    };
    let d = detail.as_str();
    if let Some(path) = d.strip_prefix("path does not exist: ") {
        return tr_fmt("path does not exist: {path}", &[("path", path)]);
    }
    if let Some(path) = d.strip_prefix("no read permission: ") {
        return tr_fmt("no read permission: {path}", &[("path", path)]);
    }
    if let Some(path) = d.strip_prefix("no write permission: ") {
        return tr_fmt("no read permission: {path}", &[("path", path)]);
    }
    if let Some(rest) = d.strip_prefix("required network not active (") {
        let names = rest.trim_end_matches(')');
        return tr_fmt(
            "required network not active ({names})",
            &[("names", names)],
        );
    }
    match d {
        "profile is disabled" => tr("profile is disabled"),
        "no backup paths configured" => tr("no backup paths configured"),
        "proxmox-backup-client not found" => tr("proxmox-backup-client not found"),
        "API token not available for background backups — open the profile and save again" => {
            tr("API token not available for background backups , open the profile and save again")}
        "device not on AC power" => tr("device not on AC power"),
        "VPN connection required but not active" => tr("VPN connection required but not active"),
        _ if d.starts_with("DNS lookup failed") => tr(d),
        _ if d.starts_with("cannot reach PBS") => tr(d),
        _ if d.starts_with("invalid PBS") => tr(d),
        _ if d.starts_with("PBS at") => tr(d),
        _ => detail.clone(),
    }
}

pub fn fill_list(list: &gtk::ListBox, checks: &[PreflightCheck]) {
    crate::util::clear_list_box(list);

    if checks.is_empty() {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::builder()
            .label(&tr("No checks yet , tap «Run preflight now»."))
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

    for check in checks {
        let row = gtk::ListBoxRow::new();
        let (icon, css) = if check.ok {
            ("emblem-ok-symbolic", "success")
        } else {
            ("dialog-warning-symbolic", "warning")
        };
        let action = libadwaita::ActionRow::builder()
            .title(&localized_label(check))
            .subtitle(&localized_detail(check))
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
