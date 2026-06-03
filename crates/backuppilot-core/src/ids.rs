//! Reverse-DNS-Kennungen (beim Build per BACKUPPILOT_APP_ID setzbar).

/// GTK-/Flatpak-/Desktop-App-ID, Icon-Name.
pub const APP_ID: &str = env!("BACKUPPILOT_APP_ID");

/// D-Bus-Well-known-Name des Daemons.
pub const DBUS_NAME: &str = env!("BACKUPPILOT_DBUS_NAME");

/// D-Bus-Objektpfad (z. B. `/ch/OneSystems/Daemon`).
pub const DBUS_PATH: &str = env!("BACKUPPILOT_DBUS_PATH");

/// Wie APP_ID (hicolor-Icons).
pub const ICON_NAME: &str = APP_ID;

/// Dateiname unter `~/.config/autostart/`.
pub const AUTOSTART_DESKTOP: &str = concat!(env!("BACKUPPILOT_APP_ID"), ".desktop");

/// Dateiname unter `~/.local/share/dbus-1/services/`.
pub const DBUS_SERVICE_FILE: &str = concat!(env!("BACKUPPILOT_DBUS_NAME"), ".service");
