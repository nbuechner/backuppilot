//! Snapshot listing, catalog browse, restore, and FUSE mount commands.

use std::process::ExitCode;

use backuppilot_core::{
    ListCatalogRequest, MountSnapshotRequest, PbsRestore, PbsSnapshotInfo, RestoreArchiveRequest,
};

use crate::i18n::{tr, tr_fmt};
use crate::util::{mount_manager, open_db, profile_by_name_or_id, snapshot_encryption_key};
use crate::Cli;

pub async fn cmd_snapshots(cli: &Cli, profile: &str) -> ExitCode {
    let db = match open_db() {
        Ok(db) => db,
        Err(err) => return fail(cli, &err, 2),
    };
    let profile = match profile_by_name_or_id(&db, profile).await {
        Ok(p) => p,
        Err(err) => return fail(cli, &err, 2),
    };

    let snapshots = match PbsRestore::list_snapshots(&profile).await {
        Ok(s) => s,
        Err(err) => return fail(cli, &err.to_string(), 2),
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string(&snapshots).unwrap_or_else(|_| "[]".into())
        );
    } else if snapshots.is_empty() {
        println!(
            "{}",
            tr_fmt(
                "No snapshots found for profile {name}.",
                &[("name", &profile.name)]
            )
        );
    } else {
        for s in &snapshots {
            print_snapshot_line(&s);
        }
    }
    ExitCode::SUCCESS
}

pub async fn cmd_archives(cli: &Cli, profile: &str, snapshot: &str) -> ExitCode {
    let db = match open_db() {
        Ok(db) => db,
        Err(err) => return fail(cli, &err, 2),
    };
    let profile = match profile_by_name_or_id(&db, profile).await {
        Ok(p) => p,
        Err(err) => return fail(cli, &err, 2),
    };

    let archives = match archives_for_snapshot(&profile, snapshot).await {
        Ok(a) => a,
        Err(err) => return fail(cli, &err, 2),
    };

    if cli.json {
        println!(
            "{}",
            serde_json::json!({
                "profile": profile.name,
                "profile_id": profile.id,
                "snapshot": snapshot,
                "archives": archives,
            })
        );
    } else if archives.is_empty() {
        println!(
            "{}",
            tr_fmt(
                "No archives in snapshot {snapshot}.",
                &[("snapshot", snapshot)]
            )
        );
    } else {
        for name in &archives {
            println!("{name}");
        }
    }
    ExitCode::SUCCESS
}

pub async fn cmd_files(
    cli: &Cli,
    profile: &str,
    snapshot: &str,
    archive: &str,
    parent_path: &str,
    refresh: bool,
) -> ExitCode {
    let db = match open_db() {
        Ok(db) => db,
        Err(err) => return fail(cli, &err, 2),
    };
    let profile = match profile_by_name_or_id(&db, profile).await {
        Ok(p) => p,
        Err(err) => return fail(cli, &err, 2),
    };
    let key_id = match snapshot_encryption_key(&db, &profile, snapshot).await {
        Ok(k) => k,
        Err(err) => return fail(cli, &err, 2),
    };

    let request = ListCatalogRequest {
        profile_id: profile.id,
        snapshot: snapshot.to_string(),
        archive_name: archive.to_string(),
        parent_path: parent_path.to_string(),
        force_refresh: refresh,
        encryption_key_id: key_id,
    };

    let response = match PbsRestore::list_catalog(&request, &profile).await {
        Ok(r) => r,
        Err(err) => return fail(cli, &err.to_string(), 2),
    };

    if cli.json {
        println!("{}", serde_json::to_string(&response).unwrap_or_else(|_| "{}".into()));
    } else if response.entries.is_empty() {
        let path = if parent_path.is_empty() {
            "/"
        } else {
            parent_path
        };
        println!(
            "{}",
            tr_fmt(
                "No entries under '{path}' in {snapshot}/{archive}.",
                &[
                    ("path", path),
                    ("snapshot", snapshot),
                    ("archive", archive),
                ],
            )
        );
    } else {
        for entry in &response.entries {
            let kind = if entry.is_dir {
                tr("dir")
            } else {
                tr("file")
            };
            println!("{}\t{kind}\t{}", entry.path, entry.name);
        }
    }
    ExitCode::SUCCESS
}

pub async fn cmd_restore(
    cli: &Cli,
    profile: &str,
    snapshot: &str,
    archive: &str,
    target: &str,
    patterns: &[String],
    overwrite: bool,
) -> ExitCode {
    let db = match open_db() {
        Ok(db) => db,
        Err(err) => return fail(cli, &err, 2),
    };
    let profile = match profile_by_name_or_id(&db, profile).await {
        Ok(p) => p,
        Err(err) => return fail(cli, &err, 2),
    };
    let key_id = match snapshot_encryption_key(&db, &profile, snapshot).await {
        Ok(k) => k,
        Err(err) => return fail(cli, &err, 2),
    };

    let request = RestoreArchiveRequest {
        profile_id: profile.id,
        snapshot: snapshot.to_string(),
        archive_name: archive.to_string(),
        target_dir: target.to_string(),
        overwrite,
        patterns: patterns.to_vec(),
        encryption_key_id: key_id,
    };

    execute_restore(cli, &profile, &request).await
}

