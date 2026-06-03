//! Standard Libadwaita form rows (full-width entry under the label).

use gtk::prelude::*;

/// Single-line text field (`AdwEntryRow`, no apply/clear icons).
pub struct TextEntryRow {
    pub row: libadwaita::EntryRow,
}

impl TextEntryRow {
    pub fn new(title: &str, text: &str) -> Self {
        let row = libadwaita::EntryRow::builder()
            .title(title)
            .text(text)
            .show_apply_button(false)
            .build();
        Self { row }
    }

    pub fn text(&self) -> String {
        self.row.text().to_string()
    }

}

/// Password field (`AdwPasswordEntryRow`, no apply button).
pub struct PasswordEntryField {
    pub row: libadwaita::PasswordEntryRow,
}

impl PasswordEntryField {
    pub fn new(title: &str, text: &str) -> Self {
        let row = libadwaita::PasswordEntryRow::builder()
            .title(title)
            .text(text)
            .show_apply_button(false)
            .build();
        Self { row }
    }

    pub fn text(&self) -> String {
        self.row.text().to_string()
    }

}
