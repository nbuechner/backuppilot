//! Guided interactive restore / mount / unmount (`backuppilot-cli restore|mount|unmount`).

use std::collections::HashSet;
use std::io::IsTerminal;
use std::process::ExitCode;

use backuppilot_core::{
    archive_source_root, fuse_available, list_archives_for_snapshot, original_restore_target_dir,
    pbs_mount::mount_point_for, BackupProfile, CatalogEntry, ListCatalogRequest,
    MountSnapshotRequest, PbsRestore, PbsSnapshotInfo, RestoreArchiveRequest,
};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

use crate::i18n::{tr, tr_fmt};
use crate::restore_ops::{execute_mount, execute_restore};
use crate::util::{mount_manager, open_db, profile_by_name_or_id, snapshot_encryption_key};
use crate::Cli;

struct PickContext {
    profile: BackupProfile,
    snapshot: String,
    archive: String,
    key_id: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WizardMode {
    Full,
    MountOnly,
}

#[derive(Clone)]
enum PickAction {
    Finish,
    GoUp,
    EnterDir(String),
    AddDir(String),
    RemoveDir(String),
    AddFile(String),
    RemoveFile(String),
}

#[derive(Clone)]
enum BrowseAction {
    Quit,
    GoUp,
    EnterDir(String),
    Ignore,
}

pub async fn run(cli: &Cli) -> ExitCode {
    run_with_mode(cli, WizardMode::Full).await
}

pub async fn run_mount(cli: &Cli) -> ExitCode {
    run_with_mode(cli, WizardMode::MountOnly).await
}

pub async fn run_unmount(cli: &Cli) -> ExitCode {
    if let Err(msg) = require_tty(cli) {
        return fail(cli, &msg, 2);
    }

    let mounts = mount_manager().list_active().await;
    if mounts.is_empty() {
        println!(
            "{}",
            tr("No active FUSE mounts under ~/.local/share/backuppilot/mounts/.")
        );
        println!(
            "{}",
            tr("GUI mounts are managed by the daemon (see the app for an overview).")
        );
        return ExitCode::SUCCESS;
    }

    let db = open_db().ok();
    let mut labels = vec![tr_fmt(
        "Disconnect all {count} mounts",
        &[("count", &mounts.len().to_string())],
    )];
    for m in &mounts {
        let profile_name = db
            .as_ref()
            .and_then(|db| db.get_profile(m.profile_id).ok())
            .map(|p| p.name)
            .unwrap_or_else(|| m.profile_name.clone());
        labels.push(format!(
            "{profile_name} — {} @ {}",
            m.archive_name, m.mount_point
        ));
    }
    let item_refs: Vec<&str> = labels.iter().map(String::as_str).collect();

    let choice = match Select::with_theme(&theme())
        .with_prompt(&tr("Disconnect mount"))
        .items(&item_refs)
        .default(0)
        .interact()
    {
        Ok(i) => i,
        Err(_) => return ExitCode::SUCCESS,
    };

    if choice == 0 {
        return crate::restore_ops::cmd_unmount(cli, None, true).await;
    }

    let mount = &mounts[choice - 1];
    crate::restore_ops::cmd_unmount(cli, Some(&mount.id), false).await
}

async fn run_with_mode(cli: &Cli, mode: WizardMode) -> ExitCode {
    if let Err(msg) = require_tty(cli) {
        return fail(cli, &msg, 2);
    }

    let ctx = match pick_profile_snapshot_archive(cli).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    match mode {
        WizardMode::MountOnly => do_mount(cli, &ctx).await,
        WizardMode::Full => {
            let action_items = vec![
                tr("Restore files to disk"),
                tr("Mount archive read-only (file manager)"),
                tr("Browse files only"),
                tr("Cancel"),
            ];
            let action_refs: Vec<&str> = action_items.iter().map(String::as_str).collect();
            let action = match Select::with_theme(&theme())
                .with_prompt(&tr("What would you like to do?"))
                .items(&action_refs)
                .default(0)
                .interact()
            {
                Ok(i) => i,
                Err(_) => return ExitCode::SUCCESS,
            };

            match action {
                0 => {
                    run_restore_branch(cli, &ctx.profile, &ctx.snapshot, &ctx.archive, ctx.key_id)
                        .await
                }
                1 => do_mount(cli, &ctx).await,
                2 => {
                    if let Err(err) =
                        browse_archive(&ctx.profile, &ctx.snapshot, &ctx.archive, ctx.key_id).await
                    {
                        eprintln!("{err}");
                        ExitCode::from(1)
                    } else {
                        ExitCode::SUCCESS
                    }
                }
                _ => ExitCode::SUCCESS,
            }
        }
    }
}

async fn pick_profile_snapshot_archive(cli: &Cli) -> Result<PickContext, ExitCode> {
    let db = match open_db() {
        Ok(db) => db,
        Err(err) => return Err(fail(cli, &err, 2)),
    };

    let profiles = match db.list_profiles() {
        Ok(p) => p,
        Err(err) => return Err(fail(cli, &err.to_string(), 2)),
    };
    if profiles.is_empty() {
        return Err(fail(cli, &tr("No profiles configured."), 1));
    }

    let profile_labels: Vec<String> = profiles
        .iter()
        .map(|p| format!("{} (id {})", p.name, p.id))
        .collect();
    let profile_refs: Vec<&str> = profile_labels.iter().map(String::as_str).collect();
    let profile_idx = match Select::with_theme(&theme())
        .with_prompt(&tr("Choose profile"))
        .items(&profile_refs)
        .default(0)
        .interact()
    {
        Ok(i) => i,
        Err(_) => return Err(ExitCode::SUCCESS),
    };
    let profile = match profile_by_name_or_id(&db, &profiles[profile_idx].name).await {
        Ok(p) => p,
        Err(err) => return Err(fail(cli, &err, 2)),
    };

    println!("\n{}", tr("Loading snapshots from PBS …"));
    let mut snapshots = match PbsRestore::list_snapshots(&profile).await {
        Ok(s) => s,
        Err(err) => return Err(fail(cli, &err.to_string(), 2)),
    };
    if snapshots.is_empty() {
        return Err(fail(
            cli,
            &tr_fmt(
                "No snapshots for profile {name}.",
                &[("name", &profile.name)],
            ),
            1,
        ));
    }
    snapshots.sort_by(|a, b| b.path.cmp(&a.path));

    let snap_labels: Vec<String> = snapshots.iter().map(snapshot_menu_label).collect();
    let snap_refs: Vec<&str> = snap_labels.iter().map(String::as_str).collect();
    let snap_idx = match Select::with_theme(&theme())
        .with_prompt(&tr("Choose snapshot"))
        .items(&snap_refs)
        .default(0)
        .interact()
    {
        Ok(i) => i,
        Err(_) => return Err(ExitCode::SUCCESS),
    };
    let snapshot = snapshots[snap_idx].path.clone();

    let key_id = match snapshot_encryption_key(&db, &profile, &snapshot).await {
        Ok(k) => k,
        Err(err) => return Err(fail(cli, &err, 2)),
    };

    println!("\n{}", tr("Resolving archives …"));
    let archives = match list_archives_for_snapshot(&profile, &snapshot).await {
        Ok(a) => a,
        Err(err) => return Err(fail(cli, &err.to_string(), 2)),
    };
    if archives.is_empty() {
        return Err(fail(
            cli,
            &tr("No archives found — check profile paths in the app or load the PBS manifest."),
            1,
        ));
    }

    let archive = pick_archive(&profile, &archives)?;

    Ok(PickContext {
        profile,
        snapshot,
        archive,
        key_id,
    })
}

fn pick_archive(profile: &BackupProfile, archives: &[String]) -> Result<String, ExitCode> {
    if archives.len() == 1 {
        return Ok(archives[0].clone());
    }
    let labels = archive_menu_labels(profile, archives);
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let arch_idx = match Select::with_theme(&theme())
        .with_prompt(&tr("Choose archive / backup path"))
        .items(&label_refs)
        .default(0)
        .interact()
    {
        Ok(i) => i,
        Err(_) => return Err(ExitCode::SUCCESS),
    };
    Ok(archives[arch_idx].clone())
}

fn archive_menu_labels(profile: &BackupProfile, archives: &[String]) -> Vec<String> {
    archives
        .iter()
        .map(|archive| {
            archive_source_root(&profile.paths, archive)
                .map(|p| format!("{}  [{archive}]", p.display()))
                .unwrap_or_else(|| archive.clone())
        })
        .collect()
}

fn archive_source_label(profile: &BackupProfile, archive: &str) -> String {
    archive_source_root(&profile.paths, archive)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| archive.to_string())
}

