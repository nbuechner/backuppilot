![BackupPilot Logo](docs/logo.png)

# BackupPilot

**A native Linux desktop backup client for [Proxmox Backup Server](https://www.proxmox.com/en/proxmox-backup-server) (PBS).**

BackupPilot wraps the official `proxmox-backup-client` in a modern GNOME app (GTK4 and Libadwaita) with a background service. You can protect local files and folders, run scheduled backups, monitor job health, and restore from snapshots — without using the command line.

- **Website:** [onesystems.ch](https://www.onesystems.ch)
- **Source & issues:** [git.onesystems.ch/backuppilot](https://git.onesystems.ch/backuppilot)
- **Support:** [Submit a ticket](https://my.onesystems.ch/submitticket.php)

---

## What you need

| Requirement | Notes |
|-------------|--------|
| **Linux desktop** | GNOME or another desktop with GTK4 / Libadwaita |
| **Proxmox Backup Server** | A reachable PBS instance with a repository you can use |
| **`proxmox-backup-client`** | Must be installed on the system (or on the host when using Flatpak) |

Flatpak builds call `proxmox-backup-client` on the host through a wrapper. See the Flatpak install notes in the repository (`packaging/flatpak/`) if PBS backups fail inside the sandbox.

---

## Main features

- **Backup profiles** — Multiple independent profiles with source paths, exclusions, and schedules (hourly, daily, weekly, at login, or custom cron)
- **Incremental backups** — To PBS via `proxmox-backup-client`; start manually or let the background service run jobs automatically
- **Restore** — Browse snapshots and restore files or folders to the original path or another location, with overwrite protection
- **Encryption & limits** — Optional client-side encryption keys and per-profile bandwidth limits
- **Smart start conditions** — Optional checks for AC power, reachable PBS, or selected network connections before a job starts
- **Health monitoring** — Warnings when backups are overdue, with configurable thresholds
- **History & logs** — Run history with status, duration, and error details
- **Desktop integration** — System tray, quick actions, and notifications (English, German, French, Italian)

---

## Installing BackupPilot

Pre-built packages are the easiest way to get started:

| Format | Typical use |
|--------|-------------|
| **`.deb`** | Debian, Ubuntu, and derivatives |
| **`.rpm`** | Fedora, openSUSE, RHEL-compatible systems |
| **`.flatpak`** | Sandboxed install; requires the GNOME Platform runtime from [Flathub](https://flathub.org) unless you use an offline bundle |

Download the package that matches your system from the publisher’s website or release archive. After installation, launch **BackupPilot** from your application menu.

The app installs a user service (`backuppilot-daemon`) that handles scheduled backups. It should start automatically with your desktop session; you can also enable **Start BackupPilot in the background** in the app settings.

**Latest releases:**

[https://git.onesystems.ch/backuppilot/-/releases](https://git.onesystems.ch/backuppilot/-/releases)

---

## Quick start

1. **Install** `proxmox-backup-client` if it is not already on your system (from your distribution or Proxmox repositories).
2. **Open BackupPilot** and follow the setup hints on the dashboard.
3. **Create a backup profile:**
   - Add the folders or files you want to back up
   - Connect to your PBS repository (server, datastore, credentials; optional namespace and backup ID)
   - Optionally set encryption, bandwidth limits, and a schedule
4. **Run a test backup** from the profile or dashboard and check the activity log.
5. **Restore when needed** from the restore view: pick a snapshot, browse the archive, and choose where files should go.

Before each scheduled run, BackupPilot can run **preflight checks** (network and PBS reachability) so jobs do not start when the server is unavailable.

---

## Where your data lives

| Item | Location |
|------|----------|
| Database | `~/.local/share/backuppilot/backuppilot.db` |
| Configuration | `~/.config/backuppilot/` |

Uninstalling the app does not remove these directories automatically; back them up or delete them manually if you no longer need them.

---

## Tips

- Use **`backuppilot --help`** for command-line options (e.g. opening a specific view).
- For crashes or a frozen UI, try: `RUST_BACKTRACE=1 backuppilot --debug`
- Interface language follows your system locale when a translation is available.

---

## Building from source

This section is for developers and advanced users who want to compile BackupPilot themselves.

### Repository layout

The `App/` directory is a Rust workspace with these crates:

| Crate | Binary | Role |
|-------|--------|------|
| `backuppilot-core` | — | Models, SQLite, PBS client integration |
| `backuppilot-daemon` | `backuppilot-daemon` | D-Bus background service |
| `backuppilot-gui` | `backuppilot` | GTK4 / Libadwaita user interface |
| `backuppilot-cli` | `backuppilot-cli` | Command-line tools |

### Prerequisites

- **Rust** ≥ 1.78 ([rustup](https://rustup.rs))
- **`proxmox-backup-client`** on the system (for real backup runs)
- **Development libraries:** GTK4, Libadwaita, gettext

**Debian / Ubuntu:**

```bash
sudo apt install build-essential pkg-config \
  libgtk-4-dev libadwaita-1-dev gettext libintl-dev \
  libnotify-bin python3-pil
```

`libnotify-bin` provides `notify-send` for desktop notifications when backups finish.

### Recommended: build helper at repository root

From the repository root (parent of `App/`):

```bash
./build.sh              # interactive menu
./build.sh local 1.0.0   # release build for local testing
```

This keeps the version in `VERSION` and `App/Cargo.toml` in sync and can produce `.deb`, `.rpm`, and Flatpak bundles under `build/dist/`. Run `./build.sh help` for all targets.

### Manual build and run

```bash
cd App
cargo build --release

# Background service (separate terminal or session)
./target/release/backuppilot-daemon

# GUI
./target/release/backuppilot

# Verbose diagnostics
RUST_BACKTRACE=1 ./target/release/backuppilot --debug
```

### Manual install (user session)

```bash
cd App
cargo install --path crates/backuppilot-daemon
cargo install --path crates/backuppilot-gui

mkdir -p ~/.config/systemd/user
cp data/backuppilot-daemon.service ~/.config/systemd/user/
cp data/ch.onesystems.backuppilot.daemon.service ~/.local/share/dbus-1/services/

systemctl --user daemon-reload
systemctl --user enable --now backuppilot-daemon.service
```

### Icons

App icons are generated from `Marketing/Icon-Dark.png`:

```bash
./scripts/generate-icons.sh
```

### Translations

Languages are listed in `po/LINGUAS` (German, English, French, Italian).

```bash
./scripts/i18n-update.sh   # refresh .pot and .po (optional: .venv-i18n)
./scripts/i18n-compile.sh
LANG=de_DE.UTF-8 ./target/release/backuppilot
```

---

## License

BackupPilot is licensed under the **GNU Affero General Public License v3.0 or later** (AGPL-3.0-or-later).
