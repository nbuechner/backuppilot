//! Profile create, update, delete, import/export via YAML.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use backuppilot_core::{
    merge_repository_for_update, normalize_new_profile, parse_profile_yaml, profile_to_yaml,
    resolve_profile_id, Database,
};

use crate::{emit_error, Cli};

pub async fn run_profile(cli: &Cli, command: ProfileCommand) -> ExitCode {
    match command {
        ProfileCommand::Show { profile, output } => cmd_show(cli, &profile, output.as_deref()).await,
        ProfileCommand::Create { file } => cmd_create(cli, &file).await,
        ProfileCommand::Update { profile, file } => cmd_update(cli, &profile, &file).await,
        ProfileCommand::Delete { profile, yes } => cmd_delete(cli, &profile, yes).await,
        ProfileCommand::Enable { profile } => cmd_set_enabled(cli, &profile, true).await,
        ProfileCommand::Disable { profile } => cmd_set_enabled(cli, &profile, false).await,
        ProfileCommand::Export { profile, output } => cmd_export(cli, &profile, output.as_deref()).await,
        ProfileCommand::Import { file, replace } => cmd_import(cli, &file, replace).await,
    }
}

#[derive(Clone, clap::Subcommand)]
pub enum ProfileCommand {
    /// Show a profile as YAML (stdout or file).
    Show {
        profile: String,
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
    },
    /// Create a profile from a YAML file.
    Create {
        #[arg(short, long)]
        file: std::path::PathBuf,
    },
    /// Update a profile from a YAML file (keeps stored API token if omitted).
    Update {
        profile: String,
        #[arg(short, long)]
        file: std::path::PathBuf,
    },
    /// Delete a profile.
    Delete {
        profile: String,
        /// Skip confirmation prompt.
        #[arg(short, long)]
        yes: bool,
    },
    /// Enable a profile.
    Enable {
        profile: String,
    },
    /// Disable a profile.
    Disable {
        profile: String,
    },
    /// Export a profile to YAML.
    Export {
        profile: String,
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
    },
    /// Import a YAML file (create, or update if --replace and name exists).
    Import {
        #[arg(short, long)]
        file: std::path::PathBuf,
        /// Update existing profile with the same name instead of failing.
        #[arg(long)]
        replace: bool,
    },
}

