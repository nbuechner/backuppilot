//! GNU gettext bindings shared by GUI, daemon and notifications.
//!
//! User-visible strings in source code are **English** (msgid). Translations live in
//! `App/po/<lang>.po` and compile to `App/locale/<lang>/LC_MESSAGES/backuppilot.mo`.
//!
//! On Windows gettext is not available; all functions return the English msgid unchanged.

pub const DOMAIN: &str = "backuppilot";

/// User-facing language selection (matches `AppSettings::appearance::language`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiLanguage {
    #[default]
    System,
    German,
    English,
    French,
    Italian,
}

// ── Non-Windows: full GNU gettext implementation ──────────────────────────────

#[cfg(not(windows))]
mod imp {
    use std::path::{Path, PathBuf};
    use gettext_rs::{bindtextdomain, setlocale, textdomain, LocaleCategory};
    use super::{DOMAIN, UiLanguage};

    pub fn set_language(language: UiLanguage) {
        match language {
            UiLanguage::System => {
                unsafe { std::env::remove_var("LANGUAGE") };
                let _ = setlocale(LocaleCategory::LcAll, "");
            }
            UiLanguage::German => {
                unsafe { std::env::set_var("LANGUAGE", "de") };
                try_set_locale_messages(&[
                    "de_DE.UTF-8", "de_DE.utf8", "de_DE", "de.UTF-8", "de.utf8", "de",
                ]);
            }
            UiLanguage::English => {
                unsafe { std::env::set_var("LANGUAGE", "en") };
                try_set_locale_messages(&[
                    "en_US.UTF-8", "en_US.utf8", "en_US", "en.UTF-8", "en.utf8", "en", "C",
                ]);
            }
            UiLanguage::French => {
                unsafe { std::env::set_var("LANGUAGE", "fr") };
                try_set_locale_messages(&[
                    "fr_FR.UTF-8", "fr_FR.utf8", "fr_FR", "fr.UTF-8", "fr.utf8", "fr",
                ]);
            }
            UiLanguage::Italian => {
                unsafe { std::env::set_var("LANGUAGE", "it") };
                try_set_locale_messages(&[
                    "it_IT.UTF-8", "it_IT.utf8", "it_IT", "it.UTF-8", "it.utf8", "it",
                ]);
            }
        }
        rebind_textdomain();
    }

    pub fn init() {
        setlocale(LocaleCategory::LcAll, "");
        rebind_textdomain();
    }

    pub fn tr(msgid: &str) -> String {
        gettext_rs::gettext(msgid)
    }

    pub fn tr_n(msgid: &str, msgid_plural: &str, n: u32) -> String {
        gettext_rs::ngettext(msgid, msgid_plural, n)
    }

    pub fn rebind_textdomain() {
        let dir = locale_search_dir().unwrap_or_else(|| PathBuf::from("/usr/share/locale"));
        let _ = bindtextdomain(DOMAIN, dir.to_str().unwrap_or("/usr/share/locale"));
        let _ = textdomain(DOMAIN);
    }

    fn try_set_locale_messages(candidates: &[&str]) {
        for locale in candidates {
            if setlocale(LocaleCategory::LcMessages, *locale).is_some() {
                return;
            }
        }
    }

    pub fn locale_search_dir() -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("BACKUPPILOT_LOCALE_DIR") {
            let path = PathBuf::from(dir);
            if locale_dir_usable(&path) {
                return Some(path);
            }
        }

        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"));
        if let Ok(home) = home {
            let local = PathBuf::from(home).join(".local/share/locale");
            if locale_dir_usable(&local) {
                return local.canonicalize().ok();
            }
        }

        if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
            let path = Path::new(&manifest).join("../../locale");
            if locale_dir_usable(&path) {
                return path.canonicalize().ok();
            }
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                for relative in ["../locale", "../../locale"] {
                    let local = parent.join(relative);
                    if locale_dir_usable(&local) {
                        return local.canonicalize().ok();
                    }
                }
            }
        }

        if locale_dir_usable(Path::new("/app/share/locale")) {
            return Some(PathBuf::from("/app/share/locale"));
        }

        if locale_dir_usable(Path::new("/usr/share/locale")) {
            return Some(PathBuf::from("/usr/share/locale"));
        }

        None
    }

    pub fn locale_dir_usable(base: &Path) -> bool {
        ["de", "en", "fr", "it"].iter().any(|lang| {
            base.join(lang)
                .join("LC_MESSAGES")
                .join(format!("{DOMAIN}.mo"))
                .is_file()
        })
    }
}

// ── Windows: no-op stub (msgids are English, returned unchanged) ──────────────

#[cfg(windows)]
mod imp {
    use super::UiLanguage;

    pub fn set_language(_language: UiLanguage) {}
    pub fn init() {}
    pub fn tr(msgid: &str) -> String { msgid.to_string() }
    pub fn tr_n(msgid: &str, msgid_plural: &str, n: u32) -> String {
        if n == 1 { msgid.to_string() } else { msgid_plural.to_string() }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn set_language(language: UiLanguage) {
    imp::set_language(language);
}

pub fn init() {
    imp::init();
}

#[inline]
pub fn tr(msgid: &str) -> String {
    imp::tr(msgid)
}

pub fn tr_fmt(msgid: &str, replacements: &[(&str, &str)]) -> String {
    let mut s = tr(msgid);
    for (key, value) in replacements {
        s = s.replace(&format!("{{{key}}}"), value);
    }
    s
}

#[inline]
pub fn tr_n(msgid: &str, msgid_plural: &str, n: u32) -> String {
    imp::tr_n(msgid, msgid_plural, n)
}

// Re-exports for callers that use gettext_rs directly (non-Windows only).
#[cfg(not(windows))]
pub use gettext_rs::{gettext, ngettext, pgettext};

#[macro_export]
macro_rules! tr {
    ($msgid:literal) => {
        $crate::tr($msgid)
    };
}
