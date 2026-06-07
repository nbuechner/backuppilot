//! BackupPilot self-extracting Windows installer.
//!
//! Embeds the three release binaries at compile time and extracts them to
//! %LOCALAPPDATA%\BackupPilot\ on first run.  Sets the HKCU Run key so the
//! daemon starts at login, creates a Start Menu shortcut, and launches the app.
#![windows_subsystem = "windows"]

#[cfg(windows)]
mod install {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    // Binaries embedded at compile time.
    // Build the Windows binaries first, then compile this crate.
    const DAEMON: &[u8] =
        include_bytes!(env!("BACKUPPILOT_DAEMON_EXE"));
    const GUI: &[u8] =
        include_bytes!(env!("BACKUPPILOT_GUI_EXE"));
    const CLI: &[u8] =
        include_bytes!(env!("BACKUPPILOT_CLI_EXE"));

    fn install_dir() -> PathBuf {
        let local = std::env::var("LOCALAPPDATA")
            .expect("LOCALAPPDATA not set");
        PathBuf::from(local).join("BackupPilot")
    }

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path)?;
        f.write_all(bytes)?;
        Ok(())
    }

    fn set_autostart(exe: &Path) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run = hkcu
            .open_subkey_with_flags(
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
                KEY_SET_VALUE,
            )
            .expect("cannot open Run key");
        run.set_value("BackupPilot Daemon", &exe.to_string_lossy().as_ref())
            .expect("cannot set Run value");
    }

    fn create_shortcut(target: &Path) {
        // Use a PowerShell one-liner — avoids COM dependency in cross-compiled binary
        let target_str = target.to_string_lossy();
        let start_menu = std::env::var("APPDATA").unwrap_or_default();
        let lnk = format!(
            r"{}\Microsoft\Windows\Start Menu\Programs\BackupPilot.lnk",
            start_menu
        );
        let ps = format!(
            "$s=(New-Object -COM WScript.Shell).CreateShortcut('{lnk}');\
             $s.TargetPath='{target_str}';\
             $s.Save()"
        );
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .status();
    }

    fn stop_existing_daemon() {
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "backuppilot-daemon.exe"])
            .status();
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    fn msgbox(title: &str, msg: &str) {
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "[System.Windows.Forms.MessageBox]::Show('{}','{}',\
                     [System.Windows.Forms.MessageBoxButtons]::OK,\
                     [System.Windows.Forms.MessageBoxIcon]::Information)",
                    msg.replace('\'', "`'"),
                    title
                ),
            ])
            .status();
    }

    pub fn run() {
        let dir = install_dir();

        if let Err(e) = std::fs::create_dir_all(&dir) {
            msgbox("BackupPilot Setup", &format!("Failed to create install directory:\n{e}"));
            return;
        }

        stop_existing_daemon();

        let files = [
            ("backuppilot-daemon.exe", DAEMON),
            ("backuppilot.exe",        GUI),
            ("backuppilot-cli.exe",    CLI),
        ];
        for (name, bytes) in &files {
            if let Err(e) = write_file(&dir, name, bytes) {
                msgbox("BackupPilot Setup", &format!("Failed to write {name}:\n{e}"));
                return;
            }
        }

        let daemon_exe = dir.join("backuppilot-daemon.exe");
        let gui_exe    = dir.join("backuppilot.exe");

        set_autostart(&daemon_exe);
        create_shortcut(&gui_exe);

        // Start daemon
        let _ = Command::new(&daemon_exe).spawn();

        // Launch GUI
        let _ = Command::new(&gui_exe).spawn();

        msgbox(
            "BackupPilot Setup",
            "BackupPilot has been installed successfully.\n\nThe application is now starting.",
        );
    }
}

fn main() {
    #[cfg(windows)]
    install::run();

    #[cfg(not(windows))]
    eprintln!("This installer is for Windows only.");
}
