use std::cell::{Cell, RefCell};
use std::rc::Rc;

use backuppilot_core::profile::{ActivityLogEntry, HealthState, ProfileStatus};
use backuppilot_core::ActiveMount;
use backuppilot_i18n::{tr, tr_fmt};
use gtk::glib;
use gtk::prelude::*;
use libadwaita::prelude::*;
use libadwaita::{Application, ApplicationWindow, ToastOverlay};

use backuppilot_core::updates::{is_update_newer_than_installed, load_update_state};

use crate::activity_log::{self, ActivityRowMode};
use crate::icons;
use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::logs;
use crate::onboarding;
use crate::pbs_client_setup;
use crate::profiles::{self, editor};
use crate::encryption_keys;
use crate::restore;
use crate::about;
use crate::settings;
use crate::settings_apply;
use crate::shell;
use crate::tray;
use crate::util::{
    clear_list_box, find_child_by_name, format_duration_since, status_prefix_label,
};

use backuppilot_core::APP_ID;

thread_local! {
    static MAIN_WINDOW: RefCell<Option<ApplicationWindow>> = const { RefCell::new(None) };
    static DASHBOARD_PAGE: RefCell<Option<gtk::Widget>> = const { RefCell::new(None) };
    static PROFILES_PAGE: RefCell<Option<gtk::Widget>> = const { RefCell::new(None) };
    static TOAST_OVERLAY: RefCell<Option<ToastOverlay>> = const { RefCell::new(None) };
    static VIEW_STACK: RefCell<Option<gtk::Stack>> = const { RefCell::new(None) };
    static APP: RefCell<Option<glib::WeakRef<Application>>> = const { RefCell::new(None) };
    static APP_HOLD: RefCell<Option<gtk::gio::ApplicationHoldGuard>> = const { RefCell::new(None) };
    static DASHBOARD_POLL: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
    static PROFILES_PAGE_READY: Cell<bool> = const { Cell::new(false) };
    static ENCRYPTION_PAGE_READY: Cell<bool> = const { Cell::new(false) };
    static SETTINGS_PAGE_READY: Cell<bool> = const { Cell::new(false) };
    static LOGS_PAGE_READY: Cell<bool> = const { Cell::new(false) };
    static ABOUT_PAGE_READY: Cell<bool> = const { Cell::new(false) };
    static RESTORE_PAGE_READY: Cell<bool> = const { Cell::new(false) };
    static SUPPRESS_STACK_PAGE_NOTIFY: Cell<bool> = const { Cell::new(false) };
    static DASHBOARD_REFRESH_IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
    static DASHBOARD_REFRESH_PENDING: Cell<bool> = const { Cell::new(false) };
    static WINDOW_TITLE: RefCell<Option<libadwaita::WindowTitle>> = const { RefCell::new(None) };
    static DASHBOARD_STATUS_LIST: RefCell<Option<gtk::ListBox>> = const { RefCell::new(None) };
    static DASHBOARD_ACTIVITY_LIST: RefCell<Option<gtk::ListBox>> = const { RefCell::new(None) };
    static TRAY_SPAWN_PENDING: Cell<bool> = const { Cell::new(false) };
}

fn reset_lazy_pages() {
    PROFILES_PAGE_READY.set(false);
    ENCRYPTION_PAGE_READY.set(false);
    SETTINGS_PAGE_READY.set(false);
    LOGS_PAGE_READY.set(false);
    ABOUT_PAGE_READY.set(false);
    RESTORE_PAGE_READY.set(false);
}

pub fn register_application(app: &Application) {
    app.set_default();
    APP.with(|slot| {
        *slot.borrow_mut() = Some(app.downgrade());
    });
    APP_HOLD.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(app.hold());
        }
    });
}

fn application() -> Option<Application> {
    APP.with(|slot| slot.borrow().as_ref().and_then(|weak| weak.upgrade())).or_else(|| {
        gtk::Application::default()
            .downcast::<Application>()
            .ok()
    })
}

/// Show the main window, creating it on first launch.
pub fn present(app: &Application) {
    MAIN_WINDOW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(window) = slot.as_ref() {
            if window.is_drawable() {
                tracing::info!("show existing main window");
                show_window(window);
                return;
            }
            tracing::info!("main window slot stale, rebuilding");
            reset_lazy_pages();
            *slot = None;
        }
        tracing::info!("building main window (shell)");
        let window = build_main_window_shell(app);
        show_window(&window);
        *slot = Some(window.clone());
        glib::idle_add_local_once(begin_after_shell_visible);
        tracing::info!("main window shell visible");
    });
}

/// Tray + Dashboard-Daten erst nach dem ersten Frame (Hauptschleife bleibt frei).
pub fn begin_after_shell_visible() {
    if settings_apply::tray_icon_enabled() {
        icons::init_tray_pixmaps();
        TRAY_SPAWN_PENDING.set(true);
    }
    std::thread::spawn(|| {
        crate::daemon_lifecycle::ensure_daemon_running();
        let reachable = crate::daemon_lifecycle::is_daemon_reachable();
        crate::debug::log_daemon_status(reachable);
        // idle_add_once: sicher aus Hintergrund-Thread; idle_add_local nur auf der GTK-Hauptschleife.
        glib::idle_add_once(|| {
            DASHBOARD_PAGE.with(|slot| {
                if let Some(dashboard) = slot.borrow().as_ref() {
                    on_overview_shown();
                    refresh_dashboard(dashboard);
                }
            });
        });
    });
}

