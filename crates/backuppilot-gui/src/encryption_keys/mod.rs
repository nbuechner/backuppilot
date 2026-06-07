//! Manage PBS backup encryption keys (create, import, export, delete).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{DateTime, Local, Utc};
use backuppilot_core::{
    encryption_key_in_use, key_absolute_path, CreateEncryptionKeyInput, EncryptionKey,
    ImportEncryptionKeyInput,
};
use backuppilot_i18n::{tr, tr_fmt};
use gtk::prelude::*;
use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, Toast, ToastOverlay};

use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::profiles::build_info_callout;
use crate::util::clear_list_box;

thread_local! {
    static PAGE_CTX: RefCell<Option<PageCtx>> = const { RefCell::new(None) };
}

struct PageCtx {
    parent: ApplicationWindow,
    toast: ToastOverlay,
    list: gtk::ListBox,
}

pub fn build_page(parent: &ApplicationWindow, toast_overlay: &ToastOverlay) -> gtk::Widget {
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(16)
        .margin_end(16)
        .vexpand(true)
        .build();
    outer.set_widget_name("encryption-keys-page");

    let loss_warning = build_info_callout(
        &tr("Encryption key required for restore"),
        &tr("If you lose an encryption key or its password, your backups cannot be restored , not even by administrators on the backup server."),
    );

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let create_btn = gtk::Button::builder()
        .label(&tr("Create key"))
        .css_classes(["suggested-action"])
        .build();
    let import_btn = gtk::Button::with_label(&tr("Import key"));
    header.append(&create_btn);
    header.append(&import_btn);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .vexpand(true)
        .build();
    list.set_widget_name("encryption-keys-list");

    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .min_content_height(240)
        .child(&list)
        .build();

    outer.append(&loss_warning);
    outer.append(&header);
    outer.append(&scroll);

    let parent_win = parent.clone();
    PAGE_CTX.with(|slot| {
        *slot.borrow_mut() = Some(PageCtx {
            parent: parent_win.clone(),
            toast: toast_overlay.clone(),
            list: list.clone(),
        });
    });

    let parent_for_create = parent.clone();
    let parent_for_import = parent.clone();
    create_btn.connect_clicked({
        let toast = toast_overlay.clone();
        move |_| show_create_dialog(&parent_for_create, toast.clone())
    });
    import_btn.connect_clicked({
        let toast = toast_overlay.clone();
        move |_| show_import_dialog(&parent_for_import, toast.clone())
    });

    outer.upcast()
}

pub fn refresh() {
    PAGE_CTX.with(|slot| {
        if let Some(ctx) = slot.borrow().as_ref() {
            refresh_list_internal(&ctx.list, &ctx.toast);
        }
    });
}

fn refresh_list_internal(list: &gtk::ListBox, toast: &ToastOverlay) {
    clear_list_box(list);
    let list = list.clone();
    let toast = toast.clone();
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::list_encryption_keys(&proxy).await
        },
        move |result| match result {
            Ok(keys) => {
                if keys.is_empty() {
                    append_hint_row(
                        &list,
                        &tr("No encryption keys yet. Create or import a key to use with profiles."),
                    );
                    return;
                }
                for key in keys {
                    list.append(&key_row(&key, toast.clone()));
                }
            }
            Err(err) => {
                append_hint_row(
                    &list,
                    &tr_fmt("Could not load keys: {err}", &[("err", &err.to_string())]),
                );
            }
        },
    );
}

