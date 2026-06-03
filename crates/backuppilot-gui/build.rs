#[path = "../backuppilot-core/build/emit_app_ids.rs"]
mod emit_app_ids;

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_embedded_tray_icon)");
    // GTK/Libadwaita beim ersten Fensteraufbau: Standard-Stack (8 MiB) reicht in Flatpak oft nicht.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        println!("cargo:rustc-link-arg=-Wl,-z,stack-size=8388608");
    }
    emit_app_ids::emit();
    embed_tray_icon();
}

fn embed_tray_icon() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dest = out_dir.join("embedded-tray-icon.png");

    let candidates = [
        manifest.join("../../data/branding/icon.png"),
        manifest.join("../../Marketing/Icon-Dark.png"),
    ];

    for src in candidates {
        if src.is_file() {
            println!("cargo:rerun-if-changed={}", src.display());
            std::fs::copy(&src, &dest).expect("copy embedded tray icon");
            println!("cargo:rustc-check-cfg=cfg(has_embedded_tray_icon)");
            println!("cargo:rustc-cfg=has_embedded_tray_icon");
            return;
        }
    }

    println!(
        "cargo:warning=No tray icon source — run App/scripts/generate-icons.sh"
    );
}
