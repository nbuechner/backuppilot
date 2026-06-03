use std::cell::RefCell;
use std::path::{Path, PathBuf};

use backuppilot_core::app_settings::{load_app_settings, ColorScheme};
use backuppilot_i18n::{tr, tr_fmt};

use crate::updates;
use gtk::glib;
use gtk::prelude::*;
use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, StyleManager};

/// Maximum wordmark width on the About page (px).
const LOGO_MAX_WIDTH: i32 = 160;

const WEBSITE: &str = "https://www.onesystems.ch";
const SUPPORT_URL: &str = "https://my.onesystems.ch/submitticket.php";

thread_local! {
    static ABOUT_LOGO: RefCell<Option<glib::WeakRef<gtk::Picture>>> = const { RefCell::new(None) };
}

/// Update the About-page wordmark after appearance settings change.
pub fn refresh_about_logo_if_visible() {
    ABOUT_LOGO.with(|slot| {
        if let Some(weak) = slot.borrow().as_ref() {
            refresh_logo_picture(weak);
        }
    });
}

pub fn build_page(parent: &ApplicationWindow) -> gtk::Widget {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .valign(gtk::Align::Start)
        .build();

    if let Some(logo_path) = branding_logo_path() {
        page.append(&build_logo_banner(&logo_path));
    }

    let info_group = libadwaita::PreferencesGroup::new();

    info_group.add(
        &libadwaita::ActionRow::builder()
            .title(&tr("Version"))
            .subtitle(&version_subtitle())
            .activatable(false)
            .build(),
    );

    page.append(&info_group);
    if backuppilot_core::app_update_checks_enabled() {
        page.append(&build_updates_group(parent));
    }

    let legal_group = libadwaita::PreferencesGroup::new();
    legal_group.add(
        &libadwaita::ActionRow::builder()
            .title(&tr("Copyright"))
            .subtitle(&copyright_line())
            .activatable(false)
            .build(),
    );

    legal_group.add(
        &libadwaita::ActionRow::builder()
            .title(&tr("License"))
            .subtitle(&tr("GNU Affero General Public License v3.0 or later"))
            .activatable(false)
            .build(),
    );

    let website_row = libadwaita::ActionRow::builder()
        .title(&tr("Website"))
        .subtitle(WEBSITE)
        .activatable(true)
        .build();
    website_row.add_suffix(&gtk::Image::from_icon_name("external-link-symbolic"));
    let parent_web = parent.clone();
    website_row.connect_activated(move |_| open_uri(&parent_web, WEBSITE));
    legal_group.add(&website_row);

    let support_row = libadwaita::ActionRow::builder()
        .title(&tr("Support"))
        .subtitle(&tr("Open support ticket"))
        .activatable(true)
        .build();
    support_row.add_suffix(&gtk::Image::from_icon_name("external-link-symbolic"));
    let parent_sup = parent.clone();
    support_row.connect_activated(move |_| open_uri(&parent_sup, SUPPORT_URL));
    legal_group.add(&support_row);

    page.append(&legal_group);

    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&page)
        .build()
        .upcast()
}

fn build_updates_group(parent: &ApplicationWindow) -> libadwaita::PreferencesGroup {
    let group = libadwaita::PreferencesGroup::new();

    let updates_row = libadwaita::ActionRow::builder()
        .title(&tr("Check for updates"))
        .subtitle(&tr("Tap to search for a newer version."))
        .activatable(true)
        .build();
    updates_row.add_suffix(&gtk::Image::from_icon_name("system-software-update-symbolic"));
    updates::register_about_status_row(&updates_row);
    updates::connect_check_button(&updates_row);
    group.add(&updates_row);

    let state = backuppilot_core::load_update_state();
    if let Some(av) = state.available.as_ref() {
        if backuppilot_core::is_update_newer_than_installed(av) {
            let install_row = libadwaita::ActionRow::builder()
                .title(&tr("View available update"))
                .subtitle(&tr_fmt(
                    "Version {version}",
                    &[("version", &av.version)],
                ))
                .activatable(true)
                .build();
            install_row.add_suffix(&gtk::Image::from_icon_name("package-x-generic-symbolic"));
            let av_clone = av.clone();
            let parent_install = parent.clone();
            install_row.connect_activated(move |_| {
                updates::present_update_dialog(&parent_install, av_clone.clone());
            });
            group.add(&install_row);
        }
    }

    let releases_row = libadwaita::ActionRow::builder()
        .title(&tr("Release page"))
        .subtitle(backuppilot_core::GITLAB_PROJECT_URL)
        .activatable(true)
        .build();
    releases_row.add_suffix(&gtk::Image::from_icon_name("external-link-symbolic"));
    let parent_rel = parent.clone();
    releases_row.connect_activated(move |_| updates::open_release_page(&parent_rel));
    group.add(&releases_row);

    group
}