fn key_row(key: &EncryptionKey, toast: ToastOverlay) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::builder()
        .selectable(false)
        .activatable(false)
        .build();

    let in_use = encryption_key_in_use(key);
    let subtitle = key_row_subtitle(key);

    let action = libadwaita::ActionRow::builder()
        .title(&key.name)
        .subtitle(&subtitle)
        .subtitle_lines(4)
        .activatable(false)
        .build();

    let icon_css: &[&str] = if in_use {
        &["success"]
    } else {
        &["dim-label"]
    };
    action.add_prefix(
        &gtk::Image::builder()
            .icon_name("security-high-symbolic")
            .pixel_size(22)
            .css_classes(icon_css)
            .valign(gtk::Align::Start)
            .margin_top(2)
            .build(),
    );

    let export_btn = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .tooltip_text(&tr("Save a copy of the key file"))
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    let delete_tooltip = if in_use {
        tr("Cannot delete while key is in use")} else {
        tr("Delete key")};
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(&delete_tooltip)
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    if in_use {
        delete_btn.add_css_class("dim-label");
        delete_btn.remove_css_class("destructive-action");
    } else {
        delete_btn.add_css_class("destructive-action");
    }
    delete_btn.set_sensitive(!in_use);

    let key_id = key.id;
    let key_name_export = key.name.clone();
    let key_name_delete = key.name.clone();
    let toast_export = toast.clone();
    export_btn.connect_clicked(move |_| {
        PAGE_CTX.with(|slot| {
            if let Some(ctx) = slot.borrow().as_ref() {
                export_key_file(
                    &ctx.parent,
                    key_id,
                    &key_name_export,
                    toast_export.clone(),
                );
            }
        });
    });
    let toast_delete = toast.clone();
    delete_btn.connect_clicked(move |_| {
        if !in_use {
            confirm_delete_key(key_id, &key_name_delete, toast_delete.clone());
        }
    });

    action.add_suffix(&export_btn);
    action.add_suffix(&delete_btn);
    row.set_child(Some(&action));
    row
}

fn key_row_subtitle(key: &EncryptionKey) -> String {
    let mut lines = vec![
        tr_fmt("Created {when}", &[("when", &format_key_datetime(key.created_at))]),
        tr_fmt("Usage: {usage}", &[("usage", &key_usage_text(key))]),
        tr_fmt(
            "Last saved: {when}",
            &[("when", &key_saved_text(key))],
        ),
    ];
    let pw = if key.password_configured {
        tr("Password stored")} else {
        tr("Password missing")};
    if let Some(hint) = key.password_hint.as_deref().filter(|s| !s.is_empty()) {
        lines.push(tr_fmt("{status} , hint: {hint}", &[("status", &pw), ("hint", hint)]));
    } else {
        lines.push(pw);
    }
    lines.join("\n")
}