/// Called from the tray icon or its menu (may run on a non-GTK thread).
pub fn present_from_tray() {
    if !glib::MainContext::default().is_owner() {
        glib::MainContext::default().invoke(|| present_from_tray());
        return;
    }
    let Some(app) = application() else {
        tracing::warn!("tray: cannot open window — application not registered");
        return;
    };
    crate::debug::log_ui("present", "from tray");
    present(&app);
}

/// Fully restart BackupPilot (GUI and background daemon when installed).
pub fn restart_application() {
    restart_background_daemon();

    if let Ok(exe) = std::env::current_exe() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut command = std::process::Command::new(exe);
        command.args(&args);
        if let Ok(display) = std::env::var("DISPLAY") {
            command.env("DISPLAY", display);
        }
        if let Ok(xauth) = std::env::var("XAUTHORITY") {
            command.env("XAUTHORITY", xauth);
        }
        match command.spawn() {
            Ok(_) => quit_application(),
            Err(err) => tracing::warn!(%err, "failed to spawn BackupPilot for restart"),
        }
    }
}

fn restart_background_daemon() {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "try-restart", "backuppilot-daemon.service"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Quit the GUI and exit the process (tray menu „Beenden“).
pub fn quit_application() {
    editor::dismiss_any_editor_window();
    hide_main_window();

    APP_HOLD.with(|slot| {
        slot.borrow_mut().take();
    });

    if let Some(app) = application() {
        app.quit();
    }

    // The tray service runs on a background thread; exit explicitly.
    std::process::exit(0);
}

fn show_window(window: &ApplicationWindow) {
    window.set_visible(true);
    window.present();
}

fn hide_main_window() {
    reset_lazy_pages();
    MAIN_WINDOW.with(|slot| {
        if let Some(window) = slot.borrow_mut().take() {
            window.destroy();
        }
    });
}

/// Schneller erster Frame: nur Shell + Übersicht; schwere Seiten folgen per Idle.
fn build_main_window_shell(app: &Application) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title(&tr("BackupPilot"))
        .default_width(1100)
        .default_height(720)
        .build();
    icons::apply_window_icon(&window);

    let window_ref = window.clone();
    window.connect_close_request(move |_| {
        if settings_apply::close_to_tray_enabled() {
            hide_main_window();
            glib::Propagation::Stop
        } else {
            quit_application();
            glib::Propagation::Stop
        }
    });
    window_ref.connect_destroy(|_| {
        MAIN_WINDOW.with(|slot| {
            slot.borrow_mut().take();
        });
    });

    shell::load_styles();

    let toast_overlay = ToastOverlay::new();
    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    VIEW_STACK.with(|slot| {
        *slot.borrow_mut() = Some(stack.clone());
    });
    TOAST_OVERLAY.with(|slot| {
        *slot.borrow_mut() = Some(toast_overlay.clone());
    });

    tracing::info!("main window: dashboard");
    let dashboard = build_dashboard_page();
    DASHBOARD_PAGE.with(|slot| {
        *slot.borrow_mut() = Some(dashboard.clone());
    });

    stack.add_named(&dashboard, Some("dashboard"));
    stack.page(&dashboard).set_title(&tr("Overview"));
    add_stack_placeholder(&stack, "profiles", &tr("Profiles"));
    add_stack_placeholder(&stack, "restore", &tr("Restore"));
    add_stack_placeholder(&stack, "encryption", &tr("Encryption"));
    add_stack_placeholder(&stack, "settings", &tr("Settings"));
    add_stack_placeholder(&stack, "logs", &tr("Log"));
    add_stack_placeholder(&stack, "about", &tr("About"));

    shell::set_stack_page_icon(&stack, "dashboard", "view-dashboard-symbolic");
    shell::set_stack_page_icon(&stack, "profiles", "document-properties-symbolic");
    shell::set_stack_page_icon(&stack, "restore", "folder-download-symbolic");
    shell::set_stack_page_icon(&stack, "encryption", "security-high-symbolic");
    shell::set_stack_page_icon(&stack, "settings", "emblem-system-symbolic");
    shell::set_stack_page_icon(&stack, "logs", "text-x-generic-symbolic");
    shell::set_stack_page_icon(&stack, "about", "help-about-symbolic");

    toast_overlay.set_child(Some(&stack));

    let (split, window_title) = shell::build_main_layout(&stack, &toast_overlay);
    WINDOW_TITLE.with(|slot| {
        *slot.borrow_mut() = Some(window_title.clone());
    });
    shell::bind_page_titles(&stack, &window_title);

    stack.connect_visible_child_name_notify(on_stack_page_changed);

    window.set_content(Some(&split));

    stack.set_visible_child_name("dashboard");

    window
}

/// Feste Sidebar-Reihenfolge (GtkStackSidebar folgt der Stack-Kind-Reihenfolge).
const NAV_STACK_ORDER: &[&str] = &[
    "dashboard",
    "profiles",
    "restore",
    "encryption",
    "settings",
    "logs",
    "about",
];

fn restore_nav_stack_order(stack: &gtk::Stack) {
    let visible_name = stack.visible_child_name();

    struct PageEntry {
        name: String,
        child: gtk::Widget,
        title: Option<glib::GString>,
        icon: Option<glib::GString>,
    }

    let mut entries = Vec::new();
    for &name in NAV_STACK_ORDER {
        let Some(child) = stack.child_by_name(name) else {
            continue;
        };
        let page = stack.page(&child);
        let entry = PageEntry {
            name: name.to_string(),
            title: page.title(),
            icon: page.icon_name(),
            child,
        };
        stack.remove(&entry.child);
        entries.push(entry);
    }

    for entry in entries {
        stack.add_named(&entry.child, Some(&entry.name));
        let page = stack.page(&entry.child);
        if let Some(title) = entry.title {
            page.set_title(&title);
        }
        if let Some(icon) = entry.icon {
            page.set_icon_name(&icon);
        }
    }

    if let Some(name) = visible_name {
        stack.set_visible_child_name(&name);
    }
}

