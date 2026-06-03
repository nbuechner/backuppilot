use std::path::Path;
use std::process::Command;

use backuppilot_core::pbs_install_result::{
    install_result_path_for_shell, PbsInstallResult, PbsInstallResultStatus,
};
use backuppilot_core::{
    is_flatpak_runtime, PbsClientInstallGuide, PbsClientInstallMethod, PBS_CLIENT_COPR_URL,
    PBS_CLIENT_DOC_URL,
};
use backuppilot_i18n::{tr, tr_fmt};
use gtk::prelude::*;
use libadwaita::prelude::{AlertDialogExt, AdwDialogExt};
use libadwaita::{AlertDialog, ApplicationWindow, Toast, ToastOverlay};

use crate::util::find_child_by_name;

/// Empty container prepended on the dashboard; filled when the PBS client is missing.
pub fn build_placeholder() -> gtk::Widget {
    let placeholder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .visible(false)
        .build();
    placeholder.set_widget_name("dashboard-pbs-setup");
    placeholder.upcast()
}

pub fn update_dashboard_banner(
    dashboard: &gtk::Widget,
    parent: &ApplicationWindow,
    toast: Option<&ToastOverlay>,
    available: bool,
) {
    let Some(container) = find_child_by_name(dashboard, "dashboard-pbs-setup") else {
        return;
    };
    let Some(container) = container.downcast_ref::<gtk::Box>() else {
        return;
    };

    if available {
        container.set_visible(false);
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        return;
    }

    let guide = PbsClientInstallGuide::detect();
    let needs_rebuild = container
        .first_child()
        .map(|w| w.widget_name().as_str() != banner_widget_name(&guide))
        .unwrap_or(true);

    if needs_rebuild {
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        container.append(&build_setup_block(&guide, parent, toast));
    }
    container.set_visible(true);
}

fn banner_widget_name(guide: &PbsClientInstallGuide) -> String {
    match guide.method {
        PbsClientInstallMethod::Apt => format!(
            "pbs-setup-apt-{}",
            guide.suite.as_deref().unwrap_or("unknown")
        ),
        PbsClientInstallMethod::DnfCopr => "pbs-setup-dnf-copr".into(),
        PbsClientInstallMethod::Manual => format!("pbs-setup-manual-{}", guide.os_id),
    }
}

fn build_setup_block(
    guide: &PbsClientInstallGuide,
    parent: &ApplicationWindow,
    toast: Option<&ToastOverlay>,
) -> gtk::Box {
    let block = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    block.set_widget_name(&banner_widget_name(guide));

    let title = gtk::Label::builder()
        .label(&tr("Proxmox Backup Client"))
        .xalign(0.0)
        .css_classes(["title-4"])
        .build();
    block.append(&title);

    let detected = gtk::Label::builder()
        .label(&tr_fmt(
            "Detected system: {system}",
            &[("system", &guide.system_label)],
        ))
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    block.append(&detected);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();

    if guide.supports_terminal_install() {
        if let Some(script) = guide.install_script() {
            let install_subtitle = match guide.method {
                PbsClientInstallMethod::Apt => tr(
                    "Configure the official Proxmox APT repository and install proxmox-backup-client (sudo required)",
                ),
                PbsClientInstallMethod::DnfCopr => tr(
                    "Enable COPR derenderkeks/proxmox-backup-client and install proxmox-backup-client (Fedora/RHEL, sudo required)",
                ),
                PbsClientInstallMethod::Manual => String::new(),
            };

            let toast_install = toast.cloned();
            let script_install = script.clone();
            let parent_install = parent.clone();
            list.append(&clickable_row(
                &tr("Install in terminal"),
                Some(&install_subtitle),
                "utilities-terminal-symbolic",
                move || {
                    handle_install_in_terminal(
                        &script_install,
                        &parent_install,
                        toast_install.as_ref(),
                    );
                },
            ));

            let toast_copy = toast.cloned();
            let script_copy = script;
            list.append(&clickable_row(
                &tr("Copy install script"),
                Some(&tr("Paste into a root shell if you prefer to install manually")),
                "edit-copy-symbolic",
                move || {
                    crate::util::copy_text_to_clipboard(&script_copy);
                    post_toast(
                        toast_copy.as_ref(),
                        &tr("Install script copied to clipboard"),
                    );
                },
            ));
        }
    } else {
        list.append(&info_row(&manual_install_hint(&guide.os_id)));
    }

    let parent_docs = parent.clone();
    match guide.method {
        PbsClientInstallMethod::DnfCopr => {
            list.append(&clickable_row(
                &tr("COPR package page"),
                Some(&tr(
                    "Community build for Fedora and RHEL (derenderkeks/proxmox-backup-client)",
                )),
                "external-link-symbolic",
                move || open_uri(&parent_docs, PBS_CLIENT_COPR_URL),
            ));
        }
        _ => {
            list.append(&clickable_row(
                &tr("Open installation guide"),
                Some(&tr("Official Proxmox Backup Server documentation (Debian/Ubuntu)")),
                "external-link-symbolic",
                move || open_uri(&parent_docs, PBS_CLIENT_DOC_URL),
            ));
        }
    }

    block.append(&list);
    block
}