async fn cmd_show(cli: &Cli, profile: &str, output: Option<&Path>) -> ExitCode {
    let db = match open_db(cli) {
        Ok(db) => db,
        Err(code) => return code,
    };
    let id = match resolve_id(cli, &db, profile) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let profile_row = match db.get_profile(id) {
        Ok(p) => p,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    let yaml = match profile_to_yaml(&db, &profile_row) {
        Ok(y) => y,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    write_or_print(cli, output, &yaml, "profile")
}

async fn cmd_export(cli: &Cli, profile: &str, output: Option<&Path>) -> ExitCode {
    cmd_show(cli, profile, output).await
}

async fn cmd_create(cli: &Cli, file: &Path) -> ExitCode {
    let yaml = match read_file(cli, file) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let doc = match parse_profile_yaml(&yaml) {
        Ok(d) => d,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    let name = doc.name.clone();
    let db = match open_db(cli) {
        Ok(db) => db,
        Err(code) => return code,
    };
    if db.find_profile_id_by_name(&name).ok().flatten().is_some() {
        emit_error(
            cli,
            &format!("profile already exists: {name} (use `profile import --replace` or `profile update`)"),
        );
        return ExitCode::from(1);
    }
    let new = match doc.into_new_profile(&db) {
        Ok(n) => n,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    match db.insert_profile(&new) {
        Ok(p) => report_profile_saved(cli, &p.name, p.id, false),
        Err(err) => {
            emit_error(cli, &err.to_string());
            ExitCode::from(2)
        }
    }
}

async fn cmd_update(cli: &Cli, profile: &str, file: &Path) -> ExitCode {
    let yaml = match read_file(cli, file) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let doc = match parse_profile_yaml(&yaml) {
        Ok(d) => d,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    let db = match open_db(cli) {
        Ok(db) => db,
        Err(code) => return code,
    };
    let id = match resolve_id(cli, &db, profile) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let mut new = match doc.into_new_profile(&db) {
        Ok(n) => n,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    new.repository = match merge_repository_for_update(id, &new.repository) {
        Ok(r) => r,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    new = normalize_new_profile(new);
    match db.update_profile(id, &new) {
        Ok(p) => report_profile_saved(cli, &p.name, p.id, true),
        Err(err) => {
            emit_error(cli, &err.to_string());
            ExitCode::from(2)
        }
    }
}

async fn cmd_import(cli: &Cli, file: &Path, replace: bool) -> ExitCode {
    let yaml = match read_file(cli, file) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let doc = match parse_profile_yaml(&yaml) {
        Ok(d) => d,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    let name = doc.name.clone();
    let db = match open_db(cli) {
        Ok(db) => db,
        Err(code) => return code,
    };
    if let Some(id) = db.find_profile_id_by_name(&name).ok().flatten() {
        if !replace {
            emit_error(
                cli,
                &format!("profile already exists: {name} (use --replace to update)"),
            );
            return ExitCode::from(1);
        }
        let mut new = match doc.into_new_profile(&db) {
            Ok(n) => n,
            Err(err) => {
                emit_error(cli, &err.to_string());
                return ExitCode::from(2);
            }
        };
        new.repository = match merge_repository_for_update(id, &new.repository) {
            Ok(r) => r,
            Err(err) => {
                emit_error(cli, &err.to_string());
                return ExitCode::from(2);
            }
        };
        new = normalize_new_profile(new);
        return match db.update_profile(id, &new) {
            Ok(p) => report_profile_saved(cli, &p.name, p.id, true),
            Err(err) => {
                emit_error(cli, &err.to_string());
                ExitCode::from(2)
            }
        };
    }
    let new = match doc.into_new_profile(&db) {
        Ok(n) => n,
        Err(err) => {
            emit_error(cli, &err.to_string());
            return ExitCode::from(2);
        }
    };
    match db.insert_profile(&new) {
        Ok(p) => report_profile_saved(cli, &p.name, p.id, false),
        Err(err) => {
            emit_error(cli, &err.to_string());
            ExitCode::from(2)
        }
    }
}

async fn cmd_delete(cli: &Cli, profile: &str, yes: bool) -> ExitCode {
    let db = match open_db(cli) {
        Ok(db) => db,
        Err(code) => return code,
    };
    let id = match resolve_id(cli, &db, profile) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let name = db.get_profile(id).map(|p| p.name).unwrap_or_default();
    if !yes && !cli.json {
        eprintln!(
            "{}",
            crate::i18n::tr_fmt(
                "Delete profile «{name}»? Re-run with --yes to confirm.",
                &[("name", &name)]
            )
        );
        return ExitCode::from(1);
    }
    match db.delete_profile(id) {
        Ok(()) => {
            if cli.json {
                println!(
                    r#"{{"success":true,"deleted":true,"id":{id},"name":{}}}"#,
                    serde_json::to_string(&name).unwrap_or_default()
                );
            } else {
                println!(
                    "{}",
                    crate::i18n::tr_fmt("Deleted profile {name}.", &[("name", &name)])
                );
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            emit_error(cli, &err.to_string());
            ExitCode::from(2)
        }
    }
}

async fn cmd_set_enabled(cli: &Cli, profile: &str, enabled: bool) -> ExitCode {
    let db = match open_db(cli) {
        Ok(db) => db,
        Err(code) => return code,
    };
    let id = match resolve_id(cli, &db, profile) {
        Ok(id) => id,
        Err(code) => return code,
    };
    match db.set_profile_enabled(id, enabled) {
        Ok(p) => {
            if cli.json {
                println!(
                    r#"{{"success":true,"id":{},"name":{},"enabled":{}}}"#,
                    p.id,
                    serde_json::to_string(&p.name).unwrap_or_default(),
                    p.enabled
                );
            } else {
                let msg = if enabled {
                    crate::i18n::tr_fmt("Enabled profile {name}.", &[("name", &p.name)])
                } else {
                    crate::i18n::tr_fmt("Disabled profile {name}.", &[("name", &p.name)])
                };
                println!("{msg}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            emit_error(cli, &err.to_string());
            ExitCode::from(2)
        }
    }
}

fn open_db(cli: &Cli) -> Result<Database, ExitCode> {
    Database::open().map_err(|err| {
        emit_error(cli, &err.to_string());
        ExitCode::from(2)
    })
}

fn resolve_id(cli: &Cli, db: &Database, profile: &str) -> Result<i64, ExitCode> {
    resolve_profile_id(db, profile).map_err(|err| {
        emit_error(cli, &err.to_string());
        ExitCode::from(2)
    })
}

fn read_file(cli: &Cli, path: &Path) -> Result<String, ExitCode> {
    fs::read_to_string(path).map_err(|err| {
        emit_error(cli, &format!("{}: {err}", path.display()));
        ExitCode::from(2)
    })
}

fn write_or_print(cli: &Cli, output: Option<&Path>, content: &str, label: &str) -> ExitCode {
    if let Some(path) = output {
        if let Err(err) = fs::write(path, content) {
            emit_error(cli, &format!("{}: {err}", path.display()));
            return ExitCode::from(2);
        }
        if cli.json {
            println!(
                r#"{{"success":true,"written":true,"path":{}}}"#,
                serde_json::to_string(&path.display().to_string()).unwrap_or_default()
            );
        } else {
            println!(
                "{}",
                crate::i18n::tr_fmt(
                    "Wrote {label} to {path}.",
                    &[
                        ("label", label),
                        ("path", &path.display().to_string()),
                    ]
                )
            );
        }
    } else {
        print!("{content}");
    }
    ExitCode::SUCCESS
}

fn report_profile_saved(cli: &Cli, name: &str, id: i64, updated: bool) -> ExitCode {
    if cli.json {
        println!(
            r#"{{"success":true,"updated":{},"id":{},"name":{}}}"#,
            updated,
            id,
            serde_json::to_string(name).unwrap_or_default()
        );
    } else {
        let msg = if updated {
            crate::i18n::tr_fmt("Updated profile {name} (id {id}).", &[("name", name), ("id", &id.to_string())])
        } else {
            crate::i18n::tr_fmt("Created profile {name} (id {id}).", &[("name", name), ("id", &id.to_string())])
        };
        println!("{msg}");
    }
    ExitCode::SUCCESS
}