fn add_stack_placeholder(stack: &gtk::Stack, name: &str, title: &str) {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();
    page.set_widget_name(&format!("{name}-placeholder"));
    let label = gtk::Label::builder()
        .label(&tr("Loading…"))
        .css_classes(["dim-label"])
        .build();
    page.append(&label);
    stack.add_named(&page, Some(name));
    stack.page(&page).set_title(title);
}

fn replace_stack_page(stack: &gtk::Stack, name: &str, widget: &gtk::Widget, title: &str) {
    SUPPRESS_STACK_PAGE_NOTIFY.set(true);
    let icon = stack
        .child_by_name(name)
        .and_then(|old| stack.page(&old).icon_name());
    if let Some(old) = stack.child_by_name(name) {
        stack.remove(&old);
    }
    stack.add_named(widget, Some(name));
    let page = stack.page(widget);
    page.set_title(title);
    if let Some(icon) = icon {
        page.set_icon_name(&icon);
    }
    restore_nav_stack_order(stack);
    SUPPRESS_STACK_PAGE_NOTIFY.set(false);
}

fn on_stack_page_changed(stack: &gtk::Stack) {
    if SUPPRESS_STACK_PAGE_NOTIFY.get() {
        return;
    }
    let page = stack.visible_child_name().unwrap_or_default();
    crate::debug::log_ui("stack page", &page);
    match stack.visible_child_name().as_deref() {
        Some("dashboard") => {
            on_overview_shown();
            DASHBOARD_PAGE.with(|slot| {
                if let Some(page) = slot.borrow().as_ref() {
                    refresh_dashboard(page);
                }
            });
        }
        Some("profiles") => {
            on_overview_hidden();
            schedule_profiles_page();
        }
        Some("restore") => {
            on_overview_hidden();
            schedule_restore_page();
        }
        Some("logs") => {
            on_overview_hidden();
            schedule_logs_page();
        }
        Some("encryption") => {
            on_overview_hidden();
            schedule_encryption_page();
        }
        Some("settings") => {
            on_overview_hidden();
            schedule_settings_page();
        }
        Some("about") => {
            on_overview_hidden();
            schedule_about_page();
        }
        _ => on_overview_hidden(),
    }
}

fn deferred_stack_and_chrome() -> Option<(gtk::Stack, ApplicationWindow, ToastOverlay)> {
    let stack = VIEW_STACK.with(|slot| slot.borrow().clone())?;
    let window = main_window()?;
    let toast = toast_overlay()?;
    Some((stack, window, toast))
}

fn schedule_profiles_page() {
    if PROFILES_PAGE_READY.get() {
        refresh_profiles_public();
        return;
    }
    idle_build_low_priority(|| {
        ensure_profiles_page_built();
        refresh_profiles_public();
    });
}

fn schedule_encryption_page() {
    if ENCRYPTION_PAGE_READY.get() {
        encryption_keys::refresh();
        return;
    }
    idle_build_low_priority(|| {
        ensure_encryption_page_built();
        encryption_keys::refresh();
    });
}

fn schedule_settings_page() {
    if SETTINGS_PAGE_READY.get() {
        return;
    }
    idle_build_low_priority(ensure_settings_page_built);
}

fn schedule_logs_page() {
    if LOGS_PAGE_READY.get() {
        logs::refresh();
        return;
    }
    idle_build_low_priority(|| {
        ensure_logs_page_built();
        logs::refresh();
    });
}

fn schedule_about_page() {
    if ABOUT_PAGE_READY.get() {
        return;
    }
    idle_build_low_priority(ensure_about_page_built);
}

fn schedule_restore_page() {
    if RESTORE_PAGE_READY.get() {
        restore::refresh();
        return;
    }
    idle_build_low_priority(|| {
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ensure_restore_page_built();
        }));
        if built.is_err() {
            tracing::error!("restore page build panicked");
            return;
        }
        restore::refresh();
    });
}

/// Schwere Seitenaufbauten mit niedriger Idle-Priorität, damit Klicks zuerst verarbeitet werden.
fn idle_build_low_priority(build: impl FnOnce() + 'static) {
    let mut build = Some(build);
    glib::idle_add_local_full(glib::Priority::LOW, move || {
        if let Some(f) = build.take() {
            f();
        }
        glib::ControlFlow::Break
    });
}

fn ensure_profiles_page_built() {
    if PROFILES_PAGE_READY.get() {
        return;
    }
    let Some((stack, window, toast)) = deferred_stack_and_chrome() else {
        return;
    };
    tracing::info!("main window: profiles (on demand)");
    let profiles_page = profiles::build_page(&window, &toast);
    PROFILES_PAGE.with(|slot| {
        *slot.borrow_mut() = Some(profiles_page.clone());
    });
    replace_stack_page(&stack, "profiles", &profiles_page, &tr("Profiles"));
    PROFILES_PAGE_READY.set(true);
}

fn ensure_encryption_page_built() {
    if ENCRYPTION_PAGE_READY.get() {
        return;
    }
    let Some((stack, window, toast)) = deferred_stack_and_chrome() else {
        return;
    };
    tracing::info!("main window: encryption (on demand)");
    let encryption_page = encryption_keys::build_page(&window, &toast);
    replace_stack_page(&stack, "encryption", &encryption_page, &tr("Encryption"));
    ENCRYPTION_PAGE_READY.set(true);
}