pub async fn execute_restore(
    cli: &Cli,
    profile: &backuppilot_core::BackupProfile,
    request: &RestoreArchiveRequest,
) -> ExitCode {
    let target = request.target_dir.clone();
    let result = match PbsRestore::execute_restore(profile, request).await {
        Ok(r) => r,
        Err(err) => return fail(cli, &err.to_string(), 2),
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string(&result).unwrap_or_else(|_| "{}".into())
        );
    } else if result.ok {
        println!(
            "{}",
            tr_fmt("Restore completed: {target}", &[("target", &target)])
        );
    } else if let Some(msg) = &result.message {
        eprintln!("{msg}");
        if !result.conflicts.is_empty() {
            for c in &result.conflicts {
                eprintln!(
                    "  {}: {c}",
                    tr("Conflict")
                );
            }
        }
    }

    if result.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

pub async fn cmd_mounts(cli: &Cli) -> ExitCode {
    let mounts = mount_manager().list_active().await;
    let db = open_db().ok();

    if cli.json {
        println!(
            "{}",
            serde_json::to_string(&mounts).unwrap_or_else(|_| "[]".into())
        );
    } else if mounts.is_empty() {
        println!("{}", tr("No active mounts (including from earlier CLI runs)."));
    } else {
        for m in &mounts {
            let profile_name = db
                .as_ref()
                .and_then(|db| db.get_profile(m.profile_id).ok())
                .map(|p| p.name)
                .unwrap_or_else(|| m.profile_name.clone());
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                m.id,
                profile_name,
                m.snapshot,
                m.archive_name,
                m.mount_point,
                m.started_at.to_rfc3339()
            );
        }
    }
    ExitCode::SUCCESS
}

pub async fn cmd_mount(
    cli: &Cli,
    profile: &str,
    snapshot: &str,
    archive: &str,
    label: Option<&str>,
) -> ExitCode {
    let db = match open_db() {
        Ok(db) => db,
        Err(err) => return fail(cli, &err, 2),
    };
    let profile = match profile_by_name_or_id(&db, profile).await {
        Ok(p) => p,
        Err(err) => return fail(cli, &err, 2),
    };
    let key_id = match snapshot_encryption_key(&db, &profile, snapshot).await {
        Ok(k) => k,
        Err(err) => return fail(cli, &err, 2),
    };

    let request = MountSnapshotRequest {
        profile_id: profile.id,
        snapshot: snapshot.to_string(),
        archive_name: archive.to_string(),
        source_label: label.unwrap_or(archive).to_string(),
        encryption_key_id: key_id,
    };

    execute_mount(cli, &profile, &request, key_id).await
}

pub async fn execute_mount(
    cli: &Cli,
    profile: &backuppilot_core::BackupProfile,
    request: &MountSnapshotRequest,
    key_id: Option<i64>,
) -> ExitCode {
    let result = match mount_manager()
        .mount(profile, request, key_id)
        .await
    {
        Ok(r) => r,
        Err(err) => return fail(cli, &err.to_string(), 2),
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string(&result).unwrap_or_else(|_| "{}".into())
        );
    } else if result.ok {
        if let Some(m) = &result.mount {
            println!(
                "{}",
                tr_fmt("Mounted at: {path}", &[("path", &m.mount_point)])
            );
            println!(
                "{}",
                tr_fmt("Mount ID: {id}", &[("id", &m.id)])
            );
        }
    } else if let Some(msg) = &result.message {
        eprintln!("{msg}");
    }

    if result.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

pub async fn cmd_unmount(cli: &Cli, mount_id: Option<&str>, all: bool) -> ExitCode {
    if all {
        match mount_manager().unmount_all().await {
            Ok(()) => {
                if cli.json {
                    println!(r#"{{"ok":true,"message":"all mounts disconnected"}}"#);
                } else {
                    println!("{}", tr("All mounts were disconnected."));
                }
                return ExitCode::SUCCESS;
            }
            Err(err) => return fail(cli, &err.to_string(), 1),
        }
    }

    let Some(mount_id) = mount_id else {
        return fail(
            cli,
            &tr("Provide a mount id (from `mounts`) or use --all."),
            2,
        );
    };

    let result = match mount_manager().unmount(mount_id).await {
        Ok(r) => r,
        Err(err) => return fail(cli, &err.to_string(), 2),
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string(&result).unwrap_or_else(|_| "{}".into())
        );
    } else if result.ok {
        println!(
            "{}",
            tr_fmt("Mount disconnected: {id}", &[("id", mount_id)])
        );
    } else if let Some(msg) = &result.message {
        eprintln!("{msg}");
    }

    if result.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

pub(crate) async fn archives_for_snapshot(
    profile: &backuppilot_core::BackupProfile,
    snapshot: &str,
) -> Result<Vec<String>, String> {
    backuppilot_core::list_archives_for_snapshot(profile, snapshot)
        .await
        .map_err(|e| e.to_string())
}

fn print_snapshot_line(s: &PbsSnapshotInfo) {
    let enc = if s.encrypted {
        tr("encrypted")
    } else {
        tr("plain")
    };
    let arch = s.archives.join(",");
    println!(
        "{}\t{}\t{enc}\t{arch}",
        s.path,
        s.size_bytes,
    );
}

fn fail(cli: &Cli, message: &str, code: u8) -> ExitCode {
    crate::emit_error(cli, message);
    ExitCode::from(code)
}
