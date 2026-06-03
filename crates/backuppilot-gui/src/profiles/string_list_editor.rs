//! Editable path/pattern lists in the profile editor (Libadwaita rows instead of text areas).

use std::cell::RefCell;
use std::rc::Rc;

use backuppilot_i18n::tr;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::util::{clear_list_box, preferences_action_row_with_button};

pub struct StringListEditor {
    items: Rc<RefCell<Vec<String>>>,
    list: gtk::ListBox,
    empty_label: String,
    icon_name: &'static str,
}

impl StringListEditor {
    fn rebuild(&self) {
        clear_list_box(&self.list);
        let items = self.items.borrow();
        if items.is_empty() {
            let row = gtk::ListBoxRow::new();
            let label = gtk::Label::builder()
                .label(&self.empty_label)
                .xalign(0.0)
                .margin_start(12)
                .margin_end(12)
                .margin_top(10)
                .margin_bottom(10)
                .wrap(true)
                .build();
            label.add_css_class("dim-label");
            row.set_child(Some(&label));
            row.set_activatable(false);
            row.set_selectable(false);
            self.list.append(&row);
            return;
        }

        for (index, value) in items.iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            row.set_activatable(false);
            row.set_selectable(false);

            let action = libadwaita::ActionRow::builder()
                .title(&ellipsize_middle(value, 80))
                .activatable(false)
                .tooltip_text(value)
                .build();
            action.add_prefix(
                &gtk::Image::builder()
                    .icon_name(self.icon_name)
                    .pixel_size(18)
                    .build(),
            );

            let remove_btn = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text(&tr("Remove"))
                .css_classes(["flat"])
                .valign(gtk::Align::Center)
                .build();
            let items_rc = self.items.clone();
            let list = self.list.clone();
            let empty_label = self.empty_label.clone();
            let icon_name = self.icon_name;
            remove_btn.connect_clicked(move |_| {
                let mut items = items_rc.borrow_mut();
                if index < items.len() {
                    items.remove(index);
                }
                drop(items);
                let editor = StringListEditor {
                    items: items_rc.clone(),
                    list: list.clone(),
                    empty_label: empty_label.clone(),
                    icon_name,
                };
                editor.rebuild();
            });
            action.add_suffix(&remove_btn);
            row.set_child(Some(&action));
            self.list.append(&row);
        }
    }

    fn add_unique(&self, value: String) -> bool {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return false;
        }
        let value = trimmed.to_string();
        let mut items = self.items.borrow_mut();
        if items.iter().any(|existing| existing == &value) {
            return false;
        }
        items.push(value);
        true
    }
}

fn ellipsize_middle(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1) / 2;
    let start: String = text.chars().take(keep).collect();
    let end: String = text
        .chars()
        .rev()
        .take(keep)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{start}…{end}")
}

pub fn build_backup_paths_block(
    parent: &gtk::Window,
    items: Rc<RefCell<Vec<String>>>,
) -> gtk::Widget {
    let editor = Rc::new(StringListEditor {
        items,
        list: gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .margin_start(12)
            .margin_end(12)
            .build(),
        empty_label: tr("No folders selected yet."),
        icon_name: "folder-symbolic",
    });
    editor.rebuild();

    let (add_row, add_btn) = preferences_action_row_with_button(
        &tr("Add folder"),
        &tr("Choose a directory on this computer."),
        &tr("Browse…"),
    );

    let parent = parent.clone();
    let editor_add = editor.clone();
    add_btn.connect_clicked(move |_| {
        pick_folder(&parent, editor_add.clone());
    });

    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    box_.append(&editor.list);
    box_.append(&add_row);

    box_.upcast()
}

pub fn build_excludes_block(
    parent: &gtk::Window,
    items: Rc<RefCell<Vec<String>>>,
) -> gtk::Widget {
    let editor = Rc::new(StringListEditor {
        items,
        list: gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .margin_start(12)
            .margin_end(12)
            .build(),
        empty_label: tr("No exclude patterns yet , backups include all files under the selected folders."),
        icon_name: "text-x-generic-symbolic",
    });
    editor.rebuild();

    let (add_row, add_btn) = preferences_action_row_with_button(
        &tr("Add exclude pattern"),
        &tr("For example *.tmp or node_modules"),
        &tr("Add…"),
    );

    let parent = parent.clone();
    let editor_add = editor.clone();
    add_btn.connect_clicked(move |_| {
        prompt_exclude_pattern(&parent, editor_add.clone());
    });

    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    box_.append(&editor.list);
    box_.append(&add_row);

    box_.upcast()
}

fn pick_folder(parent: &gtk::Window, editor: Rc<StringListEditor>) {
    let dialog = gtk::FileDialog::builder()
        .title(&tr("Select folder to back up"))
        .modal(true)
        .build();

    let parent = parent.clone();
    dialog.select_folder(
        Some(&parent),
        None::<&gio::Cancellable>,
        glib::clone!(
            #[strong]
            editor,
            move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                let path = path.to_string_lossy().into_owned();
                if editor.add_unique(path) {
                    editor.rebuild();
                }
            }
        ),
    );
}

fn prompt_exclude_pattern(parent: &gtk::Window, editor: Rc<StringListEditor>) {
    let dialog = libadwaita::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title(&tr("Exclude pattern"))
        .default_width(440)
        .default_height(200)
        .build();

    let entry = libadwaita::EntryRow::builder()
        .title(&tr("Pattern"))
        .text("")
        .show_apply_button(false)
        .build();

    let hint = gtk::Label::builder()
        .label(&tr(
            "One pattern per entry. * matches part of a name (e.g. *.tmp). \
             Use **/ only if a folder should be skipped in all subfolders , e.g. **/node_modules.",
        ))
        .xalign(0.0)
        .wrap(true)
        .margin_start(12)
        .margin_end(12)
        .build();
    hint.add_css_class("dim-label");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&hint);
    content.append(&entry);

    let header = libadwaita::HeaderBar::new();
    let cancel = gtk::Button::with_label(&tr("Cancel"));
    let add = gtk::Button::with_label(&tr("Add"));
    add.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&add);

    let toolbar = libadwaita::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    dialog.set_content(Some(&toolbar));

    let dialog_cancel = dialog.clone();
    cancel.connect_clicked(move |_| {
        dialog_cancel.destroy();
    });

    let editor_add = editor.clone();
    let dialog_add = dialog.clone();
    add.connect_clicked(move |_| {
        let pattern = entry.text().to_string();
        if editor_add.add_unique(pattern) {
            editor_add.rebuild();
        }
        dialog_add.destroy();
    });

    dialog.present();
}