fn ensure_settings_page_built() {
    if SETTINGS_PAGE_READY.get() {
        return;
    }
    let Some((stack, window, toast)) = deferred_stack_and_chrome() else {
        return;
    };
    tracing::info!("main window: settings (on demand)");
    let settings_page = settings::build_page(&window, &toast);
    replace_stack_page(&stack, "settings", &settings_page, &tr("Settings"));
    SETTINGS_PAGE_READY.set(true);
}

fn ensure_logs_page_built() {
    if LOGS_PAGE_READY.get() {
        return;
    }
    let Some((stack, window, toast)) = deferred_stack_and_chrome() else {
        return;
    };
    tracing::info!("main window: logs (on demand)");
    let logs_page = logs::build_page(&window, &toast);
    replace_stack_page(&stack, "logs", &logs_page, &tr("Log"));
    LOGS_PAGE_READY.set(true);
}

fn ensure_about_page_built() {
    if ABOUT_PAGE_READY.get() {
        return;
    }
    let Some((stack, window, _toast)) = deferred_stack_and_chrome() else {
        return;
    };
    tracing::info!("main window: about (on demand)");
    let about_page = about::build_page(&window);
    replace_stack_page(&stack, "about", &about_page, &tr("About"));
    ABOUT_PAGE_READY.set(true);
}

/// Wiederherstellung ist sehr schwer — nur bei erstem Besuch bauen.
fn ensure_restore_page_built() {
    if RESTORE_PAGE_READY.get() {
        return;
    }
    let Some((stack, window, toast)) = deferred_stack_and_chrome() else {
        return;
    };
    tracing::info!("main window: restore (on demand)");
    let restore_page = restore::build_page(&window, &toast);
    replace_stack_page(&stack, "restore", &restore_page, &tr("Restore"));
    RESTORE_PAGE_READY.set(true);
}

fn build_dashboard_page() -> gtk::Widget {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .vexpand(true)
        .hexpand(true)
        .build();

    let status_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Backup jobs"))
        .build();
    let status_box = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    status_box.set_widget_name("dashboard-status-list");
    status_group.add(&status_box);

    let activity_group = libadwaita::PreferencesGroup::builder()
        .title(&tr("Recent activity"))
        .description(&tr("Latest backup events. Click a row for details."))
        .build();
    let view_all_btn = gtk::Button::builder()
        .label(&tr("View all"))
        .css_classes(["flat"])
        .build();
    view_all_btn.connect_clicked(|_| switch_to_logs());
    activity_group.set_header_suffix(Some(&view_all_btn));

    let activity_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    activity_list.set_widget_name("dashboard-activity-list");
    activity_group.add(&activity_list);

    DASHBOARD_STATUS_LIST.with(|slot| {
        *slot.borrow_mut() = Some(status_box.clone());
    });
    DASHBOARD_ACTIVITY_LIST.with(|slot| {
        *slot.borrow_mut() = Some(activity_list.clone());
    });

    page.append(&pbs_client_setup::build_placeholder());
    page.append(&onboarding::build_placeholder());
    page.append(&build_health_summary_placeholder());
    page.append(&build_mount_banner_placeholder());
    page.append(&build_update_hint_placeholder());
    page.append(&status_group);
    page.append(&activity_group);
    page.set_widget_name("dashboard-page");

    page.upcast()
}

pub fn main_window() -> Option<ApplicationWindow> {
    MAIN_WINDOW.with(|slot| slot.borrow().clone())
}

pub fn toast_overlay() -> Option<ToastOverlay> {
    TOAST_OVERLAY.with(|slot| slot.borrow().clone())
}

/// Rebuild visible UI after the user changes the interface language in settings.
pub fn reload_ui_language() {
    settings_apply::apply_loaded();
    editor::dismiss_any_editor_window();

    let Some((stack, window, _toast)) = deferred_stack_and_chrome() else {
        return;
    };

    let visible = stack.visible_child_name();

    update_all_navigation_titles(&stack);
    window.set_title(Some(&tr("BackupPilot")));

    if let Some(wt) = WINDOW_TITLE.with(|slot| slot.borrow().clone()) {
        let name = visible.as_deref().unwrap_or("dashboard");
        wt.set_title(&shell::page_title(name));
        wt.set_subtitle(&shell::page_subtitle(name));
    }

    rebuild_dashboard_stack_page(&stack);

    if PROFILES_PAGE_READY.get() {
        PROFILES_PAGE_READY.set(false);
        PROFILES_PAGE.with(|slot| *slot.borrow_mut() = None);
        ensure_profiles_page_built();
        refresh_profiles_public();
    }
    if RESTORE_PAGE_READY.get() {
        RESTORE_PAGE_READY.set(false);
        ensure_restore_page_built();
        restore::refresh();
    }
    if ENCRYPTION_PAGE_READY.get() {
        ENCRYPTION_PAGE_READY.set(false);
        ensure_encryption_page_built();
        encryption_keys::refresh();
    }
    if SETTINGS_PAGE_READY.get() {
        SETTINGS_PAGE_READY.set(false);
        settings::clear_widgets();
        ensure_settings_page_built();
    }
    if LOGS_PAGE_READY.get() {
        LOGS_PAGE_READY.set(false);
        ensure_logs_page_built();
        logs::refresh();
    }
    if ABOUT_PAGE_READY.get() {
        ABOUT_PAGE_READY.set(false);
        ensure_about_page_built();
    }

    if let Some(name) = visible {
        stack.set_visible_child_name(&name);
    }
}

