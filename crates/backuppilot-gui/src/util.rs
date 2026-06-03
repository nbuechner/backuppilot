use chrono::{DateTime, Utc};
use gtk::gdk::prelude::*;
use gtk::prelude::*;
use libadwaita::prelude::*;
use libadwaita::Toast;

/// Human-readable duration since `started` (e.g. "5 min", "1 h 12 min").
pub fn format_duration_since(started: DateTime<Utc>) -> String {
    let secs = Utc::now()
        .signed_duration_since(started)
        .num_seconds()
        .max(0);

    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{} min", secs / 60)
    } else {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        if mins == 0 {
            format!("{hours} h")
        } else {
            format!("{hours} h {mins} min")
        }
    }
}

/// Listen-Zeilen-Prefix ohne GTK-Icon-Theme (vermeidet Hänger mit ~/.local/icon-Themes).
pub fn status_prefix_label(semantic_class: &str) -> gtk::Widget {
    let glyph = match semantic_class {
        "success" => "✓",
        "warning" => "!",
        "error" => "✕",
        "accent" => "↻",
        _ => "·",
    };
    gtk::Label::builder()
        .label(glyph)
        .xalign(0.5)
        .width_chars(1)
        .css_classes([semantic_class])
        .margin_end(8)
        .build()
        .upcast()
}

/// Preferences row with a flat suffix button (same pattern as Settings → Reset).
pub fn preferences_action_row_with_button(
    title: &str,
    subtitle: &str,
    button_label: &str,
) -> (libadwaita::ActionRow, gtk::Button) {
    let row = libadwaita::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .activatable(false)
        .build();
    let button = gtk::Button::builder()
        .label(button_label)
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .vexpand(false)
        .hexpand(false)
        .build();
    row.add_suffix(&button);
    (row, button)
}

pub fn clear_list_box(list: &gtk::ListBox) {
    // Kein select_row/unselect_all: bei SelectionMode::None kann das die GTK-Hauptschleife blockieren.
    list.remove_all();
}

pub fn find_child_by_name(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if widget.widget_name().as_str() == name {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(found) = find_child_by_name(&c, name) {
            return Some(found);
        }
        child = c.next_sibling();
    }
    None
}

/// Copy plain text to the primary clipboard (returns false if no display).
pub fn copy_text_to_clipboard(text: &str) -> bool {
    let Some(display) = gtk::gdk::Display::default() else {
        return false;
    };
    let clipboard = display.clipboard();
    clipboard.set_text(text);
    // Fallback for some Wayland compositors where set_text alone is unreliable.
    let bytes = gtk::glib::Bytes::from(text.as_bytes());
    let provider = gtk::gdk::ContentProvider::for_bytes("text/plain;charset=utf-8", &bytes);
    let _ = clipboard.set_content(Some(&provider));
    true
}

/// Escape text shown in libadwaita toasts (Pango markup).
pub fn escape_pango_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Brief feedback at the bottom of the main window, if it is open.
pub fn show_toast(message: &str) {
    if let Some(overlay) = crate::window::toast_overlay() {
        overlay.add_toast(Toast::new(&escape_pango_markup(message)));
    }
}
