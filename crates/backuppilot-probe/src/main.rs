//! Probe the running daemon and print a summary of its state.
//! Used for automated testing: exits 0 on success, non-zero on any error.
use backuppilot_ipc::IpcClient;
use serde_json::Value;

#[tokio::main]
async fn main() {
    let mut ok = true;

    // ── version ────────────────────────────────────────────────────────────────
    match IpcClient::call("version", Value::Null).await {
        Ok(v) => println!("version: {v}"),
        Err(e) => { eprintln!("ERROR version: {e}"); ok = false; }
    }

    // ── list_profiles ──────────────────────────────────────────────────────────
    match IpcClient::call("list_profiles", Value::Null).await {
        Ok(raw) => {
            match serde_json::from_str::<Value>(&raw) {
                Ok(profiles) => {
                    let arr = profiles.as_array().cloned().unwrap_or_default();
                    println!("profiles: {} found", arr.len());
                    for p in &arr {
                        let id   = p["id"].as_i64().unwrap_or(-1);
                        let name = p["name"].as_str().unwrap_or("?");
                        let enabled = p["enabled"].as_bool().unwrap_or(false);
                        let repo = p["repository"].as_str().unwrap_or("?");
                        let host = serde_json::from_str::<Value>(repo)
                            .ok()
                            .and_then(|r| r["host"].as_str().map(str::to_string))
                            .unwrap_or_else(|| repo.to_string());
                        println!("  [{id}] {name}  enabled={enabled}  host={host}");
                    }
                }
                Err(e) => { eprintln!("ERROR parse profiles: {e}"); ok = false; }
            }
        }
        Err(e) => { eprintln!("ERROR list_profiles: {e}"); ok = false; }
    }

    // ── list_statuses ──────────────────────────────────────────────────────────
    match IpcClient::call("list_statuses", Value::Null).await {
        Ok(raw) => {
            match serde_json::from_str::<Value>(&raw) {
                Ok(statuses) => {
                    let arr = statuses.as_array().cloned().unwrap_or_default();
                    println!("statuses: {} found", arr.len());
                    for s in &arr {
                        let id      = s["profile_id"].as_i64().unwrap_or(-1);
                        let name    = s["profile_name"].as_str().unwrap_or("?");
                        let health  = s["health"].as_str().unwrap_or("?");
                        let in_prog = s["backup_in_progress"].as_bool().unwrap_or(false);
                        let last_status = s["last_run"]["status"].as_str().unwrap_or("never");
                        let days    = s["days_since_last_success"].as_i64();
                        let days_str = days.map(|d| format!("{d}d ago")).unwrap_or_else(|| "never".into());
                        println!("  [{id}] {name}  health={health}  last={last_status}  success={days_str}  running={in_prog}");
                    }
                }
                Err(e) => { eprintln!("ERROR parse statuses: {e}"); ok = false; }
            }
        }
        Err(e) => { eprintln!("ERROR list_statuses: {e}"); ok = false; }
    }

    if !ok {
        std::process::exit(1);
    }
}