fn update_all_navigation_titles(stack: &gtk::Stack) {
    for name in NAV_STACK_ORDER {
        if let Some(child) = stack.child_by_name(name) {
            stack.page(&child).set_title(&shell::page_title(name));
        }
    }
}

fn rebuild_dashboard_stack_page(stack: &gtk::Stack) {
    let on_dashboard = is_dashboard_visible();
    if on_dashboard {
        on_overview_hidden();
    }
    let new_page = build_dashboard_page();
    DASHBOARD_PAGE.with(|slot| {
        *slot.borrow_mut() = Some(new_page.clone());
    });
    replace_stack_page(stack, "dashboard", &new_page, &tr("Overview"));
    if on_dashboard {
        on_overview_shown();
        refresh_dashboard(&new_page);
    }
}

pub fn refresh_profiles_public() {
    let (window, toast, page) = (
        main_window(),
        toast_overlay(),
        PROFILES_PAGE.with(|s| s.borrow().clone()),
    );
    if let (Some(w), Some(t), Some(p)) = (window, toast, page) {
        profiles::refresh_list(&p, &w, &t);
    }
}

fn on_overview_shown() {
    start_dashboard_poll();
}

fn on_overview_hidden() {
    stop_dashboard_poll();
}

fn start_dashboard_poll() {
    stop_dashboard_poll();
    let source = glib::timeout_add_seconds_local(15, move || {
        refresh_dashboard_public();
        glib::ControlFlow::Continue
    });
    DASHBOARD_POLL.with(|slot| {
        *slot.borrow_mut() = Some(source);
    });
}

fn stop_dashboard_poll() {
    DASHBOARD_POLL.with(|slot| {
        if let Some(id) = slot.borrow_mut().take() {
            id.remove();
        }
    });
}

fn switch_to_page(page_name: &str) {
    VIEW_STACK.with(|slot| {
        if let Some(stack) = slot.borrow().as_ref() {
            stack.set_visible_child_name(page_name);
        }
    });
}

/// Switch sidebar to Overview and refresh status (e.g. after starting a backup).
pub fn switch_to_overview() {
    switch_to_page("dashboard");
    on_overview_shown();
    DASHBOARD_PAGE.with(|slot| {
        if let Some(dashboard) = slot.borrow().as_ref() {
            refresh_dashboard(dashboard);
        }
    });
}

pub fn switch_to_profiles() {
    switch_to_page("profiles");
    schedule_profiles_page();
}

pub fn switch_to_restore() {
    switch_to_page("restore");
    schedule_restore_page();
}

pub fn switch_to_logs() {
    switch_to_page("logs");
    schedule_logs_page();
}

fn is_dashboard_visible() -> bool {
    VIEW_STACK.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|s| s.visible_child_name())
            .map(|n| n == "dashboard")
            .unwrap_or(false)
    })
}

fn refresh_dashboard(widget: &gtk::Widget) {
    if DASHBOARD_REFRESH_IN_FLIGHT.get() {
        DASHBOARD_REFRESH_PENDING.set(true);
        crate::debug::log_ui("refresh", "dashboard deferred (apply in progress)");
        return;
    }
    DASHBOARD_REFRESH_IN_FLIGHT.set(true);
    DASHBOARD_REFRESH_PENDING.set(false);
    crate::debug::log_ui("refresh", "dashboard");
    let widget = widget.clone();
    dbus_runtime::spawn(
        async move { fetch_dashboard_data().await },
        move |result| {
            glib::idle_add_local_once(move || apply_dashboard_refresh_result(&widget, result));
        },
    );
}

fn finish_dashboard_apply() {
    DASHBOARD_REFRESH_IN_FLIGHT.set(false);
    if DASHBOARD_REFRESH_PENDING.get() {
        DASHBOARD_REFRESH_PENDING.set(false);
        DASHBOARD_PAGE.with(|slot| {
            if let Some(dashboard) = slot.borrow().as_ref() {
                refresh_dashboard(dashboard);
            }
        });
    }
}

fn maybe_spawn_tray_after_dashboard() {
    if TRAY_SPAWN_PENDING.get() {
        TRAY_SPAWN_PENDING.set(false);
        if settings_apply::tray_icon_enabled() {
            tray::spawn();
        }
    }
    finish_dashboard_apply();
}

fn apply_dashboard_refresh_result(widget: &gtk::Widget, result: Result<DashboardData, zbus::Error>) {
    match result {
        Err(err) => {
            tracing::warn!(%err, "dashboard refresh failed");
            show_dashboard_daemon_error(widget, &err.to_string());
            maybe_spawn_tray_after_dashboard();
        }
        Ok(data) => {
            crate::debug::log_ui(
                "apply dashboard",
                format!(
                    "jobs={} activity={} mounts={}",
                    data.statuses.len(),
                    data.activity.len(),
                    data.active_mounts.len()
                ),
            );
            let widget = widget.clone();
            let data = Rc::new(data);
            glib::idle_add_local_once(move || apply_dashboard_apply_data(&widget, data));
        }
    }
}

