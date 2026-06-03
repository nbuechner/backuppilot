use backuppilot_core::app_settings::{AppearanceSettings, AppSettings, ColorScheme, UiLanguage};
use backuppilot_core::{load_app_settings, save_app_settings};
use backuppilot_i18n::{self, UiLanguage as I18nLanguage};
use libadwaita::StyleManager;

use crate::about;

pub fn apply(settings: &AppSettings) {
    apply_appearance(&settings.appearance);
}

pub fn apply_loaded() {
    apply(&load_app_settings());
}

pub fn apply_appearance(appearance: &AppearanceSettings) {
    backuppilot_i18n::set_language(to_i18n_language(appearance.language));

    if let Some(display) = gtk::gdk::Display::default() {
        let style = StyleManager::for_display(&display);
        style.set_color_scheme(to_libadwaita_scheme(appearance.color_scheme));
    }

    about::refresh_about_logo_if_visible();
}

pub fn close_to_tray_enabled() -> bool {
    load_app_settings().tray.close_to_tray
}

pub fn tray_icon_enabled() -> bool {
    load_app_settings().tray.show_tray_icon
}

pub fn save(settings: &AppSettings) -> Result<(), String> {
    save_app_settings(settings).map_err(|e| e.to_string())
}

fn to_i18n_language(lang: UiLanguage) -> I18nLanguage {
    match lang {
        UiLanguage::System => I18nLanguage::System,
        UiLanguage::German => I18nLanguage::German,
        UiLanguage::English => I18nLanguage::English,
        UiLanguage::French => I18nLanguage::French,
        UiLanguage::Italian => I18nLanguage::Italian,
    }
}

fn to_libadwaita_scheme(scheme: ColorScheme) -> libadwaita::ColorScheme {
    match scheme {
        ColorScheme::System => libadwaita::ColorScheme::Default,
        ColorScheme::Light => libadwaita::ColorScheme::ForceLight,
        ColorScheme::Dark => libadwaita::ColorScheme::ForceDark,
    }
}

pub fn language_from_dropdown(selected: u32) -> UiLanguage {
    match selected {
        1 => UiLanguage::German,
        2 => UiLanguage::English,
        3 => UiLanguage::French,
        4 => UiLanguage::Italian,
        _ => UiLanguage::System,
    }
}

pub fn language_dropdown_index(lang: UiLanguage) -> u32 {
    match lang {
        UiLanguage::System => 0,
        UiLanguage::German => 1,
        UiLanguage::English => 2,
        UiLanguage::French => 3,
        UiLanguage::Italian => 4,
    }
}

pub fn color_scheme_from_dropdown(selected: u32) -> ColorScheme {
    match selected {
        1 => ColorScheme::Light,
        2 => ColorScheme::Dark,
        _ => ColorScheme::System,
    }
}

pub fn color_scheme_dropdown_index(scheme: ColorScheme) -> u32 {
    match scheme {
        ColorScheme::System => 0,
        ColorScheme::Light => 1,
        ColorScheme::Dark => 2,
    }
}

pub fn update_channel_from_dropdown(selected: u32) -> backuppilot_core::UpdateChannel {
    match selected {
        1 => backuppilot_core::UpdateChannel::Beta,
        _ => backuppilot_core::UpdateChannel::Stable,
    }
}

pub fn update_channel_dropdown_index(channel: backuppilot_core::UpdateChannel) -> u32 {
    match channel {
        backuppilot_core::UpdateChannel::Stable => 0,
        backuppilot_core::UpdateChannel::Beta => 1,
    }
}
