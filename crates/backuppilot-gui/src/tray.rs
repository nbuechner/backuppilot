//! System tray (StatusNotifier) — status icon, tooltip, and quick actions.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk::glib;
use ksni::menu::StandardItem;
use ksni::{Error as KsniError, MenuItem, Status, ToolTip, Tray, TrayMethods};

use backuppilot_core::profile::ProfileStatus;
use backuppilot_i18n::tr;

use crate::backup_actions;
use crate::dbus_client::{self, connect};
use crate::icons;
use crate::tray_state::{TrayIndicator, TrayProfileLine, TrayState};
use crate::window;

struct BackupPilotTray {
    state: Arc<Mutex<TrayState>>,
}

impl Tray for BackupPilotTray {
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        window::application_id().into()
    }

    fn title(&self) -> String {
        tr("BackupPilot")}

    fn status(&self) -> Status {
        self.state.lock().unwrap().ksni_status()
    }

    fn icon_theme_path(&self) -> String {
        self.state.lock().unwrap().icon_assets.icon_theme_path.clone()
    }

    fn icon_name(&self) -> String {
        self.state.lock().unwrap().icon_assets.icon_name.clone()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.state.lock().unwrap().icon_assets.pixmaps.clone()
    }

    fn overlay_icon_name(&self) -> String {
        self.state.lock().unwrap().icon_assets.overlay_icon_name.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        let state = self.state.lock().unwrap();
        ToolTip {
            icon_name: state.icon_assets.icon_name.clone(),
            title: tr("BackupPilot"),
            description: state.tooltip_body.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        schedule_present_main_window();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let state = self.state.lock().unwrap();
        let idle: Vec<&TrayProfileLine> = state
            .profiles
            .iter()
            .filter(|p| !p.backup_in_progress)
            .collect();
        let running: Vec<&TrayProfileLine> = state
            .profiles
            .iter()
            .filter(|p| p.backup_in_progress)
            .collect();

        let mut items: Vec<MenuItem<Self>> = vec![
            StandardItem {
                label: tr("Open BackupPilot"),
                icon_name: icons::ICON_NAME.to_string(),
                activate: Box::new(|_: &mut Self| schedule_present_main_window()),
                ..Default::default()
            }
            .into(),
        ];

        match idle.len() {
            0 => {}
            1 => {
                let profile_id = idle[0].id;
                items.push(start_backup_item(profile_id, tr("Start backup")));
            }
            _ => {
                items.push(start_backup_submenu(&idle));
            }
        }

        match running.len() {
            0 => {}
            1 => {
                let profile_id = running[0].id;
                items.push(stop_backup_item(profile_id, tr("Stop backup")));
            }
            _ => {
                items.push(stop_backup_submenu(&running));
            }
        }

        let pause_label = if backuppilot_core::load_app_settings().tray.pause_all_backups {
            tr("Resume scheduled backups")} else {
            tr("Pause scheduled backups")};

        items.push(
            StandardItem {
                label: tr("Open restore"),
                icon_name: "folder-download-symbolic".into(),
                activate: Box::new(|_: &mut Self| schedule_open_restore()),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: tr("Open log"),
                icon_name: "text-x-generic-symbolic".into(),
                activate: Box::new(|_: &mut Self| schedule_open_logs()),
                ..Default::default()
            }
            .into(),
        );
        if backuppilot_core::app_update_checks_enabled() {
            items.push(
                StandardItem {
                    label: tr("Check for updates"),
                    icon_name: "software-update-available-symbolic".into(),
                    activate: Box::new(|_: &mut Self| schedule_check_updates()),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(
            StandardItem {
                label: pause_label,
                icon_name: "media-playback-pause-symbolic".into(),
                activate: Box::new(|_: &mut Self| schedule_toggle_pause()),
                ..Default::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: tr("Quit"),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|_: &mut Self| schedule_quit()),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

fn start_backup_item(profile_id: i64, label: String) -> MenuItem<BackupPilotTray> {
    StandardItem {
        label,
        icon_name: "folder-save-symbolic".into(),
        activate: Box::new(move |_: &mut BackupPilotTray| schedule_start_backup(profile_id)),
        ..Default::default()
    }
    .into()
}

fn stop_backup_item(profile_id: i64, label: String) -> MenuItem<BackupPilotTray> {
    StandardItem {
        label,
        icon_name: "process-stop-symbolic".into(),
        activate: Box::new(move |_: &mut BackupPilotTray| schedule_stop_backup(profile_id)),
        ..Default::default()
    }
    .into()
}

fn start_backup_submenu(profiles: &[&TrayProfileLine]) -> MenuItem<BackupPilotTray> {
    let submenu: Vec<MenuItem<BackupPilotTray>> = profiles
        .iter()
        .map(|profile| {
            let profile_id = profile.id;
            let label = profile.name.clone();
            start_backup_item(profile_id, label)
        })
        .collect();

    ksni::menu::SubMenu {
        label: tr("Start backup"),
        icon_name: "folder-save-symbolic".into(),
        submenu,
        ..Default::default()
    }
    .into()
}

fn stop_backup_submenu(profiles: &[&TrayProfileLine]) -> MenuItem<BackupPilotTray> {
    let submenu: Vec<MenuItem<BackupPilotTray>> = profiles
        .iter()
        .map(|profile| {
            let profile_id = profile.id;
            let label = profile.name.clone();
            stop_backup_item(profile_id, label)
        })
        .collect();

    ksni::menu::SubMenu {
        label: tr("Stop backup"),
        icon_name: "process-stop-symbolic".into(),
        submenu,
        ..Default::default()
    }
    .into()
}

fn schedule_present_main_window() {
    glib::MainContext::default().invoke(window::present_from_tray);
}

fn schedule_start_backup(profile_id: i64) {
    glib::MainContext::default().invoke(move || backup_actions::start_backup_from_tray(profile_id));
}

fn schedule_stop_backup(profile_id: i64) {
    glib::MainContext::default().invoke(move || backup_actions::cancel_backup_from_tray(profile_id));
}

fn schedule_quit() {
    glib::MainContext::default().invoke(window::quit_application);
}

fn schedule_open_restore() {
    glib::MainContext::default().invoke(|| {
        window::present_from_tray();
        window::switch_to_restore();
    });
}

fn schedule_open_logs() {
    glib::MainContext::default().invoke(|| {
        window::present_from_tray();
        window::switch_to_logs();
    });
}

fn schedule_check_updates() {
    glib::MainContext::default().invoke(|| {
        window::present_from_tray();
        let channel = backuppilot_core::load_app_settings().updates.channel;
        crate::updates::run_check_interactive(channel, false, true);
    });
}

fn schedule_toggle_pause() {
    glib::MainContext::default().invoke(|| {
        crate::dbus_runtime::spawn(
            async move {
                let proxy = connect().await?;
                dbus_client::toggle_pause_all_backups(&proxy).await
            },
            |_| {
                window::refresh_dashboard_public();
            },
        );
    });
}

async fn fetch_statuses() -> zbus::Result<Vec<ProfileStatus>> {
    let proxy = connect().await?;
    dbus_client::list_statuses(&proxy).await
}

const TRAY_SPAWN_ATTEMPTS: u32 = 20;
const TRAY_SPAWN_DELAY: Duration = Duration::from_secs(2);

static TRAY_SPAWNED: AtomicBool = AtomicBool::new(false);

pub fn spawn() {
    if TRAY_SPAWNED.swap(true, Ordering::SeqCst) {
        tracing::debug!("tray: spawn skipped (already running)");
        return;
    }
    std::thread::spawn(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tray tokio runtime");

        rt.block_on(async {
            let mut state = TrayState::default();
            state.icon_assets = icons::tray_assets_for_indicator(state.indicator, state.spin_frame);
            let state = Arc::new(Mutex::new(state));

            let handle = match spawn_tray_with_retry(state.clone()).await {
                Some(h) => h,
                None => return,
            };

            let _ = handle
                .update(|_: &mut BackupPilotTray| true)
                .await;

            info_tray_assets(&state.lock().unwrap().icon_assets);

            let mut interval = tokio::time::interval(Duration::from_secs(2));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                let statuses = match fetch_statuses().await {
                    Ok(s) => s,
                    Err(err) => {
                        tracing::debug!(%err, "tray: daemon status poll failed");
                        continue;
                    }
                };

                let changed = {
                    let mut st = state.lock().unwrap();
                    let prev_indicator = st.indicator;
                    let prev_tooltip = st.tooltip_body.clone();
                    let prev_overlay = st.icon_assets.overlay_icon_name.clone();
                    st.update_from_statuses(&statuses);
                    if st.indicator == TrayIndicator::Running {
                        st.spin_frame = st.spin_frame.wrapping_add(1);
                        st.icon_assets.overlay_icon_name =
                            icons::overlay_icon_for_indicator(st.indicator, st.spin_frame);
                    }
                    st.indicator != prev_indicator
                        || st.indicator == TrayIndicator::Running
                        || st.tooltip_body != prev_tooltip
                        || st.icon_assets.overlay_icon_name != prev_overlay
                };

                if changed {
                    let _ = handle
                        .update(|_: &mut BackupPilotTray| true)
                        .await;
                }
            }
        });
    });
}

async fn spawn_tray_with_retry(
    state: Arc<Mutex<TrayState>>,
) -> Option<ksni::Handle<BackupPilotTray>> {
    for attempt in 1..=TRAY_SPAWN_ATTEMPTS {
        let tray = BackupPilotTray {
            state: state.clone(),
        };
        match tray.spawn().await {
            Ok(handle) => {
                tracing::info!(attempt, "system tray registered (StatusNotifierItem)");
                return Some(handle);
            }
            Err(KsniError::WontShow) => {
                tracing::debug!(
                    attempt,
                    max = TRAY_SPAWN_ATTEMPTS,
                    "tray: StatusNotifier host not ready, retrying…"
                );
                tokio::time::sleep(TRAY_SPAWN_DELAY).await;
            }
            Err(err) => {
                tracing::warn!(%err, "system tray unavailable (StatusNotifierItem)");
                return None;
            }
        }
    }
    tracing::warn!(
        "system tray not shown after {TRAY_SPAWN_ATTEMPTS} attempts (desktop may hide tray icons)"
    );
    None
}

fn info_tray_assets(assets: &icons::TrayIconAssets) {
    tracing::info!(
        icon = %assets.icon_name,
        theme_path = %assets.icon_theme_path,
        pixmaps = assets.pixmaps.len(),
        overlay = %assets.overlay_icon_name,
        "tray icon configured"
    );
}