/// Ein Idle-Zyklus für Banner/Meta/Listen — verhindert parallele Apply-Ketten (Hänger bei ~/.local).
fn apply_dashboard_apply_data(widget: &gtk::Widget, data: Rc<DashboardData>) {
    let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let parent = main_window();
        let toast = toast_overlay();
        if let (Some(parent), Some(toast)) = (parent, toast) {
            pbs_client_setup::update_dashboard_banner(
                widget,
                &parent,
                Some(&toast),
                data.pbs_available,
            );
            onboarding::update_dashboard(
                widget,
                &parent,
                data.statuses.len(),
                data.onboarding_completed,
            );
        }
    }));
    if step.is_err() {
        tracing::error!("dashboard apply step banners panicked");
        maybe_spawn_tray_after_dashboard();
        return;
    }
    crate::debug::log_ui("apply dashboard", "step banners done");

    let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        apply_health_summary(widget, &data.statuses, data.pause_all_backups);
        apply_dashboard_mounts(widget, &data.active_mounts);
        apply_update_hint(widget);
    }));
    if step.is_err() {
        tracing::error!("dashboard apply step meta panicked");
        maybe_spawn_tray_after_dashboard();
        return;
    }
    crate::debug::log_ui("apply dashboard", "step meta done");

    crate::debug::log_ui("apply dashboard", "step lists begin");
    let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        apply_statuses_to_dashboard_widget(widget, &data.statuses);
    }));
    if step.is_err() {
        tracing::error!("dashboard apply step lists (jobs) panicked");
        maybe_spawn_tray_after_dashboard();
        return;
    }
    crate::debug::log_ui("apply dashboard", "step lists jobs done");

    if data.activity.is_empty() {
        apply_activity_to_dashboard_widget(widget, &[]);
        crate::debug::log_ui("apply dashboard", "step lists done");
        maybe_spawn_tray_after_dashboard();
        return;
    }
    clear_dashboard_activity_list(widget);
    apply_activity_rows_incremental(widget, data, 0);
}

fn apply_activity_rows_incremental(widget: &gtk::Widget, data: Rc<DashboardData>, index: usize) {
    if index >= data.activity.len() {
        crate::debug::log_ui("apply dashboard", "step lists done");
        maybe_spawn_tray_after_dashboard();
        return;
    }

    let widget = widget.to_owned();
    let entries = data.activity.clone();
    glib::idle_add_local_once(move || {
        let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            append_one_activity_row(&widget, &entries[index]);
        }));
        if step.is_err() {
            tracing::error!(index, "dashboard activity row panicked");
            maybe_spawn_tray_after_dashboard();
            return;
        }
        apply_activity_rows_incremental(&widget, data, index + 1);
    });
}

fn append_one_activity_row(widget: &gtk::Widget, entry: &ActivityLogEntry) {
    let list = DASHBOARD_ACTIVITY_LIST.with(|slot| slot.borrow().clone()).or_else(|| {
        find_child_by_name(widget, "dashboard-activity-list")
            .and_then(|w| w.downcast::<gtk::ListBox>().ok())
    });
    let Some(list) = list else {
        return;
    };
    let parent = main_window();
    list.append(&activity_log::build_activity_row(
        entry,
        parent.as_ref(),
        ActivityRowMode::DASHBOARD,
    ));
}

pub fn refresh_dashboard_public() {
    if !is_dashboard_visible() {
        crate::debug::log_ui("refresh", "dashboard skipped (not visible)");
        return;
    }
    DASHBOARD_PAGE.with(|slot| {
        if let Some(dashboard) = slot.borrow().as_ref() {
            refresh_dashboard(dashboard);
        }
    });
}

fn clear_dashboard_activity_list(widget: &gtk::Widget) {
    let list = DASHBOARD_ACTIVITY_LIST.with(|slot| slot.borrow().clone()).or_else(|| {
        find_child_by_name(widget, "dashboard-activity-list")
            .and_then(|w| w.downcast::<gtk::ListBox>().ok())
    });
    if let Some(list) = list {
        clear_list_box(&list);
    }
}

fn apply_activity_to_dashboard_widget(widget: &gtk::Widget, entries: &[ActivityLogEntry]) {
    let list = DASHBOARD_ACTIVITY_LIST.with(|slot| slot.borrow().clone()).or_else(|| {
        find_child_by_name(widget, "dashboard-activity-list")
            .and_then(|w| w.downcast::<gtk::ListBox>().ok())
    });
    let Some(list) = list else {
        tracing::warn!("dashboard activity list not found");
        return;
    };
    clear_list_box(&list);
    if entries.is_empty() {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::builder()
            .label(&tr("No backup activity yet. Once a backup runs, you will see a short summary here."))
            .xalign(0.0)
            .wrap(true)
            .margin_start(12)
            .margin_end(12)
            .margin_top(10)
            .margin_bottom(10)
            .css_classes(["dim-label"])
            .build();
        row.set_child(Some(&label));
        list.append(&row);
    }
}

fn show_dashboard_daemon_error(widget: &gtk::Widget, err: &str) {
    let list = find_child_by_name(widget, "dashboard-status-list");
    let Some(list) = list.and_then(|w| w.downcast::<gtk::ListBox>().ok()) else {
        return;
    };
    clear_list_box(&list);
    let row = gtk::ListBoxRow::new();
    let label = gtk::Label::builder()
        .label(&tr_fmt(
            "Daemon not reachable: {err}",
            &[("err", err)],
        ))
        .xalign(0.0)
        .wrap(true)
        .margin_start(12)
        .margin_end(12)
        .margin_top(10)
        .margin_bottom(10)
        .css_classes(["dim-label", "error"])
        .build();
    row.set_child(Some(&label));
    list.append(&row);
}

