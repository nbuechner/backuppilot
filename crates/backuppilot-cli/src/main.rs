//! Host-side BackupPilot CLI — backups, snapshots, restore, and mounts via `backuppilot-core`.

mod i18n;
mod profile_ops;
mod restore_ops;
mod restore_wizard;
mod update_notice;
mod util;

use std::process::ExitCode;

use backuppilot_core::{
    apply_config_owner, backup_job, list_running_backups, profile_backup_in_progress, Database,
    ExecutionContext, PreflightOptions, RunStatus,
};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use profile_ops::{run_profile, ProfileCommand};
use restore_ops::{
    cmd_archives, cmd_files, cmd_mount, cmd_mounts, cmd_restore, cmd_snapshots, cmd_unmount,
};

#[derive(Parser)]
#[command(
    name = "backuppilot-cli",
    version,
    about = "BackupPilot host CLI (backups, snapshots, restore, mount)"
)]
struct Cli {
    /// Read profiles from another user's home (e.g. when running as root).
    #[arg(long, global = true)]
    user: Option<String>,

    /// Machine-readable JSON on stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Do not print an update hint on stderr before other output.
    #[arg(long, global = true)]
    no_update_notice: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List configured profiles.
    List,
    /// Create, import, export, and manage backup profiles (YAML).
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// List backups currently in progress.
    Running,
    /// Run a backup for a profile (name or numeric id).
    Backup {
        profile: String,
        /// Print preflight/backup steps without starting PBS.
        #[arg(long)]
        dry_run: bool,
    },
    /// List PBS snapshots for a profile.
    Snapshots {
        profile: String,
    },
    /// List archive names in a snapshot.
    Archives {
        profile: String,
        /// Full snapshot path from `snapshots`.
        snapshot: String,
    },
    /// List files and folders inside an archive (catalog).
    Files {
        profile: String,
        snapshot: String,
        archive: String,
        /// Directory inside the archive (empty = root).
        #[arg(long, default_value = "")]
        path: String,
        /// Bypass the in-memory catalog cache.
        #[arg(long)]
        refresh: bool,
    },
    /// Guided wizard: profile → snapshot → restore or mount.
    Restore,
    /// Restore with all parameters on the command line (scripts, cron).
    #[command(name = "restore-run")]
    RestoreRun {
        profile: String,
        snapshot: String,
        archive: String,
        target: String,
        #[arg(long = "pattern")]
        patterns: Vec<String>,
        #[arg(long)]
        overwrite: bool,
    },
    /// List active FUSE mounts started by this CLI.
    Mounts,
    /// Guided mount wizard (profile → snapshot → archive).
    Mount,
    /// Mount with all parameters on the command line.
    #[command(name = "mount-run")]
    MountRun {
        profile: String,
        snapshot: String,
        archive: String,
        #[arg(long)]
        label: Option<String>,
    },
    /// Guided unmount wizard for active CLI mounts.
    Unmount,
    /// Unmount with mount id or --all.
    #[command(name = "unmount-run")]
    UnmountRun {
        mount_id: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Check GitLab for a newer BackupPilot release (same source as the GUI).
    #[command(name = "check-update")]
    CheckUpdate,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let cli = Cli::parse();

    if let Some(user) = &cli.user {
        if let Err(err) = apply_config_owner(user) {
            emit_error(&cli, &err);
            return ExitCode::from(2);
        }
    }

    i18n::init_from_app_settings();

    if !matches!(cli.command, Commands::CheckUpdate) {
        update_notice::maybe_print_update_notice(&cli).await;
    }

    match cli.command {
        Commands::List => cmd_list(&cli).await,
        Commands::Profile { ref command } => run_profile(&cli, command.clone()).await,
        Commands::Running => cmd_running(&cli).await,
        Commands::Backup {
            ref profile,
            dry_run,
        } => cmd_backup(&cli, profile, dry_run).await,
        Commands::Snapshots { ref profile } => cmd_snapshots(&cli, profile).await,
        Commands::Archives {
            ref profile,
            ref snapshot,
        } => cmd_archives(&cli, profile, snapshot).await,
        Commands::Files {
            ref profile,
            ref snapshot,
            ref archive,
            ref path,
            refresh,
        } => cmd_files(&cli, profile, snapshot, archive, path, refresh).await,
        Commands::Restore => restore_wizard::run(&cli).await,
        Commands::RestoreRun {
            ref profile,
            ref snapshot,
            ref archive,
            ref target,
            ref patterns,
            overwrite,
        } => {
            cmd_restore(&cli, profile, snapshot, archive, target, patterns, overwrite).await
        }
        Commands::Mounts => cmd_mounts(&cli).await,
        Commands::Mount => restore_wizard::run_mount(&cli).await,
        Commands::MountRun {
            ref profile,
            ref snapshot,
            ref archive,
            ref label,
        } => cmd_mount(&cli, profile, snapshot, archive, label.as_deref()).await,
        Commands::Unmount => restore_wizard::run_unmount(&cli).await,
        Commands::UnmountRun {
            ref mount_id,
            all,
        } => cmd_unmount(&cli, mount_id.as_deref(), all).await,
        Commands::CheckUpdate => update_notice::cmd_check_update(&cli).await,
    }
}

async fn cmd_list(cli: &Cli) -> ExitCode {
    let db = match Database::open() {
        Ok(db) => db,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    let profiles = match db.list_profiles() {
        Ok(p) => p,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };

    let running_ids: std::collections::HashSet<i64> = list_running_backups(&db)
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.profile_id)
        .collect();

    if cli.json {
        let rows: Vec<_> = profiles
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "id": p.id,
                    "execution": match p.conditions.execution_context {
                        ExecutionContext::HostCli => "host-cli",
                        ExecutionContext::Desktop => "desktop",
                    },
                    "enabled": p.enabled,
                    "backup_in_progress": running_ids.contains(&p.id),
                })
            })
            .collect();
        println!("{}", serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into()));
    } else {
        for p in profiles {
            let mode = match p.conditions.execution_context {
                ExecutionContext::HostCli => "host-cli",
                ExecutionContext::Desktop => "desktop",
            };
            let state = if running_ids.contains(&p.id) {
                "running"
            } else {
                "idle"
            };
            println!("{}\t{}\t{}\t{}\t{state}", p.name, p.id, mode, p.enabled);
        }
    }
    ExitCode::SUCCESS
}