/// Wordmark scaled to at most [`LOGO_MAX_WIDTH`], centered above the about content.
fn build_logo_banner(logo_path: &Path) -> gtk::Widget {
    let (width, height) = scaled_logo_size(logo_path);

    let picture = gtk::Picture::for_filename(logo_path);
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Contain);
    apply_logo_picture_size(&picture, logo_path);
    picture.set_halign(gtk::Align::Fill);
    picture.set_valign(gtk::Align::Fill);
    picture.set_hexpand(false);
    picture.set_vexpand(false);

    let banner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .hexpand(false)
        .vexpand(false)
        .width_request(width)
        .height_request(height)
        .margin_bottom(12)
        .build();
    banner.append(&picture);

    let picture = picture.downgrade();
    ABOUT_LOGO.with(|slot| {
        *slot.borrow_mut() = Some(picture.clone());
    });

    if let Some(display) = gtk::gdk::Display::default() {
        let style = StyleManager::for_display(&display);
        style.connect_dark_notify(move |_| {
            refresh_logo_picture(&picture);
        });
    }

    banner.upcast()
}

fn refresh_logo_picture(picture: &glib::WeakRef<gtk::Picture>) {
    let Some(picture) = picture.upgrade() else {
        return;
    };
    let Some(path) = branding_logo_path() else {
        return;
    };
    picture.set_filename(Some(path.as_os_str()));
    apply_logo_picture_size(&picture, &path);
    if let Some(parent) = picture.parent() {
        if let Ok(banner) = parent.downcast::<gtk::Box>() {
            let (w, h) = scaled_logo_size(&path);
            banner.set_width_request(w);
            banner.set_height_request(h);
        }
    }
}

fn scaled_logo_size(path: &Path) -> (i32, i32) {
    let path_str = path.to_string_lossy();
    let (nat_w, nat_h) = if let Ok(texture) = gtk::gdk::Texture::from_filename(path_str.as_ref()) {
        (
            texture.width().max(1) as i32,
            texture.height().max(1) as i32,
        )
    } else {
        (1195, 300)
    };

    let width = nat_w.min(LOGO_MAX_WIDTH);
    let height = ((nat_h as f64) * (width as f64 / nat_w as f64)).round() as i32;
    (width, height.max(1))
}

fn apply_logo_picture_size(picture: &gtk::Picture, path: &Path) {
    let (width, height) = scaled_logo_size(path);
    picture.set_size_request(width, height);
}

/// Whether the UI is currently dark (respects Settings → Appearance, not only the OS theme).
pub fn effective_ui_is_dark() -> bool {
    match load_app_settings().appearance.color_scheme {
        ColorScheme::Light => false,
        ColorScheme::Dark => true,
        ColorScheme::System => style_manager_is_dark(),
    }
}

fn style_manager_is_dark() -> bool {
    gtk::gdk::Display::default()
        .map(|display| StyleManager::for_display(&display).is_dark())
        .unwrap_or(false)
}

fn version_subtitle() -> String {
    let app = env!("CARGO_PKG_VERSION");
    match backuppilot_core::pbs_client_version() {
        Some(pbs) => tr_fmt(
            "{app} (proxmox-backup-client {pbs})",
            &[("app", app), ("pbs", &pbs)],
        ),
        None => app.to_string(),
    }
}

fn copyright_line() -> String {
    let year = glib::DateTime::now_local()
        .map(|dt| dt.year())
        .unwrap_or(2026);
    tr_fmt(
        "Copyright © 2018-{year} OneSystems GmbH (Michael Kleger)",
        &[("year", &year.to_string())],
    )
}

fn open_uri(parent: &ApplicationWindow, uri: &str) {
    let launcher = gtk::UriLauncher::new(uri);
    let parent = parent.clone();
    let uri = uri.to_string();
    launcher.launch(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            if let Err(err) = result {
                tracing::warn!(%err, uri = %uri, "failed to open link");
            }
        },
    );
}

/// Horizontal wordmark (`Logo.png` / `Logo-Dark.png`).
pub fn branding_logo_path_for_dark(dark: bool) -> Option<PathBuf> {
    let file = if dark { "logo-dark.png" } else { "logo.png" };
    crate::icons::branding_file_path(file)
}

/// Wordmark matching Settings → Appearance (light → `logo.png`, dark → `logo-dark.png`).
pub fn branding_logo_path() -> Option<PathBuf> {
    branding_logo_path_for_dark(effective_ui_is_dark())
}
