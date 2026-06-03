//! Main window chrome (Evince-style full-height sidebar + content header).

use gtk::prelude::*;
use libadwaita::{OverlaySplitView, WindowTitle};

use backuppilot_i18n::tr;

const SHELL_CSS: &str = r#"
navigation-sidebar row {
  padding: 6px 8px;
}
"#;

pub fn load_styles() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(SHELL_CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Full-height sidebar + content pane with its own header.
pub fn build_main_layout(
    stack: &gtk::Stack,
    main_body: &impl IsA<gtk::Widget>,
) -> (OverlaySplitView, WindowTitle) {
    let split = OverlaySplitView::builder()
        .min_sidebar_width(56.0)
        .max_sidebar_width(260.0)
        .sidebar_width_fraction(0.18)
        .show_sidebar(true)
        .build();

    let sidebar = build_sidebar_column(stack);
    split.set_sidebar(Some(&sidebar));

    let (header, window_title) = build_content_header_bar(&split);
    let content_pane = build_content_pane(&header, main_body);
    split.set_content(Some(&content_pane));

    (split, window_title)
}

fn build_sidebar_column(stack: &gtk::Stack) -> gtk::StackSidebar {
    let stack_sidebar = gtk::StackSidebar::new();
    stack_sidebar.set_stack(stack);
    stack_sidebar.set_halign(gtk::Align::Fill);
    stack_sidebar.set_vexpand(true);
    stack_sidebar.add_css_class("navigation-sidebar");
    stack_sidebar.add_css_class("sidebar");
    stack_sidebar
}

fn build_content_pane(header: &libadwaita::HeaderBar, body: &impl IsA<gtk::Widget>) -> gtk::Box {
    let pane = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    pane.append(header);
    body.set_vexpand(true);
    pane.append(body);
    pane
}

fn build_content_header_bar(split: &OverlaySplitView) -> (libadwaita::HeaderBar, WindowTitle) {
    let header = libadwaita::HeaderBar::new();
    header.add_css_class("flat");

    let window_title = WindowTitle::new(&tr("Overview"), &tr("BackupPilot"));

    let sidebar_toggle = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-symbolic")
        .tooltip_text(&tr("Show sidebar"))
        .active(true)
        .css_classes(["flat"])
        .build();
    sidebar_toggle
        .bind_property("active", split, "show-sidebar")
        .bidirectional()
        .sync_create()
        .build();

    header.set_title_widget(Some(&window_title));
    header.pack_end(&sidebar_toggle);

    (header, window_title)
}

pub fn set_stack_page_icon(stack: &gtk::Stack, name: &str, icon: &str) {
    if let Some(child) = stack.child_by_name(name) {
        stack.page(&child).set_icon_name(icon);
    }
}

pub fn page_title(page_name: &str) -> String {
    match page_name {
        "dashboard" => tr("Overview"),
        "profiles" => tr("Profiles"),
        "restore" => tr("Restore"),
        "encryption" => tr("Encryption"),
        "settings" => tr("Settings"),
        "about" => tr("About"),
        _ => tr("BackupPilot"),
    }
}

pub fn page_subtitle(page_name: &str) -> String {
    match page_name {
        "dashboard" => tr("Status and recent backup activity"),
        "profiles" => tr("Configure backup profiles and schedules"),
        "restore" => tr("Browse snapshots and restore files"),
        "encryption" => tr("Create and manage backup encryption keys"),
        "settings" => tr("Application preferences"),
        "about" => tr("Version, updates and support"),
        _ => tr("BackupPilot"),
    }
}

pub fn bind_page_titles(stack: &gtk::Stack, window_title: &WindowTitle) {
    let window_title = window_title.clone();
    let stack = stack.clone();
    update_page_titles(&stack, &window_title);
    stack.connect_visible_child_name_notify(move |stack| {
        update_page_titles(stack, &window_title);
    });
}

fn update_page_titles(stack: &gtk::Stack, window_title: &WindowTitle) {
    let name = stack
        .visible_child_name()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "dashboard".to_string());
    window_title.set_title(&page_title(&name));
    window_title.set_subtitle(&page_subtitle(&name));
}
