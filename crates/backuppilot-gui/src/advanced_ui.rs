//! Global “advanced mode” — hides expert options in settings and the profile editor.

use backuppilot_core::{load_app_settings, save_app_settings};
use backuppilot_i18n::tr;
use gtk::prelude::*;

pub fn advanced_mode_enabled() -> bool {
    load_app_settings().appearance.advanced_mode
}

pub fn persist_advanced_mode(enabled: bool) {
    let mut settings = load_app_settings();
    if settings.appearance.advanced_mode == enabled {
        return;
    }
    settings.appearance.advanced_mode = enabled;
    let _ = save_app_settings(&settings);
}

pub fn set_sections_visible(sections: &[gtk::Widget], visible: bool) {
    for section in sections {
        section.set_visible(visible);
    }
}

/// Compact label + switch for header bars (not a full preferences row).
pub fn build_compact_advanced_toggle(switch: &gtk::Switch) -> gtk::Box {
    let label = gtk::Label::builder()
        .label(&tr("Advanced"))
        .css_classes(["caption", "dim-label"])
        .valign(gtk::Align::Center)
        .build();

    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    box_.append(&label);
    box_.append(switch);
    box_
}

/// Right-aligned strip for the top of the settings page.
pub fn build_settings_advanced_strip(switch: &gtk::Switch) -> gtk::Widget {
    let strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::End)
        .margin_bottom(8)
        .build();
    strip.append(&build_compact_advanced_toggle(switch));
    strip.upcast()
}

/// Toggles visibility of advanced sections and persists the setting.
pub fn bind_advanced_mode_switch(switch: &gtk::Switch, advanced_sections: &[gtk::Widget]) {
    let enabled = advanced_mode_enabled();
    switch.set_active(enabled);
    set_sections_visible(advanced_sections, enabled);

    let sections: Vec<gtk::Widget> = advanced_sections.to_vec();
    switch.connect_active_notify(move |sw| {
        let on = sw.is_active();
        persist_advanced_mode(on);
        set_sections_visible(&sections, on);
    });
}
