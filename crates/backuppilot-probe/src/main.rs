//! Probe the running daemon and print a summary of its state.
//! Used for automated testing: exits 0 on success, non-zero on any error.
//!
//! Flags:
//!   --run-backup   trigger run_backup on each profile and poll until done
use backuppilot_ipc::IpcClient;
use serde_json::Value;

async fn ipc<P: serde::Serialize>(method: &str, params: P) -> Result<Value, String> {
    let p = serde_json::to_value(params).map_err(|e| e.to_string())?;
    let raw = IpcClient::call(method, p).await.map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("parse {method}: {e}"))
}

#[tokio::main]
async fn main() {
    let run_backup = std::env::args().any(|a| a == "--run-backup");
    let mut ok = true;

    // ── version ────────────────────────────────────────────────────────────────
    match IpcClient::call("version", Value::Null).await {
        Ok(v) => println!("version: {v}"),
        Err(e) => { eprintln!("ERROR version: {e}"); ok = false; }
    }

    // ── profiles + statuses ────────────────────────────────────────────────────
    let profiles = match ipc("list_profiles", Value::Null).await {
        Ok(v) => { let arr = v.as_array().cloned().unwrap_or_default(); arr }
        Err(e) => { eprintln!("ERROR list_profiles: {e}"); ok = false; vec![] }
    };
    println!("profiles: {}", profiles.len());
    for p in &profiles {
        let id      = p["id"].as_i64().unwrap_or(-1);
        let name    = p["name"].as_str().unwrap_or("?");
        let enabled = p["enabled"].as_bool().unwrap_or(false);
        let repo    = p["repository"].as_str().unwrap_or("{}");
        let host    = serde_json::from_str::<Value>(repo).ok()
            .and_then(|r| r["host"].as_str().map(str::to_string))
            .unwrap_or_else(|| repo.to_string());
        println!("  [{id}] {name}  enabled={enabled}  host={host}");
    }

    let statuses = match ipc("list_statuses", Value::Null).await {
        Ok(v) => v.as_array().cloned().unwrap_or_default(),
        Err(e) => { eprintln!("ERROR list_statuses: {e}"); ok = false; vec![] }
    };
    println!("statuses: {}", statuses.len());
    for s in &statuses {
        let id      = s["profile_id"].as_i64().unwrap_or(-1);
        let name    = s["profile_name"].as_str().unwrap_or("?");
        let health  = s["health"].as_str().unwrap_or("?");
        let running = s["backup_in_progress"].as_bool().unwrap_or(false);
        let last    = s["last_run"]["status"].as_str().unwrap_or("never");
        let days    = s["days_since_last_success"].as_i64()
            .map(|d| format!("{d}d ago")).unwrap_or_else(|| "never".into());
        println!("  [{id}] {name}  health={health}  last={last}  success={days}  running={running}");
    }

    // ── recent activity ────────────────────────────────────────────────────────
    match ipc("list_recent_activity", serde_json::json!({ "limit": 5 })).await {
        Ok(v) => {
            let arr = v.as_array().cloned().unwrap_or_default();
            println!("recent activity: {} entries", arr.len());
            for e in &arr {
                let name   = e["profile_name"].as_str().unwrap_or("system");
                let status = e["run"]["status"].as_str().unwrap_or("?");
                let start  = e["run"]["started_at"].as_str().unwrap_or("?");
                let msg    = e["run"]["error_message"].as_str().unwrap_or("");
                let suffix = if msg.is_empty() { String::new() } else { format!("  err={msg}") };
                println!("  {start}  {name}  {status}{suffix}");
            }
        }
        Err(e) => { eprintln!("ERROR list_recent_activity: {e}"); ok = false; }
    }

    // ── snapshots + catalog (first snapshot of each profile) ──────────────────
    for p in &profiles {
        let id   = p["id"].as_i64().unwrap_or(-1);
        let name = p["name"].as_str().unwrap_or("?");
        match ipc("list_snapshots", serde_json::json!({ "profile_id": id })).await {
            Ok(v) => {
                let snaps = v.as_array().cloned().unwrap_or_default();
                println!("snapshots [{id}] {name}: {} found", snaps.len());
                for s in snaps.iter().take(3) {
                    let path = s["path"].as_str().unwrap_or("?");
                    let size = s["size_bytes"].as_u64().unwrap_or(0);
                    let enc  = s["encrypted"].as_bool().unwrap_or(false);
                    println!("  {path}  size={size}  encrypted={enc}");
                }
                // verify catalog can be listed for the most recent snapshot
                if let Some(snap) = snaps.first() {
                    let snap_path = snap["path"].as_str().unwrap_or("");
                    match ipc("list_catalog", serde_json::json!({
                        "profile_id": id,
                        "snapshot": snap_path,
                        "archive_name": "",
                        "parent_path": "/",
                        "force_refresh": false,
                    })).await {
                        Ok(cat) => {
                            let archives = cat["archives"].as_array().map(|a| a.len()).unwrap_or(0);
                            let entries  = cat["entries"].as_array().map(|a| a.len()).unwrap_or(0);
                            let suggested = cat["suggested_archive"].as_str().unwrap_or("none");
                            println!("  catalog {snap_path}: archives={archives}  entries={entries}  suggested={suggested}");
                        }
                        Err(e) => { eprintln!("  ERROR list_catalog {snap_path}: {e}"); ok = false; }
                    }
                }
            }
            Err(e) => { eprintln!("ERROR list_snapshots [{id}]: {e}"); ok = false; }
        }
    }

    // ── optional: trigger backup and poll to completion ────────────────────────
    if run_backup {
        for p in &profiles {
            let id   = p["id"].as_i64().unwrap_or(-1);
            let name = p["name"].as_str().unwrap_or("?");
            println!("run_backup [{id}] {name}...");
            match ipc("run_backup", serde_json::json!({ "profile_id": id })).await {
                Ok(result) => {
                    let started = result["started"].as_bool().unwrap_or(false);
                    let skipped = result["skipped"].as_bool().unwrap_or(false);
                    let msg     = result["message"].as_str().unwrap_or("");
                    println!("  started={started}  skipped={skipped}  msg={msg}");
                    if started {
                        // poll until no longer running
                        for _ in 0..60 {
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            match ipc("list_statuses", Value::Null).await {
                                Ok(v) => {
                                    let running = v.as_array().unwrap_or(&vec![]).iter()
                                        .find(|s| s["profile_id"].as_i64() == Some(id))
                                        .and_then(|s| s["backup_in_progress"].as_bool())
                                        .unwrap_or(false);
                                    let last = v.as_array().unwrap_or(&vec![]).iter()
                                        .find(|s| s["profile_id"].as_i64() == Some(id))
                                        .and_then(|s| s["last_run"]["status"].as_str().map(str::to_string))
                                        .unwrap_or_default();
                                    print!("  polling... running={running} last={last}");
                                    if !running {
                                        println!("  => done: {last}");
                                        if last != "success" { ok = false; }
                                        break;
                                    }
                                    println!();
                                }
                                Err(e) => { eprintln!("  poll error: {e}"); break; }
                            }
                        }
                    }
                }
                Err(e) => { eprintln!("ERROR run_backup [{id}]: {e}"); ok = false; }
            }
        }
    }

    if !ok {
        std::process::exit(1);
    }
}
