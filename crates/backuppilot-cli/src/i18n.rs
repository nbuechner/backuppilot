//! CLI gettext — uses the same `app-settings.json` language as the GUI.

use backuppilot_core::{load_app_settings, UiLanguage};
use backuppilot_i18n::{self, UiLanguage as I18nLanguage};

pub use backuppilot_i18n::{tr, tr_fmt};

/// Bind locale and apply language from `~/.config/backuppilot/app-settings.json`.
pub fn init_from_app_settings() {
    backuppilot_i18n::init();
    apply_language(&load_app_settings());
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
