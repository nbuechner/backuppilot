mod dbus_iface;
mod health_monitor;
mod log_retention_monitor;
mod preflight;
mod scheduler;
mod service;
mod update_monitor;

use anyhow::Context;
use backuppilot_core::{load_app_settings, Database, UiLanguage};
use backuppilot_i18n::{self, UiLanguage as I18nLanguage};
use tracing::info;
use tracing_subscriber::EnvFilter;
use zbus::connection;

use crate::dbus_iface::{BackupPilotDaemon, DBUS_OBJECT_PATH, DBUS_WELL_KNOWN_NAME};
use crate::service::DaemonService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    backuppilot_i18n::init();
    apply_language(&load_app_settings());

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("backuppilot=info".parse()?))
        .init();

    let db = Database::open().context("failed to open database")?;
    let service = DaemonService::new(db);
    service.reconcile_interrupted_runs().await;
    scheduler::spawn(service.clone());
    health_monitor::spawn(service.clone());
    log_retention_monitor::spawn(service.clone());
    update_monitor::spawn();
    let daemon = BackupPilotDaemon { service };

    let _connection = connection::Builder::session()?
        .name(DBUS_WELL_KNOWN_NAME)?
        .serve_at(DBUS_OBJECT_PATH, daemon)?
        .build()
        .await
        .context("failed to start D-Bus service")?;

    info!(
        name = DBUS_WELL_KNOWN_NAME,
        path = DBUS_OBJECT_PATH,
        "BackupPilot daemon running"
    );

    std::future::pending::<()>().await;
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