fn clickable_row(
    title: &str,
    subtitle: Option<&str>,
    icon_name: &str,
    on_click: impl Fn() + Clone + 'static,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::builder()
        .activatable(true)
        .selectable(false)
        .build();

    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(10)
        .margin_bottom(10)
        .hexpand(true)
        .build();

    let text_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .build();

    let title_label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    text_box.append(&title_label);

    if let Some(subtitle) = subtitle {
        let subtitle_label = gtk::Label::builder()
            .label(subtitle)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .css_classes(["dim-label"])
            .build();
        text_box.append(&subtitle_label);
    }

    let icon = gtk::Image::builder()
        .icon_name(icon_name)
        .pixel_size(16)
        .valign(gtk::Align::Center)
        .build();
    icon.set_can_target(false);

    row_box.append(&text_box);
    row_box.append(&icon);
    row.set_child(Some(&row_box));

    let on_activate = on_click.clone();
    row.connect_activate(move |_| on_activate());

    let gesture = gtk::GestureClick::new();
    gesture.connect_released(move |_, _, _, _| {
        on_click();
    });
    row.add_controller(gesture);

    row
}

fn info_row(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::builder()
        .activatable(false)
        .selectable(false)
        .build();
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .margin_start(12)
        .margin_end(12)
        .margin_top(10)
        .margin_bottom(10)
        .build();
    row.set_child(Some(&label));
    row
}

enum TerminalLaunch {
    Opened,
    ClipboardFallback,
    FailedPrepare,
}

fn handle_install_in_terminal(
    script: &str,
    parent: &ApplicationWindow,
    toast: Option<&ToastOverlay>,
) {
    match launch_install_terminal(script) {
        TerminalLaunch::Opened => {
            post_toast(
                toast,
                &tr("Terminal opened. Enter your administrator password when prompted (sudo)."),
            );
        }
        TerminalLaunch::ClipboardFallback => {
            crate::util::copy_text_to_clipboard(script);
            let message = tr("Could not open a terminal , install script copied to clipboard. Run it in a root shell.");
            post_toast(toast, &message);
            show_install_notice(
                parent,
                &tr("Terminal could not be started"),
                &message,
            );
        }
        TerminalLaunch::FailedPrepare => {
            let message = tr("Could not prepare installation files. Check write access to /tmp.");
            post_toast(toast, &message);
            show_install_notice(parent, &tr("Installation not started"), &message);
        }
    }
}

fn show_install_notice(parent: &ApplicationWindow, heading: &str, body: &str) {
    let alert = AlertDialog::builder().heading(heading).body(body).build();
    alert.add_response("ok", &tr("OK"));
    alert.present(Some(parent));
}

