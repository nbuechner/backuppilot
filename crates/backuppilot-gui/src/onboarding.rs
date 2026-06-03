//! First-run setup assistant on the dashboard.

use backuppilot_i18n::tr;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::dbus_client::{self, connect};
use crate::dbus_runtime;
use crate::profiles::editor;
use crate::window;

const ONBOARDING_NAME: &str = "dashboard-onboarding";

pub fn build_placeholder() -> gtk::Widget {
    let block = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .visible(false)
        .build();
    block.set_widget_name(ONBOARDING_NAME);
    block.upcast()
}

pub fn update_dashboard(
    dashboard: &gtk::Widget,
    parent: &libadwaita::ApplicationWindow,
    profile_count: usize,
    completed: bool,
) {
    let Some(container) = crate::util::find_child_by_name(dashboard, ONBOARDING_NAME) else {
        return;
    };
    let Some(container) = container.downcast_ref::<gtk::Box>() else {
        return;
    };

    if completed || profile_count > 0 {
        container.set_visible(false);
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        return;
    }

    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    container.append(&build_setup_block(parent));
    container.set_visible(true);
}

fn build_setup_block(parent: &libadwaita::ApplicationWindow) -> gtk::Widget {
    let card = libadwaita::PreferencesGroup::builder()
        .title(&tr("Set up your first backup"))
        .description(&tr("No backup destination configured yet. Connect your own Proxmox Backup Server or add a profile manually."))
        .build();

    let own_pbs = libadwaita::ActionRow::builder()
        .title(&tr("Connect your own PBS"))
        .subtitle(&tr("Repository, token, and backup paths"))
        .activatable(true)
        .build();
    own_pbs.connect_activated({
        let parent = parent.clone();
        move |_row| open_new_profile(&parent)
    });

    let cloud = libadwaita::ActionRow::builder()
        .title(&tr("Discover PBS Cloud"))
        .subtitle(&tr("Compatible hosted Proxmox Backup Server offerings"))
        .activatable(true)
        .build();
    cloud.connect_activated(|_| {
        if gtk::gdk::Display::default().is_some() {
            let launcher = gtk::UriLauncher::new("https://www.backup-as-a-service.cloud/");
            launcher.launch(None::<&gtk::Window>, None::<&gtk::gio::Cancellable>, |_| {});
        }
    });

    let later = libadwaita::ActionRow::builder()
        .title(&tr("Configure later"))
        .subtitle(&tr("Hide this assistant until you add a profile"))
        .activatable(true)
        .build();
    later.connect_activated(|_row| mark_completed());

    card.add(&own_pbs);
    card.add(&cloud);
    card.add(&later);
    card.upcast()
}

fn open_new_profile(parent: &libadwaita::ApplicationWindow) {
    mark_completed();
    window::switch_to_profiles();
    let refresh = || window::refresh_profiles_public();
    editor::open(parent, None, refresh);
}

fn mark_completed() {
    dbus_runtime::spawn(
        async move {
            let proxy = connect().await?;
            dbus_client::complete_onboarding(&proxy).await
        },
        |_| {
            window::refresh_dashboard_public();
        },
    );
}
