use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use gdk_pixbuf::Pixbuf;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;

use backuppilot_core::is_flatpak_runtime;
use crate::tray_state::TrayIndicator;

pub use backuppilot_core::ICON_NAME;

const TRAY_SYMBOLIC_FALLBACK: &str = "folder-save-symbolic";
/// Minimal outer margin after trimming transparent borders (tray panels use ~16–22 px).
const TRAY_ICON_PADDING_FRAC: f32 = 0.0;
/// Zoom into the glyph — Icon-Dark.png leaves much empty space inside the shield shape.
const TRAY_ICON_ZOOM: f32 = 1.28;
const TRAY_PIXMAP_SIZES: [i32; 5] = [16, 22, 24, 32, 48];

#[cfg(has_embedded_tray_icon)]
const EMBEDDED_TRAY_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded-tray-icon.png"));

static TRAY_PIXMAPS: OnceLock<Vec<ksni::Icon>> = OnceLock::new();
static TRAY_THEME_ICON_NAME: OnceLock<String> = OnceLock::new();

/// Resolved tray assets for StatusNotifierItem.
#[derive(Debug, Clone)]
pub struct TrayIconAssets {
    /// Empty when [`Self::pixmaps`] are set — panels prefer `IconName` and would ignore pixmaps.
    pub icon_name: String,
    pub icon_theme_path: String,
    pub pixmaps: Vec<ksni::Icon>,
    pub overlay_icon_name: String,
}

/// Preload tray pixmaps on the GTK main thread (call before [`crate::tray::spawn`]).
pub fn init_tray_pixmaps() -> bool {
    let pixmaps = TRAY_PIXMAPS.get_or_init(load_tray_ksni_icons);
    let theme_name = if icon_theme_has_name(ICON_NAME) {
        ICON_NAME.to_string()
    } else {
        TRAY_SYMBOLIC_FALLBACK.to_string()
    };
    let _ = TRAY_THEME_ICON_NAME.set(theme_name);
    if pixmaps.is_empty() {
        tracing::warn!(
            "Tray icon: no pixmap source (run App/scripts/generate-icons.sh or rebuild with branding/icon.png)"
        );
        false
    } else {
        tracing::debug!(count = pixmaps.len(), "tray pixmaps cached");
        true
    }
}

/// Tray-Status ohne GTK (für Hintergrund-Thread / ksni).
pub fn tray_assets_for_indicator(indicator: TrayIndicator, spin_frame: u8) -> TrayIconAssets {
    let pixmaps = tray_icon_pixmaps_cached();
    let icon_theme_path = icon_theme_path_for_app();
    let overlay_icon_name = overlay_icon_for_indicator(indicator, spin_frame);
    let icon_name = if pixmaps.is_empty() {
        TRAY_THEME_ICON_NAME
            .get()
            .cloned()
            .unwrap_or_else(|| TRAY_SYMBOLIC_FALLBACK.to_string())
    } else {
        String::new()
    };
    TrayIconAssets {
        icon_name,
        icon_theme_path,
        pixmaps,
        overlay_icon_name,
    }
}

fn tray_icon_pixmaps_cached() -> Vec<ksni::Icon> {
    TRAY_PIXMAPS
        .get()
        .cloned()
        .unwrap_or_else(|| load_tray_ksni_icons())
}

pub fn overlay_icon_for_indicator(indicator: TrayIndicator, spin_frame: u8) -> String {
    match indicator {
        TrayIndicator::Ok | TrayIndicator::Unknown => String::new(),
        TrayIndicator::Warning => "dialog-warning-symbolic".into(),
        TrayIndicator::Critical => "dialog-error-symbolic".into(),
        TrayIndicator::Running if spin_frame.is_multiple_of(2) => "view-refresh-symbolic".into(),
        TrayIndicator::Running => String::new(),
    }
}

/// Path to a file under the app branding directory (`icon.png`, `logo.png`, …).
pub fn branding_file_path(filename: &str) -> Option<PathBuf> {
    branding_dir_candidates()
        .into_iter()
        .map(|dir| dir.join(filename))
        .find(|path| path.is_file())
}

/// Path to the bundled app icon file (`Icon-Dark.png` copy).
pub fn branding_app_icon_path() -> Option<PathBuf> {
    branding_file_path("icon.png")
}

fn branding_dir_candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(dir) = std::env::var("BACKUPPILOT_BRANDING_DIR") {
        dirs.push(PathBuf::from(dir));
    }

    dirs.push(PathBuf::from("/app/share/backuppilot/branding"));
    dirs.push(PathBuf::from("/usr/share/backuppilot/branding"));

    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/backuppilot/branding"));
    }

    dirs.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/branding"),
    );

    dirs
}

fn tray_icon_path() -> Option<PathBuf> {
    branding_file_path("tray-icon.png")
}

