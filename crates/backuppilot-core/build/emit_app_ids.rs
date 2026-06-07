//! Gemeinsame Build-Logik für App-ID / D-Bus (von build.rs der Workspace-Crates).

use std::path::PathBuf;

fn app_and_dbus_names() -> (String, String, String) {
    let app_id = std::env::var("BACKUPPILOT_APP_ID")
        .unwrap_or_else(|_| "ch.onesystems.backuppilot".to_string());

    let dbus_name = std::env::var("BACKUPPILOT_DBUS_NAME")
        .unwrap_or_else(|_| format!("{app_id}.daemon"));

    let dbus_path = format!("/{}", dbus_name.replace('.', "/"));
    (app_id, dbus_name, dbus_path)
}

pub fn emit() {
    let (app_id, dbus_name, dbus_path) = app_and_dbus_names();
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let pkg = std::env::var("CARGO_PKG_NAME").unwrap_or_default();

    println!("cargo:rerun-if-env-changed=BACKUPPILOT_APP_ID");
    println!("cargo:rerun-if-env-changed=BACKUPPILOT_DBUS_NAME");
    println!("cargo:rustc-env=BACKUPPILOT_APP_ID={app_id}");
    println!("cargo:rustc-env=BACKUPPILOT_DBUS_NAME={dbus_name}");
    println!("cargo:rustc-env=BACKUPPILOT_DBUS_PATH={dbus_path}");

    if pkg == "backuppilot-daemon" {
        let tpl = include_str!("templates/ipc_handler.inc.rs");
        std::fs::write(out_dir.join("ipc_handler.rs"), tpl).expect("write ipc_handler.rs");
    }
}