fn launch_install_terminal(script: &str) -> TerminalLaunch {
    let Some(install_path) = write_temp_install_script(script) else {
        tracing::warn!("PBS install: could not write install script to temp dir");
        return TerminalLaunch::FailedPrepare;
    };
    let Some(wrapper_path) =
        write_terminal_wrapper_script(&install_path, &tr("Press Enter to close…"))
    else {
        tracing::warn!("PBS install: could not write terminal wrapper script");
        return TerminalLaunch::FailedPrepare;
    };

    let wrapper = match std::fs::canonicalize(&wrapper_path) {
        Ok(path) => path,
        Err(_) => wrapper_path,
    };
    let wrapper_arg = wrapper.display().to_string();

    tracing::info!(wrapper = %wrapper_arg, "PBS install: trying to open terminal");

    if spawn_terminal(&wrapper_arg) {
        gtk::glib::timeout_add_seconds_once(3, || {
            crate::window::refresh_dashboard_public();
        });
        return TerminalLaunch::Opened;
    }

    tracing::warn!("PBS install: no terminal emulator could be started");
    TerminalLaunch::ClipboardFallback
}

fn spawn_terminal(wrapper: &str) -> bool {
    let bash_wrapper = vec!["bash".to_string(), wrapper.to_string()];
    if is_flatpak_runtime() {
        let host_attempts: Vec<(&str, Vec<String>)> = vec![
            (
                "flatpak-spawn",
                std::iter::once("--host".to_string())
                    .chain(
                        ["xdg-terminal-exec", "--", "bash", wrapper]
                            .into_iter()
                            .map(str::to_string),
                    )
                    .collect(),
            ),
            (
                "flatpak-spawn",
                vec![
                    "--host".into(),
                    "gnome-terminal".into(),
                    "--".into(),
                    "bash".into(),
                    wrapper.into(),
                ],
            ),
            (
                "flatpak-spawn",
                vec![
                    "--host".into(),
                    "kgx".into(),
                    "-e".into(),
                    "bash".into(),
                    wrapper.into(),
                ],
            ),
        ];
        for (binary, args) in host_attempts {
            if spawn_command(binary, &args) {
                tracing::info!(terminal = binary, host = true, "PBS install: terminal started");
                return true;
            }
        }
    }
    let attempts: Vec<(&str, Vec<String>)> = vec![
        (
            "xdg-terminal-exec",
            std::iter::once("--".to_string())
                .chain(bash_wrapper.clone())
                .collect(),
        ),
        (
            "kgx",
            vec!["-e".into(), "bash".into(), wrapper.into()],
        ),
        (
            "gnome-terminal",
            vec!["--".into(), "bash".into(), wrapper.into()],
        ),
        (
            "ubuntu-terminal",
            vec!["--".into(), "bash".into(), wrapper.into()],
        ),
        ("ptyxis", vec!["--".into(), "bash".into(), wrapper.into()]),
        (
            "konsole",
            vec!["-e".into(), "bash".into(), wrapper.into()],
        ),
        (
            "xfce4-terminal",
            vec![
                "-e".into(),
                format!("bash {}", shell_escape_single(wrapper)),
            ],
        ),
        (
            "x-terminal-emulator",
            vec!["-e".into(), "bash".into(), wrapper.into()],
        ),
        ("xterm", vec!["-e".into(), "bash".into(), wrapper.into()]),
        (
            "alacritty",
            vec!["-e".into(), "bash".into(), wrapper.into()],
        ),
        ("tilix", vec!["-e".into(), "bash".into(), wrapper.into()]),
    ];

    for (binary, args) in attempts {
        if command_exists(binary) && spawn_command(binary, &args) {
            tracing::info!(terminal = binary, "PBS install: terminal started");
            return true;
        }
    }
    false
}

fn command_exists(binary: &str) -> bool {
    let path = Path::new(binary);
    if path.is_absolute() || binary.contains('/') {
        return path.is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file())
        })
        .unwrap_or(false)
}

fn spawn_command(binary: &str, args: &[String]) -> bool {
    let mut command = Command::new(binary);
    command.args(args.iter().map(String::as_str));

    for key in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_CURRENT_DESKTOP",
        "DESKTOP_SESSION",
        "TERM",
    ] {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }

    match command.spawn() {
        Ok(_) => true,
        Err(err) => {
            tracing::debug!(%err, %binary, ?args, "terminal spawn failed");
            false
        }
    }
}

