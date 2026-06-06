mod dbus_iface;
mod health_monitor;
mod log_retention_monitor;
mod preflight;
mod scheduler;
mod service;
mod update_monitor;

use anyhow::Context;
use backuppilot_core::{load_app_settings, paths, Database, UiLanguage};
use backuppilot_i18n::{self, UiLanguage as I18nLanguage};
use backuppilot_ipc::IpcServer;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::service::DaemonService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Hide the console window in release builds — the daemon runs in the background.
    #[cfg(all(windows, not(debug_assertions)))]
    unsafe {
        extern "system" { fn FreeConsole() -> i32; }
        FreeConsole();
    }

    backuppilot_i18n::init();
    apply_language(&load_app_settings());

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("backuppilot=info".parse()?))
        .init();

    // Exit immediately if another daemon instance is already running.
    if backuppilot_ipc::IpcClient::call("version", serde_json::Value::Null).await.is_ok() {
        info!("BackupPilot daemon already running — exiting");
        return Ok(());
    }

    // Resolve and cache the PBS client binary path before serving any requests.
    // On Windows this may probe WSL; doing it here (with a timeout) prevents
    // the OnceLock from being held indefinitely by a hanging wsl.exe probe.
    paths::init_pbs_client_path().await;

    let db = Database::open().context("failed to open database")?;
    let service = DaemonService::new(db);
    service.reconcile_interrupted_runs().await;
    scheduler::spawn(service.clone());
    health_monitor::spawn(service.clone());
    log_retention_monitor::spawn(service.clone());
    update_monitor::spawn();

    let server = IpcServer::bind().context("failed to bind IPC socket")?;
    info!("BackupPilot daemon running (IPC)");

    server
        .serve(move |method, params| {
            let svc = service.clone();
            async move { dbus_iface::dispatch(&svc, &method, params).await }
        })
        .await;

    Ok(())
}

fn apply_language(settings: &backuppilot_core::AppSettings) {
    let lang = match settings.appearance.language {
        UiLanguage::System => I18nLanguage::System,
        UiLanguage::German => I18nLanguage::German,
        UiLanguage::English => I18nLanguage::English,
        UiLanguage::French => I18nLanguage::French,
        UiLanguage::Italian => I18nLanguage::Italian,
    };
    backuppilot_i18n::set_language(lang);
}
