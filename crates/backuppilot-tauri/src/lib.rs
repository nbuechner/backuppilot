mod commands;
mod daemon;
mod tray;

pub fn run() {
    tracing_subscriber::fmt::init();

    backuppilot_core::notify::register_windows_aumid();
    daemon::ensure_daemon_running();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![commands::ipc_call, commands::open_in_explorer])
        .setup(|app| {
            tray::setup_tray(app)?;
            // Show window after setup (was created hidden so tray icon appears first)
            use tauri::Manager;
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running BackupPilot");
}