/// Icon theme search paths safe for GTK (no host exports / cyclic index.theme chains).
fn icon_search_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = icon_root_candidates()
        .into_iter()
        .filter(|p| p.join("hicolor").is_dir())
        .collect();

    if is_flatpak_runtime() {
        // --filesystem=home exposes ~/.local/share/icons; has_icon() can recurse forever there.
        if let Ok(home) = std::env::var("HOME") {
            let home = PathBuf::from(home);
            paths.retain(|p| !p.starts_with(&home));
        }
        paths.retain(|p| p.starts_with("/app") || p.starts_with("/usr"));
    }

    paths
}

/// Register icon theme paths (hicolor from build) and window icon.
pub fn init() {
    if let Some(display) = gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        let paths = icon_search_paths();
        let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
        if !refs.is_empty() {
            // Ersetzt die Standard-Suchpfade (inkl. Flatpak-Exports), die lokale GUI-Starts hängen lassen.
            theme.set_search_path(&refs);
        }
    }

    // Kein gtk-update-icon-cache beim App-Start (nur bei ./build.sh --install).
    // Läuft synchron und kann die GTK-Hauptschleife blockieren (Symptom: „antwortet nicht“).

    if !icon_theme_has_name(ICON_NAME) {
        tracing::warn!(
            icon = ICON_NAME,
            "App icon not in GTK icon theme — run ./build.sh … --install"
        );
    }

    apply_default_window_icon();
}

pub fn apply_to_application(_app: &libadwaita::Application) {
    apply_default_window_icon();
}

pub fn apply_window_icon(window: &impl IsA<gtk::Window>) {
    let name = if icon_theme_has_name(ICON_NAME) {
        ICON_NAME
    } else {
        TRAY_SYMBOLIC_FALLBACK
    };
    window.set_icon_name(Some(name));
}

fn apply_default_window_icon() {
    let name = if icon_theme_has_name(ICON_NAME) {
        ICON_NAME
    } else {
        TRAY_SYMBOLIC_FALLBACK
    };
    gtk::Window::set_default_icon_name(name);
}

fn icon_theme_has_name(name: &str) -> bool {
    if name != ICON_NAME {
        return false;
    }
    // Nur gebündelte PNGs prüfen — IconTheme::has_icon() rekursiert in Host-/Export-Themes endlos.
    icon_search_paths()
        .into_iter()
        .any(|root| hicolor_app_icon_path_in_root(&root, 32).is_some())
}

fn icon_theme_path_for_app() -> String {
    for root in icon_root_candidates() {
        if hicolor_app_icon_path_in_root(&root, 32).is_some() {
            return root.display().to_string();
        }
    }
    String::new()
}

fn load_tray_ksni_icons() -> Vec<ksni::Icon> {
    if let Some(path) = tray_icon_path() {
        let icons = pixmaps_from_png_file(&path);
        if !icons.is_empty() {
            return icons;
        }
    }

    if let Some(path) = branding_app_icon_path() {
        let icons = pixmaps_from_png_file(&path);
        if !icons.is_empty() {
            return icons;
        }
    }

    for size in [256_i32] {
        if let Some(path) = hicolor_app_icon_path(size) {
            let icons = pixmaps_from_png_file(&path);
            if !icons.is_empty() {
                return icons;
            }
        }
    }

    embedded_tray_pixmaps()
}

fn embedded_tray_pixmaps() -> Vec<ksni::Icon> {
    #[cfg(has_embedded_tray_icon)]
    {
        let loader = gdk_pixbuf::PixbufLoader::new();
        if loader.write(EMBEDDED_TRAY_ICON).is_ok() && loader.close().is_ok() {
            if let Some(pb) = loader.pixbuf() {
                return pixmaps_from_pixbuf(&pb);
            }
        }
    }
    Vec::new()
}

fn hicolor_app_icon_path(size: i32) -> Option<PathBuf> {
    icon_root_candidates()
        .into_iter()
        .find_map(|root| hicolor_app_icon_path_in_root(&root, size))
}

fn hicolor_app_icon_path_in_root(root: &Path, size: i32) -> Option<PathBuf> {
    let path = root
        .join("hicolor")
        .join(format!("{size}x{size}"))
        .join("apps")
        .join(format!("{ICON_NAME}.png"));
    path.is_file().then_some(path)
}

fn pixmaps_from_png_file(path: &Path) -> Vec<ksni::Icon> {
    let Ok(source) = Pixbuf::from_file(path) else {
        return Vec::new();
    };
    pixmaps_from_pixbuf(&source)
}

fn pixmaps_from_pixbuf(source: &Pixbuf) -> Vec<ksni::Icon> {
    let trimmed = trim_to_content(source);
    let squared = square_with_padding(&trimmed, TRAY_ICON_PADDING_FRAC);
    let mut icons = Vec::new();
    for size in TRAY_PIXMAP_SIZES {
        if let Some(pixbuf) = tray_pixbuf_at_size(&squared, size) {
            icons.push(pixbuf_to_ksni_icon(&pixbuf));
        }
    }
    icons
}

