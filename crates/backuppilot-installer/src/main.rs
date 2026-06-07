//! BackupPilot self-extracting Windows installer / uninstaller.
//!
//! Run without args  -> installs to %LOCALAPPDATA%\BackupPilot\,
//!                      registers autostart and Add/Remove Programs entry.
//! Run --uninstall   -> stops the daemon, removes all files and registry entries.
#![windows_subsystem = "windows"]

#[cfg(windows)]
mod install {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    const APP_NAME: &str = "BackupPilot";
    const UNINSTALL_KEY: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\BackupPilot";

    // Binaries embedded at compile time.
    const DAEMON: &[u8] = include_bytes!(env!("BACKUPPILOT_DAEMON_EXE"));
    const GUI: &[u8]    = include_bytes!(env!("BACKUPPILOT_GUI_EXE"));
    const CLI: &[u8]    = include_bytes!(env!("BACKUPPILOT_CLI_EXE"));

    fn install_dir() -> PathBuf {
        let local = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA not set");
        PathBuf::from(local).join(APP_NAME)
    }

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path)?;
        f.write_all(bytes)?;
        Ok(())
    }

    fn set_autostart(exe: &Path) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run) = hkcu.open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            KEY_SET_VALUE,
        ) {
            let _ = run.set_value("BackupPilot Daemon", &exe.to_string_lossy().as_ref());
        }
    }

    fn remove_autostart() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run) = hkcu.open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            KEY_SET_VALUE,
        ) {
            let _ = run.delete_value("BackupPilot Daemon");
        }
    }

    fn register_uninstall(installer_path: &Path) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(UNINSTALL_KEY)
            .expect("cannot create Uninstall key");
        let uninstall_cmd = format!(
            "\"{}\" --uninstall",
            installer_path.to_string_lossy()
        );
        let _ = key.set_value("DisplayName",          &APP_NAME);
        let _ = key.set_value("UninstallString",      &uninstall_cmd);
        let _ = key.set_value("DisplayIcon",          &installer_path.to_string_lossy().as_ref());
        let _ = key.set_value("Publisher",            &"BackupPilot Contributors");
        let _ = key.set_value("NoModify",             &1u32);
        let _ = key.set_value("NoRepair",             &1u32);
    }

    fn remove_uninstall_entry() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all(UNINSTALL_KEY);
    }

    fn create_shortcut(target: &Path) {
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

    fn remove_shortcut() {
        let start_menu = std::env::var("APPDATA").unwrap_or_default();
        let lnk = format!(
            r"{}\Microsoft\Windows\Start Menu\Programs\BackupPilot.lnk",
            start_menu
        );
        let _ = std::fs::remove_file(&lnk);
    }

    fn stop_daemon() {
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "backuppilot-daemon.exe"])
            .status();
        std::thread::sleep(std::time::Duration::from_millis(600));
    }

    fn stop_gui() {
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "backuppilot.exe"])
            .status();
        std::thread::sleep(std::time::Duration::from_millis(400));
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

    fn confirm(title: &str, msg: &str) -> bool {
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "[System.Windows.Forms.MessageBox]::Show('{}','{}',\
                     [System.Windows.Forms.MessageBoxButtons]::YesNo,\
                     [System.Windows.Forms.MessageBoxIcon]::Question)",
                    msg.replace('\'', "`'"),
                    title
                ),
            ])
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim() == "Yes",
            Err(_) => false,
        }
    }

    pub fn run_install() {
        let dir = install_dir();

        if let Err(e) = std::fs::create_dir_all(&dir) {
            msgbox("BackupPilot Setup", &format!("Failed to create install directory:\n{e}"));
            return;
        }

        stop_daemon();
        stop_gui();

        let files = [
            ("backuppilot-daemon.exe",   DAEMON),
            ("backuppilot.exe",          GUI),
            ("backuppilot-cli.exe",      CLI),
        ];
        for (name, bytes) in &files {
            if let Err(e) = write_file(&dir, name, bytes) {
                msgbox("BackupPilot Setup", &format!("Failed to write {name}:\n{e}"));
                return;
            }
        }

        // Copy the installer itself into the install dir so uninstall can be launched
        // from Add/Remove Programs without needing the original setup exe.
        let installer_dst = dir.join("BackupPilot-Setup.exe");
        if let Ok(self_exe) = std::env::current_exe() {
            let _ = std::fs::copy(&self_exe, &installer_dst);
        }

        let daemon_exe = dir.join("backuppilot-daemon.exe");
        let gui_exe    = dir.join("backuppilot.exe");

        set_autostart(&daemon_exe);
        create_shortcut(&gui_exe);
        register_uninstall(&installer_dst);

        let _ = Command::new(&daemon_exe).spawn();
        let _ = Command::new(&gui_exe).spawn();

        msgbox(
            "BackupPilot Setup",
            "BackupPilot has been installed successfully.\n\nThe application is now starting.",
        );
    }

    pub fn run_uninstall() {
        if !confirm(
            "BackupPilot Uninstall",
            "This will remove BackupPilot from your computer.\n\nContinue?",
        ) {
            return;
        }

        stop_daemon();
        stop_gui();

        remove_autostart();
        remove_shortcut();
        remove_uninstall_entry();

        let dir = install_dir();

        // Schedule directory removal via cmd /c timeout so this process can exit first
        // (the installer exe inside the dir is still running at this point).
        let dir_str = dir.to_string_lossy().into_owned();
        let _ = Command::new("cmd")
            .args([
                "/c",
                "timeout /t 2 /nobreak >nul && rmdir /s /q",
                &dir_str,
            ])
            .spawn();

        msgbox(
            "BackupPilot Uninstall",
            "BackupPilot has been uninstalled successfully.",
        );
    }
}

fn main() {
    #[cfg(windows)]
    {
        let uninstall = std::env::args().any(|a| a == "--uninstall");
        if uninstall {
            install::run_uninstall();
        } else {
            install::run_install();
        }
    }

    #[cfg(not(windows))]
    eprintln!("This installer is for Windows only.");
}
