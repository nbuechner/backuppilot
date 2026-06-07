/// Single generic Tauri command: proxy any IPC method call to the daemon.
/// The JS frontend calls: invoke('ipc_call', { method: 'list_profiles', params: null })
#[tauri::command]
pub async fn ipc_call(
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let call = backuppilot_ipc::IpcClient::call(&method, params);
    let raw = tokio::time::timeout(std::time::Duration::from_secs(15), call)
        .await
        .map_err(|_| format!("daemon did not respond to '{method}' within 15 s"))?
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

/// Open a local path in the system file manager (Explorer on Windows).
/// Used for UNC paths (\\wsl.localhost\...) that openShell cannot handle.
#[tauri::command]
pub fn open_in_explorer(path: String) {
    #[cfg(windows)]
    { let _ = std::process::Command::new("explorer").arg(&path).spawn(); }
    #[cfg(not(windows))]
    { let _ = std::process::Command::new("xdg-open").arg(&path).spawn(); }
}