fn temp_install_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("backuppilot")
}

fn write_temp_install_script(script: &str) -> Option<std::path::PathBuf> {
    let dir = temp_install_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("install-pbs-client.sh");
    std::fs::write(&path, script.as_bytes()).ok()?;
    make_executable(&path)?;
    Some(path)
}

/// Führt das Installationsskript aus und wartet auf Enter (Prompt sicher in der Datei, nicht in -lc).
fn write_terminal_wrapper_script(
    install_script: &Path,
    close_prompt: &str,
) -> Option<std::path::PathBuf> {
    let path = temp_install_dir().join("run-install-pbs-client.sh");
    let install = shell_escape_single(&install_script.display().to_string());
    let prompt = shell_escape_single(close_prompt);
    let result_file = install_result_path_for_shell().unwrap_or_else(|_| {
        shell_escape_single("/tmp/backuppilot-pbs-client-install-result.json")
    });
    let json_success = install_result_json(PbsInstallResultStatus::Success, None);
    let json_no_binary = install_result_json(
        PbsInstallResultStatus::Failed,
        Some(tr("proxmox-backup-client was not found after installation")),
    );
    let json_script_failed = install_result_json(
        PbsInstallResultStatus::Failed,
        Some(tr("Installation script failed")),
    );
    let body = format!(
        r#"#!/bin/bash
set -uo pipefail

RESULT_FILE={result_file}

write_install_result() {{
  mkdir -p "$(dirname "$RESULT_FILE")"
  printf '%s\n' "$1" >"$RESULT_FILE"
}}

echo "============================================================"
echo " {title}"
echo "============================================================"
echo
echo "{sudo_hint}"
echo "{starting}"
echo

if bash {install}; then
  echo
  echo "{done}"
  if command -v proxmox-backup-client >/dev/null 2>&1; then
    write_install_result {json_success}
  else
    write_install_result {json_no_binary}
  fi
else
  code=$?
  echo
  echo "{failed}"
  echo "(exit code: $code)"
  write_install_result {json_script_failed}
fi

echo
read -r -p {prompt} _
"#,
        title = tr("BackupPilot , installing Proxmox Backup Client"),
        sudo_hint = tr("Administrator rights are required. You will be prompted for your sudo password."),
        starting = tr("Starting installation…"),
        done = tr("Installation finished."),
        failed = tr("Installation failed. See the messages above."),
        install = install,
        prompt = prompt,
        result_file = result_file,
        json_success = shell_escape_single(&json_success),
        json_no_binary = shell_escape_single(&json_no_binary),
        json_script_failed = shell_escape_single(&json_script_failed),
    );
    std::fs::write(&path, body.as_bytes()).ok()?;
    make_executable(&path)?;
    Some(path)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Option<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).ok()?;
    Some(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Option<()> {
    Some(())
}

fn shell_escape_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn install_result_json(status: PbsInstallResultStatus, message: Option<String>) -> String {
    serde_json::to_string(&PbsInstallResult { status, message })
        .unwrap_or_else(|_| r#"{"status":"failed"}"#.to_string())
}


fn open_uri(parent: &ApplicationWindow, uri: &str) {
    let launcher = gtk::UriLauncher::new(uri);
    let parent = parent.clone();
    let uri = uri.to_string();
    launcher.launch(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            if let Err(err) = result {
                tracing::warn!(%err, uri = %uri, "failed to open PBS install documentation");
            }
        },
    );
}

fn manual_install_hint(os_id: &str) -> String {
    match os_id {
        "arch" | "manjaro" => tr(
            "Install from the AUR or use the static client binary from the Proxmox documentation.",
        ),
        _ => tr("See the Proxmox PBS installation guide for client-only repositories."),
    }
}

fn post_toast(overlay: Option<&ToastOverlay>, message: &str) {
    let Some(overlay) = overlay else {
        return;
    };
    let toast = Toast::new(message);
    toast.set_timeout(5);
    overlay.add_toast(toast);
}
