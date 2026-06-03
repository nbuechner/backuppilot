//! FUSE mount confirmation and file-manager integration.

use std::rc::Rc;

use backuppilot_core::pbs_mount::{fuse_available, mount_session_id, MountSnapshotRequest};
use backuppilot_core::ActiveMount;
use backuppilot_i18n::{tr, tr_fmt};
use gtk::prelude::*;
use libadwaita::prelude::{ActionRowExt, AdwDialogExt, AlertDialogExt, PreferencesGroupExt};
use libadwaita::{AlertDialog, ApplicationWindow, Toast, ToastOverlay};

use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::restore::browser::ArchiveBrowseItem;
use crate::window;

pub fn open_path_in_file_manager(path: &str) {
    if path.is_empty() {
        return;
    }
    let _ = std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub fn confirm_and_mount_archive(
    parent: &ApplicationWindow,
    toast: &ToastOverlay,
    profile_id: i64,
    snapshot: String,
    item: ArchiveBrowseItem,
    encryption_key_id: Option<i64>,
    on_done: Rc<dyn Fn()>,
) {
    if !fuse_available() {
        toast.add_toast(Toast::new(&tr("FUSE is not available on this system. Install fuse3 to mount backups in the file manager.")));
        return;
    }

    let snapshot_label = snapshot
        .rsplit('/')
        .next()
        .unwrap_or(&snapshot)
        .to_string();

    let body = tr_fmt(
        "Mount «{path}» from snapshot {snapshot} as a read-only folder?\n\n• Requires network access to the backup server while browsing\n• May use noticeable CPU and bandwidth when opening many files\n• Only mount backups you trust\n• Disconnect the mount from the overview when you are done",
        &[("path", &item.source_path), ("snapshot", &snapshot_label)],
    );

    let alert = AlertDialog::builder()
        .heading(&tr("Mount backup in file manager?"))
        .body(&body)
        .build();
    alert.add_response("cancel", &tr("Cancel"));
    alert.add_response("mount", &tr("Mount"));
    alert.set_response_appearance("mount", libadwaita::ResponseAppearance::Suggested);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");

    let toast = toast.clone();
    let on_done = on_done.clone();
    let item = item.clone();
    alert.connect_response(None::<&str>, move |_, response| {
        if response != "mount" {
            return;
        }
        start_mount(
            toast.clone(),
            profile_id,
            snapshot.clone(),
            item.clone(),
            encryption_key_id,
            on_done.clone(),
        );
    });
    alert.present(Some(parent));
}

fn start_mount(
    toast: ToastOverlay,
    profile_id: i64,
    snapshot: String,
    item: ArchiveBrowseItem,
    encryption_key_id: Option<i64>,
    on_done: Rc<dyn Fn()>,
) {
    let request = MountSnapshotRequest {
        profile_id,
        snapshot: snapshot.clone(),
        archive_name: item.archive_name.clone(),
        source_label: item.source_path.clone(),
        encryption_key_id,
    };

    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::mount_snapshot(&proxy, &request).await
        },
        move |result| {
            on_done();
            match result {
                Ok(res) if res.ok => {
                    if let Some(mount) = res.mount.as_ref() {
                        toast.add_toast(Toast::new(&tr_fmt(
                            "Mounted at {path}",
                            &[("path", &mount.mount_point)],
                        )));
                        window::refresh_dashboard_public();
                    }
                }
                Ok(res) => {
                    let msg = res
                        .message
                        .unwrap_or_else(|| tr("Mount failed."));
                    toast.add_toast(Toast::new(&msg));
                }
                Err(err) => {
                    toast.add_toast(Toast::new(&tr_fmt(
                        "Mount failed: {err}",
                        &[("err", &err.to_string())],
                    )));
                }
            }
        },
    );
}

pub fn mount_state_for_archive(
    active_mounts: &[ActiveMount],
    profile_id: i64,
    snapshot: &str,
    archive_name: &str,
    mounting_ids: &std::collections::HashSet<String>,
) -> crate::restore::browser::ArchiveMountBtnState {
    let id = mount_session_id(profile_id, snapshot, archive_name);
    if mounting_ids.contains(&id) {
        return crate::restore::browser::ArchiveMountBtnState::Mounting;
    }
    if active_mounts.iter().any(|m| m.id == id) {
        return crate::restore::browser::ArchiveMountBtnState::Mounted;
    }
    crate::restore::browser::ArchiveMountBtnState::Mount
}