async fn do_mount(cli: &Cli, ctx: &PickContext) -> ExitCode {
    if !fuse_available() {
        return fail(
            cli,
            &tr("FUSE is not available (install the fuse3 package)."),
            1,
        );
    }
    let ok = Confirm::with_theme(&theme())
        .with_prompt(&tr(
            "Mount archive read-only? (network access to PBS; only trusted backups)",
        ))
        .default(true)
        .interact()
        .unwrap_or(false);
    if !ok {
        return ExitCode::SUCCESS;
    }

    let label = archive_source_label(&ctx.profile, &ctx.archive);
    let request = MountSnapshotRequest {
        profile_id: ctx.profile.id,
        snapshot: ctx.snapshot.clone(),
        archive_name: ctx.archive.clone(),
        source_label: label,
        encryption_key_id: ctx.key_id,
    };
    let code = execute_mount(cli, &ctx.profile, &request, ctx.key_id).await;
    if code == ExitCode::from(0)
        && Confirm::with_theme(&theme())
            .with_prompt(&tr("Open in file manager?"))
            .default(true)
            .interact()
            .unwrap_or(false)
    {
        if let Some(m) = mount_point_from_request(ctx.profile.id, &ctx.snapshot, &ctx.archive) {
            let _ = std::process::Command::new("xdg-open")
                .arg(m)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }
    code
}

fn require_tty(cli: &Cli) -> Result<(), String> {
    if cli.json {
        return Err(tr("Guided mode does not support --json."));
    }
    if !std::io::stdin().is_terminal() {
        return Err(tr("Guided mode requires an interactive terminal."));
    }
    Ok(())
}

async fn run_restore_branch(
    cli: &Cli,
    profile: &backuppilot_core::BackupProfile,
    snapshot: &str,
    archive: &str,
    key_id: Option<i64>,
) -> ExitCode {
    let scope_items = vec![
        tr("Entire archive"),
        tr("Select specific files or folders"),
        tr("Enter glob patterns (e.g. Documents/**)"),
    ];
    let scope_refs: Vec<&str> = scope_items.iter().map(String::as_str).collect();
    let scope = match Select::with_theme(&theme())
        .with_prompt(&tr("Restore scope"))
        .items(&scope_refs)
        .default(0)
        .interact()
    {
        Ok(i) => i,
        Err(_) => return ExitCode::SUCCESS,
    };

    let patterns = match scope {
        0 => Vec::new(),
        1 => match pick_paths_interactive(profile, snapshot, archive, key_id).await {
            Ok(p) => p,
            Err(err) => return fail(cli, &err, 1),
        },
        2 => {
            let raw: String = Input::with_theme(&theme())
                .with_prompt(&tr("Glob patterns (comma-separated for multiple)"))
                .interact_text()
                .unwrap_or_default();
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
        _ => Vec::new(),
    };

    let (target, overwrite) = match pick_restore_target(cli, profile, archive, &patterns) {
        Ok(t) => t,
        Err(code) => return code,
    };

    let request = RestoreArchiveRequest {
        profile_id: profile.id,
        snapshot: snapshot.to_string(),
        archive_name: archive.to_string(),
        target_dir: target.clone(),
        overwrite,
        patterns,
        encryption_key_id: key_id,
    };

    println!("\n{}", tr("Restore in progress (may take a long time) …"));
    execute_restore(cli, profile, &request).await
}

async fn pick_paths_interactive(
    profile: &backuppilot_core::BackupProfile,
    snapshot: &str,
    archive: &str,
    key_id: Option<i64>,
) -> Result<Vec<String>, String> {
    let mut parent_path = String::new();
    let mut selected: HashSet<String> = HashSet::new();

    println!();
    println!("{}", tr("Select files or folders with Enter. Then choose Done."));
    println!(
        "{}",
        tr("No space bar needed — each entry runs an action immediately.")
    );
    println!();

    loop {
        let entries = load_catalog_entries(profile, snapshot, archive, &parent_path, key_id).await?;
        let mut dirs: Vec<&CatalogEntry> = entries.iter().filter(|e| e.is_dir).collect();
        let mut files: Vec<&CatalogEntry> = entries.iter().filter(|e| !e.is_dir).collect();
        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        files.sort_by(|a, b| a.name.cmp(&b.name));

        let (labels, actions) = build_pick_menu(&parent_path, &dirs, &files, &selected);
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();

        let location = if parent_path.is_empty() {
            tr("Archive root")
        } else {
            format!("/{parent_path}")
        };
        let prompt = tr_fmt(
            "{location} — selected: {count}",
            &[
                ("location", &location),
                ("count", &selected.len().to_string()),
            ],
        );

        let choice = Select::with_theme(&theme())
            .with_prompt(prompt)
            .items(&label_refs)
            .default(0)
            .interact()
            .map_err(|e| e.to_string())?;

        match &actions[choice] {
            PickAction::Finish => {
                if selected.is_empty() {
                    println!("{}\n", tr("Please select at least one file or folder."));
                    continue;
                }
                break;
            }
            PickAction::GoUp => {
                parent_path = parent_path_of(&parent_path);
            }
            PickAction::EnterDir(name) => {
                parent_path = join_catalog_path(&parent_path, name);
            }
            PickAction::AddDir(name) => {
                if let Some(entry) = dirs.iter().find(|e| e.name == *name) {
                    selected.insert(dir_pattern(&entry.path));
                }
            }
            PickAction::RemoveDir(name) => {
                if let Some(entry) = dirs.iter().find(|e| e.name == *name) {
                    selected.remove(&dir_pattern(&entry.path));
                }
            }
            PickAction::AddFile(name) => {
                if let Some(entry) = files.iter().find(|e| e.name == *name) {
                    selected.insert(entry.path.clone());
                }
            }
            PickAction::RemoveFile(name) => {
                if let Some(entry) = files.iter().find(|e| e.name == *name) {
                    selected.remove(&entry.path);
                }
            }
        }
    }

    Ok(selected.into_iter().collect())
}

fn build_pick_menu(
    parent_path: &str,
    dirs: &[&CatalogEntry],
    files: &[&CatalogEntry],
    selected: &HashSet<String>,
) -> (Vec<String>, Vec<PickAction>) {
    let mut labels = Vec::new();
    let mut actions = Vec::new();

    let finish = if selected.is_empty() {
        tr("► Done — choose target folder (nothing selected yet)")
    } else {
        tr_fmt(
            "► Done — restore {count} item(s), choose target folder",
            &[("count", &selected.len().to_string())],
        )
    };
    labels.push(finish);
    actions.push(PickAction::Finish);

    if !parent_path.is_empty() {
        labels.push(tr("↑ Parent folder"));
        actions.push(PickAction::GoUp);
    }

    for d in dirs {
        let pattern = dir_pattern(&d.path);
        if selected.contains(&pattern) {
            labels.push(tr_fmt("− Remove folder: {name}", &[("name", &d.name)]));
            actions.push(PickAction::RemoveDir(d.name.clone()));
        } else {
            labels.push(tr_fmt("→ Open folder: {name}", &[("name", &d.name)]));
            actions.push(PickAction::EnterDir(d.name.clone()));
            labels.push(tr_fmt("+ Select folder: {name}", &[("name", &d.name)]));
            actions.push(PickAction::AddDir(d.name.clone()));
        }
    }
    for f in files {
        if selected.contains(&f.path) {
            labels.push(tr_fmt("− Deselect file: {name}", &[("name", &f.name)]));
            actions.push(PickAction::RemoveFile(f.name.clone()));
        } else {
            labels.push(tr_fmt("+ Select file: {name}", &[("name", &f.name)]));
            actions.push(PickAction::AddFile(f.name.clone()));
        }
    }

    (labels, actions)
}

fn dir_pattern(path: &str) -> String {
    let path = path.trim().trim_end_matches('/');
    if path.is_empty() {
        "**".to_string()
    } else {
        format!("{path}/**")
    }
}

fn pick_restore_target(
    cli: &Cli,
    profile: &BackupProfile,
    archive: &str,
    patterns: &[String],
) -> Result<(String, bool), ExitCode> {
    let original = archive_source_root(&profile.paths, archive);
    let unknown = tr("unknown");
    let original_label = original
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| unknown.clone());

    let target_items = vec![
        tr_fmt("Original path ({path})", &[("path", &original_label)]),
        tr("Enter another target folder …"),
    ];
    let target_refs: Vec<&str> = target_items.iter().map(String::as_str).collect();
    let target_choice = Select::with_theme(&theme())
        .with_prompt(&tr("Restore to"))
        .items(&target_refs)
        .default(0)
        .interact();

    let target_choice = match target_choice {
        Ok(i) => i,
        Err(_) => return Err(ExitCode::SUCCESS),
    };

    let target = if target_choice == 0 {
        let rel = patterns.first().map(String::as_str).unwrap_or("");
        original_restore_target_dir(&profile.paths, archive, rel)
            .or(original)
            .map(|p| p.display().to_string())
    } else {
        let fallback = original
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| {
                std::env::var("HOME")
                    .map(|h| format!("{h}/Backups/restore/{}", profile.name))
                    .unwrap_or_else(|_| "/tmp/backuppilot-restore".into())
            });
        let entered: String = Input::with_theme(&theme())
            .with_prompt(&tr("Target directory on disk"))
            .default(fallback)
            .interact_text()
            .unwrap_or_default();
        let trimmed = entered.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    };

    let Some(target) = target else {
        return Err(fail(cli, &tr("No target directory specified."), 2));
    };

    let overwrite_prompt = if target_choice == 0 {
        tr_fmt(
            "Overwrite existing files under {target}?",
            &[("target", &target)],
        )
    } else {
        tr_fmt(
            "Overwrite existing files in {target}?",
            &[("target", &target)],
        )
    };

    let overwrite = Confirm::with_theme(&theme())
        .with_prompt(overwrite_prompt)
        .default(false)
        .interact()
        .unwrap_or(false);

    println!();
    println!("{}", tr("Summary:"));
    if patterns.is_empty() {
        println!("  {}", tr("Scope: entire archive"));
    } else {
        println!(
            "  {}",
            tr_fmt("Scope: {count} path(s)/pattern(s)", &[(
                "count",
                &patterns.len().to_string()
            )])
        );
    }
    println!(
        "  {}",
        tr_fmt("Target: {target}", &[("target", &target)])
    );
    let overwrite_label = if overwrite {
        tr("yes")
    } else {
        tr("no (abort on conflicts)")
    };
    println!(
        "  {}",
        tr_fmt("Overwrite: {value}", &[("value", &overwrite_label)])
    );

    let start = Confirm::with_theme(&theme())
        .with_prompt(&tr("Start restore now?"))
        .default(true)
        .interact()
        .unwrap_or(false);
    if !start {
        return Err(ExitCode::SUCCESS);
    }

    Ok((target, overwrite))
}

