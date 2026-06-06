/// Single generic Tauri command: proxy any IPC method call to the daemon.
/// The JS frontend calls: invoke('ipc_call', { method: 'list_profiles', params: null })
#[tauri::command]
pub async fn ipc_call(
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let raw = backuppilot_ipc::IpcClient::call(&method, params)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}
