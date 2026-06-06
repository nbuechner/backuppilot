//! YAML import/export for profiles (same format as `backuppilot-cli profile`).

use backuppilot_core::{
    merge_repository_for_update, normalize_new_profile, parse_profile_yaml, profile_to_yaml,
    Database,
};
use backuppilot_i18n::{tr, tr_fmt};
use gtk::prelude::*;
use libadwaita::{ApplicationWindow, Toast, ToastOverlay};


use crate::dbus_client::{self, connect};
use crate::dbus_runtime;

pub fn import_profile_yaml(
    parent: &ApplicationWindow,
    toast: &ToastOverlay,
    on_done: impl Fn() + 'static + Clone,
) {
    let picker = gtk::FileDialog::builder()
        .title(&tr("Import profile"))
        .modal(true)
        .build();
    let toast = toast.clone();
    let parent = parent.clone();
    let on_done = std::rc::Rc::new(on_done);
    picker.open(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            let Ok(file) = result else { return };
            let Some(path) = file.path() else { return };
            let contents = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => {
                    toast.add_toast(Toast::new(&tr("Could not read file.")));
                    return;
                }
            };
            let doc = match parse_profile_yaml(&contents) {
                Ok(d) => d,
                Err(err) => {
                    toast.add_toast(Toast::new(&tr_fmt(
                        "Invalid profile file: {err}",
                        &[("err", &err.to_string())],
                    )));
                    return;
                }
            };
            let name = doc.name.clone();
            let toast_save = toast.clone();
            let on_done_save = on_done.clone();
            dbus_runtime::spawn(
                async move {
                    let proxy = connect().await?;
                    let profiles = dbus_client::list_profiles(&proxy).await?;
                    let existing = profiles
                        .iter()
                        .find(|p| p.name.eq_ignore_ascii_case(&name))
                        .map(|p| p.id);
                    let db = Database::open().map_err(|e| backuppilot_ipc::IpcError::failure(e.to_string()))?;
                    let mut new = doc
                        .into_new_profile(&db)
                        .map_err(|e| backuppilot_ipc::IpcError::failure(e.to_string()))?;
                    let updated = if let Some(id) = existing {
                        new.repository = merge_repository_for_update(id, &new.repository)
                            .map_err(|e| backuppilot_ipc::IpcError::failure(e.to_string()))?;
                        dbus_client::update_profile(&proxy, id, &normalize_new_profile(new)).await?;
                        true
                    } else {
                        dbus_client::create_profile(&proxy, &new).await?;
                        false
                    };
                    Ok::<_, backuppilot_ipc::IpcError>((name, updated))
                },
                move |result| match result {
                    Ok((name, updated)) => {
                        let msg = if updated {
                            tr_fmt("Updated profile {name}.", &[("name", &name)])
                        } else {
                            tr_fmt("Imported profile {name}.", &[("name", &name)])
                        };
                        toast_save.add_toast(Toast::new(&msg));
                        on_done_save();
                    }
                    Err(_) => toast_save.add_toast(Toast::new(&tr("Import failed."))),
                },
            );
        },
    );
}

pub fn export_profile_yaml(
    parent: &ApplicationWindow,
    toast: &ToastOverlay,
    profile_id: i64,
    profile_name: &str,
) {
    let default_name = format!(
        "{}.yaml",
        profile_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
    );
    let picker = gtk::FileDialog::builder()
        .title(&tr("Export profile"))
        .initial_name(&default_name)
        .modal(true)
        .build();
    let parent = parent.clone();
    let toast = toast.clone();
    picker.save(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |save_result| {
            let Ok(file) = save_result else { return };
            let Some(target) = file.path() else { return };
            let toast = toast.clone();
            dbus_runtime::spawn(
                async move {
                    let db = Database::open().map_err(|e| backuppilot_ipc::IpcError::failure(e.to_string()))?;
                    let profile = db
                        .get_profile(profile_id)
                        .map_err(|e| backuppilot_ipc::IpcError::failure(e.to_string()))?;
                    let yaml = profile_to_yaml(&db, &profile)
                        .map_err(|e| backuppilot_ipc::IpcError::failure(e.to_string()))?;
                    std::fs::write(&target, yaml).map_err(|e| backuppilot_ipc::IpcError::failure(e.to_string()))?;
                    Ok(())
                },
                move |result| {
                    if result.is_ok() {
                        toast.add_toast(Toast::new(&tr("Profile exported.")));
                    } else {
                        toast.add_toast(Toast::new(&tr("Export failed.")));
                    }
                },
            );
        },
    );
}