async fn cmd_running(cli: &Cli) -> ExitCode {
    let db = match Database::open() {
        Ok(db) => db,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    let running = match list_running_backups(&db) {
        Ok(r) => r,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string(&running).unwrap_or_else(|_| "[]".into())
        );
    } else if running.is_empty() {
        println!("{}", i18n::tr("No backups are currently running."));
    } else {
        for job in running {
            println!(
                "{}\t{}\t{}\t{}",
                job.profile_name,
                job.profile_id,
                job.run_id,
                job.started_at.to_rfc3339()
            );
        }
    }
    ExitCode::SUCCESS
}

async fn cmd_backup(cli: &Cli, profile: &str, dry_run: bool) -> ExitCode {
    let db = match Database::open() {
        Ok(db) => db,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    let profile_id = match backup_job::resolve_profile_id(&db, profile) {
        Ok(id) => id,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };

    if dry_run {
        let name = db.get_profile(profile_id).map(|p| p.name).unwrap_or_default();
        let blocked = profile_backup_in_progress(&db, profile_id).unwrap_or(false);
        let msg = if blocked {
            format!("would not start backup for profile {name} (id {profile_id}): already running")
        } else {
            format!("would run backup for profile {name} (id {profile_id})")
        };
        if cli.json {
            println!(
                "{}",
                serde_json::json!({
                    "dry_run": true,
                    "profile_id": profile_id,
                    "already_running": blocked,
                    "message": msg,
                })
            );
        } else {
            println!("{msg}");
        }
        return ExitCode::SUCCESS;
    }

    if profile_backup_in_progress(&db, profile_id).unwrap_or(false) {
        let name = db.get_profile(profile_id).map(|p| p.name).unwrap_or_default();
        let msg = format!("backup already running for profile {name} (id {profile_id})");
        emit_error(cli, &msg);
        return ExitCode::from(1);
    }

    let outcome = match backup_job::execute_profile_backup(profile_id, PreflightOptions::default()).await
    {
        Ok(o) => o,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };

    if outcome.already_running {
        if let Some(msg) = &outcome.message {
            emit_error(cli, msg);
        }
        return ExitCode::from(1);
    }

    let success = matches!(outcome.status, RunStatus::Success)
        || matches!(outcome.status, RunStatus::Skipped);
    if cli.json {
        println!(
            "{}",
            serde_json::json!({
                "success": success && outcome.status == RunStatus::Success,
                "run_id": outcome.run_id,
                "status": outcome.status,
                "bytes_uploaded": outcome.bytes_uploaded,
                "snapshot_id": outcome.snapshot_id,
                "message": outcome.message,
                "already_running": false,
            })
        );
    } else if outcome.status != RunStatus::Success {
        if let Some(msg) = &outcome.message {
            eprintln!("{msg}");
        }
    }

    if outcome.status == RunStatus::Success {
        ExitCode::SUCCESS
    } else if outcome.status == RunStatus::Skipped {
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

pub(crate) fn emit_error(cli: &Cli, message: &str) {
    if cli.json {
        println!(
            r#"{{"success":false,"message":{}}}"#,
            serde_json::to_string(message).unwrap_or_default()
        );
    } else {
        eprintln!("{message}");
    }
}