fn apply_statuses_to_dashboard_widget(widget: &gtk::Widget, statuses: &[ProfileStatus]) {
    let list = DASHBOARD_STATUS_LIST.with(|slot| slot.borrow().clone()).or_else(|| {
        find_child_by_name(widget, "dashboard-status-list")
            .and_then(|w| w.downcast::<gtk::ListBox>().ok())
    });

    if let Some(list) = list {
        crate::debug::log_ui("apply dashboard", "step lists clear jobs");
        clear_list_box(&list);
        let mut jobs: Vec<&ProfileStatus> = statuses.iter().collect();
        jobs.sort_by(|a, b| {
            match (a.backup_in_progress, b.backup_in_progress) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.profile_name.cmp(&b.profile_name),
            }
        });
        if jobs.is_empty() {
            let row = gtk::ListBoxRow::new();
            let label = gtk::Label::new(Some(&tr("No backup profiles configured yet.")));
            label.set_xalign(0.0);
            label.set_margin_start(12);
            label.set_margin_end(12);
            label.set_margin_top(10);
            label.set_margin_bottom(10);
            row.set_child(Some(&label));
            list.append(&row);
        } else {
            let any_running = jobs.iter().any(|s| s.backup_in_progress);
            if any_running {
                list.append(&cancel_all_backups_row());
            }
            for (i, status) in jobs.iter().enumerate() {
                crate::debug::log_ui("apply dashboard", format!("step lists job row {i}"));
                list.append(&profile_job_row(status));
            }
        }
        crate::debug::log_ui("apply dashboard", "step lists jobs built");
    } else {
        tracing::warn!("dashboard status list not found");
    }
}

fn cancel_all_backups_row() -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let action = libadwaita::ActionRow::builder()
        .title(&tr("Cancel all running backups"))
        .subtitle(&tr("Stops every active backup immediately and frees network bandwidth."))
        .activatable(true)
        .build();
    action.add_suffix(
        &gtk::Label::builder()
            .label("⏹")
            .xalign(0.5)
            .margin_end(4)
            .build(),
    );
    action.connect_activated(|_| {
        crate::backup_actions::cancel_all_running_backups(toast_overlay().as_ref());
    });
    row.set_child(Some(&action));
    row
}

pub fn in_progress_status_text(status: &ProfileStatus) -> String {
    let duration = status
        .last_run
        .as_ref()
        .map(|run| format_duration_since(run.started_at));

    if let Some(detail) = status.progress_message.as_deref() {
        if let Some(duration) = duration {
            return tr_fmt(
                "Backup in progress ({duration}): {detail}",
                &[("duration", &duration), ("detail", detail)],
            );
        }
        return tr_fmt("Backup in progress: {detail}", &[("detail", detail)]);
    }

    if let Some(duration) = duration {
        return tr_fmt(
            "Backup in progress ({duration})",
            &[("duration", &duration)],
        );
    }

    tr("Backup in progress…")}

fn profile_job_row(status: &ProfileStatus) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let subtitle = job_row_subtitle(status);

    let action = libadwaita::ActionRow::builder()
        .title(&status.profile_name)
        .subtitle(&subtitle)
        .activatable(false)
        .build();

    if status.backup_in_progress {
        action.add_prefix(&status_prefix_label("accent"));

        let stop_btn = gtk::Button::builder()
            .label("⏹")
            .tooltip_text(&tr("Cancel backup"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "destructive-action"])
            .build();
        let profile_id = status.profile_id;
        stop_btn.connect_clicked(move |_| {
            crate::backup_actions::cancel_backup(profile_id, None, None);
        });
        action.add_suffix(&stop_btn);
    } else {
        let (_icon, css) = job_row_icon(status);
        action.add_prefix(&status_prefix_label(css));
    }

    row.set_child(Some(&action));
    row
}

fn job_row_subtitle(status: &ProfileStatus) -> String {
    if status.backup_in_progress {
        return in_progress_status_text(status);
    }
    if !status.enabled {
        return tr("Disabled , scheduled backups are paused");
    }
    if let Some(run) = &status.last_run {
        return activity_log::run_summary_subtitle(run);
    }
    tr("No backup run yet")}

fn job_row_icon(status: &ProfileStatus) -> (&'static str, &'static str) {
    if let Some(run) = &status.last_run {
        return match run.status {
            backuppilot_core::profile::RunStatus::Success => ("emblem-ok-symbolic", "success"),
            backuppilot_core::profile::RunStatus::Failed => ("dialog-error-symbolic", "error"),
            backuppilot_core::profile::RunStatus::Skipped => ("dialog-warning-symbolic", "warning"),
            backuppilot_core::profile::RunStatus::Cancelled => ("process-stop-symbolic", "warning"),
            backuppilot_core::profile::RunStatus::Running | backuppilot_core::profile::RunStatus::Pending => {
                ("view-refresh-symbolic", "accent")
            }
        };
    }
    match status.health {
        HealthState::Ok => ("emblem-ok-symbolic", "success"),
        HealthState::Warning => ("dialog-warning-symbolic", "warning"),
        HealthState::Critical => ("dialog-error-symbolic", "error"),
        HealthState::Unknown => ("help-about-symbolic", "dim-label"),
    }
}

pub fn run_status_label(run: &backuppilot_core::profile::BackupRun) -> String {
    use backuppilot_core::profile::RunStatus;
    match run.status {
        RunStatus::Pending => tr("pending"),
        RunStatus::Running => tr("running"),
        RunStatus::Success => tr("successful"),
        RunStatus::Failed => tr("failed"),
        RunStatus::Skipped => tr("skipped"),
        RunStatus::Cancelled => tr("cancelled"),
    }
}

fn build_health_summary_placeholder() -> gtk::Widget {
    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_bottom(4)
        .visible(false)
        .build();
    box_.set_widget_name("dashboard-health-summary");
    box_.upcast()
}