/// Scale up slightly and crop so the tray glyph matches circular neighbor icons.
fn tray_pixbuf_at_size(source: &Pixbuf, size: i32) -> Option<Pixbuf> {
    let inner = ((size as f32) * TRAY_ICON_ZOOM).round().max(1.0) as i32;
    let scaled = source.scale_simple(inner, inner, gdk_pixbuf::InterpType::Bilinear)?;
    if inner <= size {
        let Some(dest) = Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, size, size) else {
            return Some(scaled);
        };
        dest.fill(0x0000_0000);
        let x_off = (size - inner) / 2;
        let y_off = (size - inner) / 2;
        scaled.composite(
            &dest,
            x_off,
            y_off,
            inner,
            inner,
            0.0,
            0.0,
            1.0,
            1.0,
            gdk_pixbuf::InterpType::Nearest,
            255,
        );
        return Some(dest);
    }
    let offset = (inner - size) / 2;
    Some(scaled.new_subpixbuf(offset, offset, size, size))
}

/// Drop transparent margins so the tray glyph fills the panel slot like other app icons.
fn trim_to_content(pixbuf: &Pixbuf) -> Pixbuf {
    let Some((x, y, w, h)) = visible_content_bounds(pixbuf) else {
        return pixbuf.clone();
    };
    if w == pixbuf.width() && h == pixbuf.height() {
        return pixbuf.clone();
    }
    pixbuf.new_subpixbuf(x, y, w, h)
}

fn square_with_padding(pixbuf: &Pixbuf, padding_frac: f32) -> Pixbuf {
    let w = pixbuf.width();
    let h = pixbuf.height();
    let side = w.max(h);
    let pad = if padding_frac <= 0.0 {
        0
    } else {
        ((side as f32) * padding_frac).round().max(1.0) as i32
    };
    if pad == 0 && w == side && h == side {
        return pixbuf.clone();
    }
    let canvas = side + 2 * pad;

    let Some(dest) = Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, canvas, canvas) else {
        return pixbuf.clone();
    };
    dest.fill(0x0000_0000);

    let x_off = (canvas - w) / 2;
    let y_off = (canvas - h) / 2;
    pixbuf.composite(
        &dest,
        x_off,
        y_off,
        w,
        h,
        0.0,
        0.0,
        1.0,
        1.0,
        gdk_pixbuf::InterpType::Nearest,
        255,
    );
    dest
}

fn visible_content_bounds(pixbuf: &Pixbuf) -> Option<(i32, i32, i32, i32)> {
    let width = pixbuf.width();
    let height = pixbuf.height();
    if width <= 0 || height <= 0 {
        return None;
    }

    let n_channels = pixbuf.n_channels() as usize;
    let rowstride = pixbuf.rowstride() as usize;
    let pixels = unsafe { pixbuf.pixels() };
    const ALPHA_THRESHOLD: u8 = 16;

    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0_i32;
    let mut max_y = 0_i32;

    for y in 0..height {
        for x in 0..width {
            let offset = y as usize * rowstride + x as usize * n_channels;
            let alpha = if n_channels >= 4 {
                pixels[offset + 3]
            } else {
                255
            };
            if alpha > ALPHA_THRESHOLD {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if max_x < min_x || max_y < min_y {
        return None;
    }

    Some((min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))
}

/// KSNI expects ARGB32 (RGBA → rotate to ARGB, see `ksni` examples).
fn pixbuf_to_ksni_icon(pixbuf: &Pixbuf) -> ksni::Icon {
    let width = pixbuf.width();
    let height = pixbuf.height();
    let n_channels = pixbuf.n_channels() as usize;
    let rowstride = pixbuf.rowstride() as usize;
    let pixels = unsafe { pixbuf.pixels() };
    let mut data = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height as usize {
        for x in 0..width as usize {
            let offset = y * rowstride + x * n_channels;
            let mut pixel = [0u8; 4];
            pixel[0] = pixels[offset];
            pixel[1] = pixels[offset + 1];
            pixel[2] = pixels[offset + 2];
            pixel[3] = if n_channels >= 4 {
                pixels[offset + 3]
            } else {
                0xff
            };
            pixel.rotate_right(1);
            data.extend_from_slice(&pixel);
        }
    }

    ksni::Icon {
        width,
        height,
        data,
    }
}

fn icon_root_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(dir) = std::env::var("BACKUPPILOT_ICON_DIR") {
        roots.push(PathBuf::from(dir));
    }

    roots.push(PathBuf::from("/app/share/icons"));
    roots.push(PathBuf::from("/usr/share/icons"));

    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/icons"));
    }

    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/icons"));

    roots
}

pub fn refresh_on_main_loop() {
    glib::idle_add_local(|| {
        apply_default_window_icon();
        glib::ControlFlow::Break
    });
}
