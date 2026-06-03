//! Optional update hint on stderr (shared GitLab check with GUI/daemon).

use std::process::ExitCode;
use std::time::Duration;

use backuppilot_core::app_settings::load_app_settings;
use backuppilot_core::paths::is_flatpak_runtime;
use backuppilot_core::updates::{
    can_install_update_packages, check_for_updates, installed_version, is_update_newer_than_installed,
    load_update_state, should_notify_user, should_run_automatic_check, UpdateAvailability,
    UpdateCheckOutcome,
};

use crate::i18n::{tr, tr_fmt};
use crate::Cli;

const CHECK_TIMEOUT: Duration = Duration::from_secs(8);

/// Print a short notice when a newer release is known (does not install anything).
pub async fn maybe_print_update_notice(cli: &Cli) {
    if cli.no_update_notice || cli.json {
        return;
    }
    if std::env::var_os("BACKUPPILOT_NO_UPDATE_NOTICE").is_some() {
        return;
    }

    let settings = load_app_settings();
    let mut state = load_update_state();

    if settings.updates.check_automatically && should_run_automatic_check(&state) {
        let channel = settings.updates.channel;
        let _ = tokio::time::timeout(CHECK_TIMEOUT, check_for_updates(channel)).await;
        state = load_update_state();
    }

    let Some(avail) = state.available.as_ref() else {
        return;
    };
    if !is_update_newer_than_installed(avail) || !should_notify_user(&state, avail) {
        return;
    }

    for line in format_notice_lines(avail) {
        eprintln!("{line}");
    }
}

/// Explicit `backuppilot-cli check-update` — always queries GitLab.
pub async fn cmd_check_update(cli: &Cli) -> ExitCode {
    let settings = load_app_settings();
    let outcome = match tokio::time::timeout(
        CHECK_TIMEOUT,
        check_for_updates(settings.updates.channel),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            emit_check_error(cli, &tr("Update check timed out."));
            return ExitCode::from(2);
        }
    };

    match outcome {
        UpdateCheckOutcome::UpToDate => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "up_to_date": true,
                        "installed_version": installed_version(),
                    })
                );
            } else {
                println!(
                    "{}",
                    tr_fmt(
                        "You are running the latest version ({version}).",
                        &[("version", installed_version())],
                    )
                );
            }
            ExitCode::SUCCESS
        }
        UpdateCheckOutcome::UpdateAvailable { availability: avail } => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "up_to_date": false,
                        "installed_version": installed_version(),
                        "available_version": avail.version,
                        "release_url": avail.release_url,
                        "tag": avail.tag,
                    })
                );
            } else {
                for line in format_notice_lines(&avail) {
                    println!("{line}");
                }
            }
            // 0 = information only; scripts can parse JSON or stdout.
            ExitCode::SUCCESS
        }
        UpdateCheckOutcome::Error { message } => {
            emit_check_error(cli, &message);
            ExitCode::from(2)
        }
    }
}

fn emit_check_error(cli: &Cli, detail: &str) {
    if cli.json {
        println!(
            "{}",
            serde_json::json!({ "error": detail })
        );
    } else {
        eprintln!(
            "{}",
            tr_fmt("Update check failed: {detail}", &[("detail", detail)])
        );
    }
}

fn format_notice_lines(avail: &UpdateAvailability) -> Vec<String> {
    let prefix = tr("backuppilot-cli");
    let headline = tr_fmt(
        "Version {version} is available (installed: {installed}).",
        &[
            ("version", &avail.version),
            ("installed", installed_version()),
        ],
    );

    let mut lines = vec![format!("{prefix}: {headline}")];

    if is_flatpak_runtime() {
        lines.push(tr_fmt(
            "Open the release page to download the Flatpak bundle: {url}",
            &[("url", &avail.release_url)],
        ));
    } else if can_install_update_packages() {
        lines.push(tr_fmt(
            "Install via BackupPilot (About) or download: {url}",
            &[("url", &avail.release_url)],
        ));
    } else {
        lines.push(tr_fmt(
            "See the release page: {url}",
            &[("url", &avail.release_url)],
        ));
    }

    lines
}