fn format_key_datetime(dt: DateTime<Utc>) -> String {
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

fn key_usage_text(key: &EncryptionKey) -> String {
    if key.profile_usage.is_empty() {
        return tr("Not used");
    }
    key.profile_usage
        .iter()
        .map(format_key_profile_usage)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_key_profile_usage(u: &backuppilot_core::EncryptionKeyProfileUsage) -> String {
    if u.encrypted_snapshots > 0 {
        tr_fmt(
            "{profile} ({count} encrypted snapshots)",
            &[
                ("profile", &u.profile_name),
                ("count", &u.encrypted_snapshots.to_string()),
            ],
        )
    } else if u.assigned {
        tr_fmt("{profile} (assigned)", &[("profile", &u.profile_name)])
    } else {
        u.profile_name.clone()
    }
}

fn key_saved_text(key: &EncryptionKey) -> String {
    match key.last_exported_at {
        Some(dt) => format_key_datetime(dt),
        None => tr("Never"),
    }
}

fn append_hint_row(list: &gtk::ListBox, text: &str) {
    let row = gtk::ListBoxRow::builder()
        .selectable(false)
        .activatable(false)
        .build();
    let label = gtk::Label::builder()
        .label(text)
        .wrap(true)
        .xalign(0.0)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .css_classes(["dim-label"])
        .build();
    row.set_child(Some(&label));
    list.append(&row);
}

fn show_create_dialog(parent: &ApplicationWindow, toast: ToastOverlay) {
    let win = libadwaita::Window::builder()
        .title(&tr("Create encryption key"))
        .modal(true)
        .transient_for(parent)
        .default_width(520)
        .default_height(480)
        .build();

    let header = libadwaita::HeaderBar::new();
    let cancel_btn = gtk::Button::with_label(&tr("Cancel"));
    let create_btn = gtk::Button::builder()
        .label(&tr("Create"))
        .css_classes(["suggested-action"])
        .build();
    header.pack_start(&cancel_btn);
    header.pack_end(&create_btn);

    let info = build_info_callout(
        &tr("Create encryption key"),
        &tr("A new PBS encryption key will be created. Save a backup copy immediately (password manager, USB, safe). Without key and password, encrypted backups cannot be restored."),
    );

    let name_row = libadwaita::EntryRow::builder()
        .title(&tr("Name"))
        .text(&tr("Laptop backup key"))
        .build();
    let pass_row = libadwaita::PasswordEntryRow::builder()
        .title(&tr("Encryption password"))
        .show_apply_button(false)
        .build();
    let confirm_row = libadwaita::PasswordEntryRow::builder()
        .title(&tr("Confirm password"))
        .show_apply_button(false)
        .build();
    let hint_row = libadwaita::EntryRow::builder()
        .title(&tr("Password hint (optional)"))
        .build();

    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .vexpand(true)
        .build();
    form.append(&info);
    form.append(&name_row);
    form.append(&pass_row);
    form.append(&confirm_row);
    form.append(&hint_row);

    let toolbar = libadwaita::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&form));
    win.set_content(Some(&toolbar));

    let toast_create = toast.clone();
    let win_weak_cancel = win.downgrade();
    let win_weak_create = win.downgrade();
    cancel_btn.connect_clicked(move |_| {
        if let Some(w) = win_weak_cancel.upgrade() {
            w.close();
        }
    });
    let parent_clone = parent.clone();
    create_btn.connect_clicked(move |_| {
        let name = name_row.text().trim().to_string();
        let pass = pass_row.text().to_string();
        let confirm = confirm_row.text().to_string();
        if pass != confirm {
            toast.add_toast(Toast::new(&tr("Passwords do not match.")));
            return;
        }
        if pass.len() < 8 {
            toast.add_toast(Toast::new(&tr("Password must be at least 8 characters.")));
            return;
        }
        let input = CreateEncryptionKeyInput {
            name,
            password: pass,
            password_hint: {
                let h = hint_row.text().trim().to_string();
                if h.is_empty() { None } else { Some(h) }
            },
        };
        if let Some(w) = win_weak_create.upgrade() {
            w.close();
        }
        let toast_spawn = toast_create.clone();
        let parent_spawn = parent_clone.clone();
        dbus_runtime::spawn(
            async move {
                let proxy = connect().await?;
                dbus_client::create_encryption_key(&proxy, &input).await
            },
            move |result| match result {
                Ok(key) => {
                    let t = Toast::new(&tr_fmt(
                        "Key «{name}» created , save a backup copy now.",
                        &[("name", &key.name)],
                    ));
                    t.set_timeout(8);
                    toast_spawn.add_toast(t);
                    refresh();
                    prompt_export_after_create(
                        &parent_spawn,
                        key.id,
                        &key.name,
                        toast_spawn.clone(),
                    );
                }
                Err(err) => {
                    toast_spawn.add_toast(Toast::new(&tr_fmt(
                        "Could not create key: {err}",
                        &[("err", &err.to_string())],
                    )));
                }
            },
        );
    });

    win.present();
}