async fn browse_archive(
    profile: &backuppilot_core::BackupProfile,
    snapshot: &str,
    archive: &str,
    key_id: Option<i64>,
) -> Result<(), String> {
    let mut parent_path = String::new();
    loop {
        let entries = load_catalog_entries(profile, snapshot, archive, &parent_path, key_id).await?;
        let mut dirs: Vec<&CatalogEntry> = entries.iter().filter(|e| e.is_dir).collect();
        let mut files: Vec<&CatalogEntry> = entries.iter().filter(|e| !e.is_dir).collect();
        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        files.sort_by(|a, b| a.name.cmp(&b.name));

        let mut labels = vec![tr("← Exit")];
        let mut actions = vec![BrowseAction::Quit];
        if !parent_path.is_empty() {
            labels.push(tr("↑ Parent folder"));
            actions.push(BrowseAction::GoUp);
        }
        for d in dirs {
            labels.push(tr_fmt("→ Open folder: {name}", &[("name", &d.name)]));
            actions.push(BrowseAction::EnterDir(d.name.clone()));
        }
        for f in files {
            labels.push(format!("📄 {}", f.name));
            actions.push(BrowseAction::Ignore);
        }

        let prompt = if parent_path.is_empty() {
            tr_fmt("Contents of {archive}", &[("archive", archive)])
        } else {
            tr_fmt("Contents of /{path}", &[("path", &parent_path)])
        };
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();

        let choice = Select::with_theme(&theme())
            .with_prompt(prompt)
            .items(&label_refs)
            .interact()
            .map_err(|e| e.to_string())?;

        match &actions[choice] {
            BrowseAction::Quit => break,
            BrowseAction::GoUp => {
                parent_path = parent_path_of(&parent_path);
            }
            BrowseAction::EnterDir(name) => {
                parent_path = join_catalog_path(&parent_path, name);
            }
            BrowseAction::Ignore => {}
        }
    }
    Ok(())
}