pub fn active_mount_for_archive<'a>(
    active_mounts: &'a [ActiveMount],
    profile_id: i64,
    snapshot: &str,
    archive_name: &str,
) -> Option<&'a ActiveMount> {
    let id = mount_session_id(profile_id, snapshot, archive_name);
    active_mounts.iter().find(|m| m.id == id)
}

pub fn confirm_unmount(
    parent: &ApplicationWindow,
    toast: &ToastOverlay,
    mount: ActiveMount,
    on_done: Rc<dyn Fn()>,
) {
    let alert = AlertDialog::builder()
        .heading(&tr("Disconnect mounted backup?"))
        .body(&tr_fmt(
            "Stop exposing “{path}” at {mount}?\n\nFiles in the file manager will no longer be available until you mount again.",
            &[
                ("path", &mount.source_label),
                ("mount", &mount.mount_point),
            ],
        ))
        .build();

    alert.add_response("cancel", &tr("Cancel"));
    alert.add_response("unmount", &tr("Disconnect"));
    alert.set_response_appearance("unmount", libadwaita::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");

    let mount_id = mount.id.clone();
    let toast = toast.clone();
    let on_done = Rc::new(std::cell::RefCell::new(Some(on_done)));
    alert.connect_response(None::<&str>, move |_, response| {
        if response != "unmount" {
            return;
        }
        let mount_id = mount_id.clone();
        let toast = toast.clone();
        let on_done = Rc::clone(&on_done);
        dbus_runtime::spawn(
            async move {
                let proxy = connect().await?;
                dbus_client::unmount_snapshot(&proxy, &mount_id).await
            },
            move |result| {
                if let Some(done) = on_done.borrow_mut().take() {
                    done();
                }
                match result {
                    Ok(res) if res.ok => {
                        toast.add_toast(Toast::new(&tr("Mount disconnected.")));
                        window::refresh_dashboard_public();
                    }
                    Ok(res) => {
                        let msg = res
                            .message
                            .unwrap_or_else(|| tr("Could not disconnect mount."));
                        toast.add_toast(Toast::new(&msg));
                    }
                    Err(err) => {
                        toast.add_toast(Toast::new(&tr_fmt(
                            "Disconnect failed: {err}",
                            &[("err", &err.to_string())],
                        )));
                    }
                }
            },
        );
    });
    alert.present(Some(parent));
}

pub fn apply_dashboard_mount_banners(
    container: &gtk::Box,
    mounts: &[ActiveMount],
    parent: &ApplicationWindow,
) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    if mounts.is_empty() {
        container.set_visible(false);
        return;
    }

    let group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Mounted backups"))
        .description(&tr("Backup archives exposed read-only in your file manager. Click a row to open the folder."))
        .build();

    let toast = window::toast_overlay();
    for mount in mounts {
        let row = libadwaita::ActionRow::builder()
            .title(&tr_fmt(
                "“{path}” mounted read-only",
                &[("path", &mount.source_label)],
            ))
            .subtitle(&mount.mount_point)
            .activatable(true)
            .build();

        let mount_point = mount.mount_point.clone();
        row.connect_activated(move |_| {
            open_path_in_file_manager(&mount_point);
        });

        let disconnect_btn = gtk::Button::builder()
            .label(&tr("Disconnect…"))
            .css_classes(["flat", "destructive-action"])
            .valign(gtk::Align::Center)
            .vexpand(false)
            .hexpand(false)
            .build();

        let mount_copy = mount.clone();
        let parent = parent.clone();
        let toast = toast.clone();
        disconnect_btn.connect_clicked(move |_| {
            if let Some(toast) = toast.as_ref() {
                confirm_unmount(
                    &parent,
                    toast,
                    mount_copy.clone(),
                    Rc::new(|| window::refresh_dashboard_public()),
                );
            }
        });
        row.add_suffix(&disconnect_btn);
        group.add(&row);
    }

    container.append(&group);
    container.set_visible(true);
}