fn prompt_export_after_create(
    parent: &ApplicationWindow,
    key_id: i64,
    key_name: &str,
    toast: ToastOverlay,
) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(&tr("Save encryption key backup"))
        .body(&tr("Store a copy of the key file outside BackupPilot. You need the key file and password to restore encrypted backups."))
        .build();
    alert.add_response("later", &tr("Later"));
    alert.add_response("export", &tr("Save copy…"));
    alert.set_response_appearance("export", libadwaita::ResponseAppearance::Suggested);
    alert.set_default_response(Some("export"));
    alert.set_close_response("later");
    let parent_export = parent.clone();
    let parent_present = parent.clone();
    let key_name = key_name.to_string();
    alert.connect_response(None, move |_, response| {
        if response == "export" {
            export_key_file(&parent_export, key_id, &key_name, toast.clone());
        }
    });
    alert.present(Some(&parent_present));
}

fn show_import_dialog(parent: &ApplicationWindow, toast: ToastOverlay) {
    let win = libadwaita::Window::builder()
        .title(&tr("Import encryption key"))
        .modal(true)
        .transient_for(parent)
        .default_width(480)
        .default_height(360)
        .build();

    let header = libadwaita::HeaderBar::new();
    let cancel_btn = gtk::Button::with_label(&tr("Cancel"));
    let import_btn = gtk::Button::builder()
        .label(&tr("Import"))
        .css_classes(["suggested-action"])
        .build();
    header.pack_start(&cancel_btn);
    header.pack_end(&import_btn);

    let selected_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    let file_row = libadwaita::ActionRow::builder()
        .title(&tr("Key file"))
        .subtitle(&tr("No file selected yet"))
        .build();
    let choose_file_btn = gtk::Button::with_label(&tr("Choose file…"));
    file_row.add_suffix(&choose_file_btn);

    let name_row = libadwaita::EntryRow::builder().title(&tr("Name")).build();
    let pass_row = libadwaita::PasswordEntryRow::builder()
        .title(&tr("Encryption password"))
        .show_apply_button(false)
        .build();

    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .vexpand(true)
        .build();
    form.append(&gtk::Label::builder()
        .label(&tr(
            "Choose the PBS key file (JSON), enter a display name and the encryption password, then tap Import.",
        ))
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build());
    form.append(&file_row);
    form.append(&name_row);
    form.append(&pass_row);

    let toolbar = libadwaita::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&form));
    win.set_content(Some(&toolbar));

    let toast_import = toast.clone();
    let win_weak_cancel = win.downgrade();
    cancel_btn.connect_clicked(move |_| {
        if let Some(w) = win_weak_cancel.upgrade() {
            w.close();
        }
    });

    let win_for_picker = win.clone();
    let file_row_pick = file_row.clone();
    let selected_for_pick = selected_path.clone();
    let toast_pick = toast_import.clone();
    choose_file_btn.connect_clicked(move |_| {
        let picker = gtk::FileDialog::builder()
            .title(&tr("Select encryption key file"))
            .modal(true)
            .build();
        let toast_pick = toast_pick.clone();
        let selected_for_pick = selected_for_pick.clone();
        let file_row_pick = file_row_pick.clone();
        picker.open(
            Some(&win_for_picker),
            None::<&gtk::gio::Cancellable>,
            move |result| {
                let Ok(file) = result else {
                    return;
                };
                let Some(path) = file.path() else {
                    toast_pick.add_toast(Toast::new(&tr(
                        "Could not read the selected file path.",
                    )));
                    return;
                };
                *selected_for_pick.borrow_mut() = Some(path.clone());
                let subtitle = path.display().to_string();
                file_row_pick.set_subtitle(&subtitle);
            },
        );
    });

    let win_weak_import = win.downgrade();
    import_btn.connect_clicked(move |_| {
        let name = name_row.text().trim().to_string();
        if name.is_empty() {
            toast_import.add_toast(Toast::new(&tr("Name is required.")));
            return;
        }
        let password = pass_row.text().to_string();
        if password.is_empty() {
            toast_import.add_toast(Toast::new(&tr("Encryption password is required.")));
            return;
        }
        let Some(path) = selected_path.borrow().clone() else {
            toast_import.add_toast(Toast::new(&tr("Choose a key file first.")));
            return;
        };
        if let Some(w) = win_weak_import.upgrade() {
            w.close();
        }
        let input = ImportEncryptionKeyInput {
            name,
            source_path: path.display().to_string(),
            password,
            password_hint: None,
        };
        let toast_done = toast_import.clone();
        dbus_runtime::spawn(
            async move {
                let proxy = connect().await?;
                dbus_client::import_encryption_key(&proxy, &input).await
            },
            move |result| match result {
                Ok(key) => {
                    toast_done.add_toast(Toast::new(&tr_fmt(
                        "Key «{name}» imported.",
                        &[("name", &key.name)],
                    )));
                    refresh();
                }
                Err(err) => {
                    toast_done.add_toast(Toast::new(&tr_fmt(
                        "Import failed: {err}",
                        &[("err", &err.to_string())],
                    )));
                }
            },
        );
    });

    win.present();
}