async fn load_catalog_entries(
    profile: &backuppilot_core::BackupProfile,
    snapshot: &str,
    archive: &str,
    parent_path: &str,
    key_id: Option<i64>,
) -> Result<Vec<CatalogEntry>, String> {
    let request = ListCatalogRequest {
        profile_id: profile.id,
        snapshot: snapshot.to_string(),
        archive_name: archive.to_string(),
        parent_path: parent_path.to_string(),
        force_refresh: false,
        encryption_key_id: key_id,
    };
    let response = PbsRestore::list_catalog(&request, profile)
        .await
        .map_err(|e| e.to_string())?;
    Ok(response.entries)
}

fn mount_point_from_request(profile_id: i64, snapshot: &str, archive: &str) -> Option<String> {
    mount_point_for(profile_id, snapshot, archive)
        .to_str()
        .map(str::to_string)
}

fn snapshot_menu_label(s: &PbsSnapshotInfo) -> String {
    let short = s.path.rsplit('/').next().unwrap_or(&s.path);
    let enc = if s.encrypted {
        tr("encrypted")
    } else {
        tr("plain")
    };
    format!("{short} — {} — {enc}", format_size(s.size_bytes))
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GiB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MiB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KiB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn parent_path_of(path: &str) -> String {
    let p = path.trim().trim_matches('/');
    if p.is_empty() {
        return String::new();
    }
    p.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn join_catalog_path(parent: &str, name: &str) -> String {
    let parent = parent.trim().trim_matches('/');
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

fn fail(cli: &Cli, message: &str, code: u8) -> ExitCode {
    crate::emit_error(cli, message);
    ExitCode::from(code)
}
