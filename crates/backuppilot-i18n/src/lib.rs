//! GNU gettext bindings shared by GUI, daemon and notifications.
//!
//! User-visible strings in source code are **English** (msgid). Translations live in
//! `App/po/<lang>.po` and compile to `App/locale/<lang>/LC_MESSAGES/backuppilot.mo`.

use std::path::{Path, PathBuf};

use gettext_rs::{bindtextdomain, setlocale, textdomain, LocaleCategory};

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

/// Apply a user-selected UI language (`System` = follow the OS locale).
pub fn set_language(language: UiLanguage) {
    match language {
        UiLanguage::System => {
            // SAFETY: single-threaded GUI / daemon startup before other threads use env.
            unsafe { std::env::remove_var("LANGUAGE") };
            let _ = setlocale(LocaleCategory::LcAll, "");
        }
        UiLanguage::German => {
            unsafe { std::env::set_var("LANGUAGE", "de") };
            try_set_locale_messages(&[
                "de_DE.UTF-8",
                "de_DE.utf8",
                "de_DE",
                "de.UTF-8",
                "de.utf8",
                "de",
            ]);
        }
        UiLanguage::English => {
            unsafe { std::env::set_var("LANGUAGE", "en") };
            try_set_locale_messages(&[
                "en_US.UTF-8",
                "en_US.utf8",
                "en_US",
                "en.UTF-8",
                "en.utf8",
                "en",
                "C",
            ]);
        }
        UiLanguage::French => {
            unsafe { std::env::set_var("LANGUAGE", "fr") };
            try_set_locale_messages(&[
                "fr_FR.UTF-8",
                "fr_FR.utf8",
                "fr_FR",
                "fr.UTF-8",
                "fr.utf8",
                "fr",
            ]);
        }
        UiLanguage::Italian => {
            unsafe { std::env::set_var("LANGUAGE", "it") };
            try_set_locale_messages(&[
                "it_IT.UTF-8",
                "it_IT.utf8",
                "it_IT",
                "it.UTF-8",
                "it.utf8",
                "it",
            ]);
        }
    }
    rebind_textdomain();
}

fn rebind_textdomain() {
    if let Some(dir) = locale_search_dir() {
        let _ = bindtextdomain(DOMAIN, dir.to_str().unwrap_or("/usr/share/locale"));
    } else {
        let _ = bindtextdomain(DOMAIN, "/usr/share/locale");
    }
    let _ = textdomain(DOMAIN);
}

fn try_set_locale_messages(candidates: &[&str]) {
    for locale in candidates {
        if setlocale(LocaleCategory::LcMessages, *locale).is_some() {
            return;
        }
    }
}

/// Initialize libc locale and bind the `backuppilot` text domain.
///
/// Search order for `.mo` files:
/// 1. `$BACKUPPILOT_LOCALE_DIR`
/// 2. `../locale` next to the running binary (local install)
/// 3. `App/locale` when running via `cargo run` from the workspace
/// 4. `/usr/share/locale` (distribution package)
pub fn init() {
    setlocale(LocaleCategory::LcAll, "");
    rebind_textdomain();
}

fn locale_search_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("BACKUPPILOT_LOCALE_DIR") {
        let path = PathBuf::from(dir);
        if locale_dir_usable(&path) {
            return Some(path);
        }
    }

    // Prefer user-installed translations from `build.sh local --install`.
    if let Ok(home) = std::env::var("HOME") {
        let local = PathBuf::from(home).join(".local/share/locale");
        if locale_dir_usable(&local) {
            return local.canonicalize().ok();
        }
    }

    // `cargo run -p backuppilot-gui` → manifest dir is `crates/backuppilot-gui`
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

fn locale_dir_usable(base: &Path) -> bool {
    ["de", "en", "fr", "it"].iter().any(|lang| {
        base.join(lang)
            .join("LC_MESSAGES")
            .join(format!("{DOMAIN}.mo"))
            .is_file()
    })
}

/// Translate a static English msgid at runtime.
#[inline]
pub fn tr(msgid: &str) -> String {
    gettext_rs::gettext(msgid)
}

/// Translate and substitute `{name}`-style placeholders in the translated string.
///
/// Example: `tr_fmt("Hello, {name}!", &[("name", "World")])`
pub fn tr_fmt(msgid: &str, replacements: &[(&str, &str)]) -> String {
    let mut s = tr(msgid);
    for (key, value) in replacements {
        s = s.replace(&format!("{{{key}}}"), value);
    }
    s
}

/// Plural form (English msgid / msgid_plural).
#[inline]
pub fn tr_n(msgid: &str, msgid_plural: &str, n: u32) -> String {
    gettext_rs::ngettext(msgid, msgid_plural, n)
}

/// Re-export for advanced use (context, comments in `.po`).
pub use gettext_rs::{gettext, ngettext, pgettext};

#[macro_export]
macro_rules! tr {
    ($msgid:literal) => {
        $crate::tr($msgid)
    };
}