fn build_mount_banner_placeholder() -> gtk::Widget {
    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .visible(false)
        .build();
    box_.set_widget_name("dashboard-mount-banner");
    box_.upcast()
}

fn build_update_hint_placeholder() -> gtk::Widget {
    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .visible(false)
        .build();
    box_.set_widget_name("dashboard-update-hint");
    box_.upcast()
}

async fn fetch_dashboard_data() -> zbus::Result<DashboardData> {
    let proxy = match connect().await {
        Ok(proxy) => proxy,
        Err(_) => {
            crate::daemon_lifecycle::ensure_daemon_running();
            connect().await?
        }
    };
    let pbs_available = proxy.pbs_client_available().await?;
    let statuses = dbus_client::list_statuses(&proxy).await?;
    const DASHBOARD_ACTIVITY_LIMIT: u32 = 6;
    let activity = dbus_client::list_recent_activity(&proxy, DASHBOARD_ACTIVITY_LIMIT).await?;
    let onboarding_done = dbus_client::is_onboarding_completed(&proxy).await?;
    let pause_all = dbus_client::pause_all_backups(&proxy).await?;
    let active_mounts = dbus_client::list_active_mounts(&proxy).await.unwrap_or_default();
    Ok(DashboardData {
        pbs_available,
        statuses,
        activity,
        onboarding_completed: onboarding_done,
        pause_all_backups: pause_all,
        active_mounts,
    })
}

#[derive(Clone)]
struct DashboardData {
    pbs_available: bool,
    statuses: Vec<ProfileStatus>,
    activity: Vec<ActivityLogEntry>,
    onboarding_completed: bool,
    pause_all_backups: bool,
    active_mounts: Vec<ActiveMount>,
}

fn apply_dashboard_mounts(widget: &gtk::Widget, mounts: &[ActiveMount]) {
    let Some(parent) = main_window() else {
        return;
    };
    let Some(container) = find_child_by_name(widget, "dashboard-mount-banner") else {
        return;
    };
    let Some(container) = container.downcast_ref::<gtk::Box>() else {
        return;
    };
    restore::mount_ui::apply_dashboard_mount_banners(container, mounts, &parent);
}

fn apply_health_summary(widget: &gtk::Widget, statuses: &[ProfileStatus], pause_all: bool) {
    let Some(container) = find_child_by_name(widget, "dashboard-health-summary") else {
        return;
    };
    let Some(container) = container.downcast_ref::<gtk::Box>() else {
        return;
    };
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let mut warn = 0u32;
    let mut critical = 0u32;
    for s in statuses {
        if !s.enabled {
            continue;
        }
        match s.health {
            HealthState::Warning => warn += 1,
            HealthState::Critical => critical += 1,
            _ => {}
        }
    }

    if pause_all {
        let callout = profiles::build_info_callout_with_icon(
            "media-playback-pause-symbolic",
            None,
            &tr("Scheduled backups are paused"),
            &tr("Automatic backups will not start until you resume them in Settings."),
        );
        container.append(&callout);
        container.set_visible(true);
        return;
    }

    if critical == 0 && warn == 0 {
        container.set_visible(false);
        return;
    }

    let (icon, frame_class, title, body) = health_attention_callout_copy(warn, critical);
    let callout =
        profiles::build_info_callout_with_icon(icon, Some(frame_class), &title, &body);
    container.append(&callout);
    container.set_visible(true);
}

fn health_attention_callout_copy(
    warn: u32,
    critical: u32,
) -> (&'static str, &'static str, String, String) {
    let body = tr(
        "Based on the warning and critical thresholds in each profile. Check the jobs below, run a backup, or adjust thresholds in Settings.",
    );

    if critical > 0 && warn > 0 {
        return (
            "dialog-error-symbolic",
            "health-critical",
            tr_fmt(
                "{critical} critically overdue, {warn} need attention",
                &[
                    ("critical", &critical.to_string()),
                    ("warn", &warn.to_string()),
                ],
            ),
            body,
        );
    }

    if critical > 0 {
        return (
            "dialog-error-symbolic",
            "health-critical",
            tr_fmt(
                "{count} backup job(s) critically overdue",
                &[("count", &critical.to_string())],
            ),
            body,
        );
    }

    (
        "dialog-warning-symbolic",
        "health-warning",
        tr_fmt(
            "{count} backup job(s) need attention",
            &[("count", &warn.to_string())],
        ),
        body,
    )
}

fn apply_update_hint(widget: &gtk::Widget) {
    if !backuppilot_core::app_update_checks_enabled() {
        if let Some(container) = find_child_by_name(widget, "dashboard-update-hint") {
            container.set_visible(false);
        }
        return;
    }
    let Some(container) = find_child_by_name(widget, "dashboard-update-hint") else {
        return;
    };
    let Some(container) = container.downcast_ref::<gtk::Box>() else {
        return;
    };
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let state = load_update_state();
    let Some(avail) = state.available.as_ref() else {
        container.set_visible(false);
        return;
    };
    if !is_update_newer_than_installed(avail) {
        container.set_visible(false);
        return;
    }

    let row = libadwaita::ActionRow::builder()
        .title(&tr("Application update available"))
        .subtitle(&tr_fmt(
            "Version {version} is available.",
            &[("version", &avail.version)],
        ))
        .activatable(true)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("software-update-available-symbolic"));
    row.connect_activated(|_| {
        present_from_tray();
        crate::updates::present_pending_update_if_any();
    });
    container.append(&row);
    container.set_visible(true);
}

pub fn application_id() -> &'static str {
    APP_ID
}