fn encryption_key_export_filename(name: &str) -> String {
    let safe: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base = if safe.is_empty() {
        "encryption-key".to_string()
    } else {
        safe
    };
    if base.to_ascii_lowercase().ends_with(".json") {
        base
    } else {
        format!("{base}.json")
    }
}

fn export_key_file(
    parent: &ApplicationWindow,
    key_id: i64,
    key_name: &str,
    toast: ToastOverlay,
) {
    let src = key_absolute_path(&format!("encryption-keys/{key_id}.json"));
    if !src.is_file() {
        toast.add_toast(Toast::new(&tr("Key file is missing on disk.")));
        return;
    }
    let default_name = encryption_key_export_filename(key_name);
    let picker = gtk::FileDialog::builder()
        .title(&tr("Save encryption key backup"))
        .initial_name(&default_name)
        .modal(true)
        .build();
    picker.save(
        Some(parent),
        None::<&gtk::gio::Cancellable>,
        move |save_result| {
            let Ok(file) = save_result else { return };
            let Some(target) = file.path() else { return };
            if std::fs::copy(&src, &target).is_err() {
                toast.add_toast(Toast::new(&tr("Could not save key file.")));
                return;
            }
            toast.add_toast(Toast::new(&tr("Key file saved. Keep it separate from this computer and from the backup server.")));
            dbus_runtime::spawn(
                async move {
                    let proxy = connect().await?;
                    dbus_client::mark_encryption_key_exported(&proxy, key_id).await
                },
                move |result| {
                    if result.is_ok() {
                        refresh();
                    }
                },
            );
        },
    );
}

fn confirm_delete_key(key_id: i64, name: &str, toast: ToastOverlay) {
    let parent = PAGE_CTX.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|c| c.parent.clone())
    });
    let Some(parent) = parent else {
        return;
    };
    let alert = libadwaita::AlertDialog::builder()
        .heading(&tr_fmt("Delete key \"{name}\"?", &[("name", name)]))
        .body(&tr("The key file will be permanently deleted. Make sure you have a backup copy if you may need to restore encrypted snapshots."))
        .build();
    alert.add_response("cancel", &tr("Cancel"));
    alert.add_response("delete", &tr("Delete"));
    alert.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");
    let toast_alert = toast.clone();
    alert.connect_response(None, move |_, response| {
        if response != "delete" {
            return;
        }
        let toast_result = toast_alert.clone();
        dbus_runtime::spawn(
            async move {
                let proxy = connect().await?;
                dbus_client::delete_encryption_key(&proxy, key_id).await
            },
            move |result| {
                match result {
                    Ok(()) => {
                        toast_result.add_toast(Toast::new(&tr("Encryption key deleted.")));
                        refresh();
                    }
                    Err(err) => {
                        toast_result.add_toast(Toast::new(&tr_fmt(
                            "Could not delete: {err}",
                            &[("err", &err.to_string())],
                        )));
                    }
                }
            },
        );
    });
    alert.present(Some(&parent));
}
