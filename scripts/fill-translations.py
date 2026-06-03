#!/usr/bin/env python3
"""Fill missing translations in de.po and en.po (en: copy msgid to msgstr)."""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PO_DIR = ROOT / "po"

DE: dict[str, str] = {
    "Advanced": "Erweitert",
    "Advanced mode": "Erweiterter Modus",
    "Show expert options in settings and when editing profiles.": (
        "Expertenoptionen in Einstellungen und im Profil-Editor anzeigen."
    ),
    "Show namespace, excludes, conditions, encryption, health, and preflight.": (
        "Namespace, Ausschlüsse, Bedingungen, Verschlüsselung, Gesundheit und Vorabprüfung anzeigen."
    ),
    "French": "Französisch",
    "Italian": "Italienisch",
    "Desktop (background service)": "Desktop (Hintergrunddienst)",
    "Host CLI (backuppilot-cli)": "Host-CLI (backuppilot-cli)",
    "Install backuppilot-cli on the host to enable privileged or unattended backups.": (
        "Installieren Sie backuppilot-cli auf dem Host, um privilegierte oder "
        "unbeaufsichtigte Backups zu ermöglichen."
    ),
    "Use Host CLI for paths outside your user account or systemd/cron schedules.": (
        "Host-CLI für Pfade außerhalb Ihres Benutzerkontos oder für systemd/Cron-"
        "Zeitpläne verwenden."
    ),
    "Execution": "Ausführung",
    "How backups run": "Wie Backups ausgeführt werden",
    "Desktop uses the BackupPilot background service. Host CLI runs via backuppilot-cli on the system (for root paths or scheduled jobs without a logged-in session).": (
        "Desktop nutzt den BackupPilot-Hintergrunddienst. Die Host-CLI läuft über "
        "backuppilot-cli auf dem System (für Root-Pfade oder geplante Jobs ohne "
        "angemeldete Sitzung)."
    ),
    "Host CLI is not installed. Install backuppilot-cli on the system or choose Desktop execution.": (
        "Host-CLI ist nicht installiert. Installieren Sie backuppilot-cli auf dem "
        "System oder wählen Sie Desktop-Ausführung."
    ),
    "No folders selected yet.": "Noch keine Ordner ausgewählt.",
    "Browse…": "Durchsuchen…",
    "Add folder": "Ordner hinzufügen",
    "Choose a directory on this computer.": "Verzeichnis auf diesem Computer wählen.",
    "Add exclude pattern": "Ausschlussmuster hinzufügen",
    "For example *.tmp or node_modules": "Zum Beispiel *.tmp oder node_modules",
    "Add…": "Hinzufügen…",
    "Exclude pattern": "Ausschlussmuster",
    "Pattern": "Muster",
    "Remove": "Entfernen",
    "Directories on this computer that are sent to PBS.": (
        "Verzeichnisse auf diesem Computer, die an PBS gesendet werden."
    ),
    "Files matching these patterns are not included in the backup.": (
        "Dateien, die diesen Mustern entsprechen, werden nicht gesichert."
    ),
    "No exclude patterns yet , backups include all files under the selected folders.": (
        "Noch keine Ausschlussmuster, Backups enthalten alle Dateien in den gewählten Ordnern."
    ),
    "One pattern per entry. * matches part of a name (e.g. *.tmp). Use **/ only if a folder should be skipped in all subfolders , e.g. **/node_modules.": (
        "Ein Muster pro Eintrag. * passt auf Teile des Namens (z. B. *.tmp). **/ nur, wenn ein Ordner in allen Unterordnern ausgeschlossen werden soll, z. B. **/node_modules."
    ),
    "One glob pattern per entry. Matches paths inside your backup folders.": (
        "Ein Glob-Muster pro Eintrag. Trifft auf Pfade in Ihren Backup-Ordnern zu."
    ),
    "Test…": "Testen…",
    "pending": "ausstehend",
    "running": "läuft",
    "successful": "erfolgreich",
    "failed": "fehlgeschlagen",
    "skipped": "übersprungen",
    "{status}, {when} — {msg}": "{status}, {when} - {msg}",
    "{status}, {when}": "{status}, {when}",
    "{status}, {when} — {size}": "{status}, {when} - {size}",
    "PBS must be reachable": "PBS muss erreichbar sein",
    "DNS, port, and login are checked before each backup.": (
        "DNS, Port und Anmeldung werden vor jedem Backup geprüft."
    ),
    "Cron schedule is required for advanced mode.": (
        "Für den erweiterten Modus ist ein Cron-Zeitplan erforderlich."
    ),
    "Preflight checks": "Vorabprüfungen",
    "DNS, network port, PBS login, paths, and backup conditions for this profile.": (
        "DNS, Netzwerkport, PBS-Anmeldung, Pfade und Backup-Bedingungen für dieses Profil."
    ),
    "Run preflight now": "Vorabprüfung jetzt ausführen",
    "Check readiness": "Bereitschaft prüfen",
    "Run all checks without starting a backup.": (
        "Alle Prüfungen ausführen, ohne ein Backup zu starten."
    ),
    "No checks yet — tap «Run preflight now».": (
        "Noch keine Prüfungen - tippe auf «Vorabprüfung jetzt ausführen»."
    ),
    "Backup history": "Backup-Verlauf",
    "Backup history {name}": "Backup-Verlauf {name}",
    "No backup runs recorded yet.": "Noch keine Backup-Läufe erfasst.",
    "Restore started in the background.": "Wiederherstellung im Hintergrund gestartet.",
    "Restore could not be started": "Wiederherstellung konnte nicht gestartet werden",
    "{count} file(s) already exist at the target. Overwrite them?": (
        "{count} Datei(en) existieren bereits am Ziel. Überschreiben?"
    ),
    "Files already exist": "Dateien existieren bereits",
    "Overwrite": "Überschreiben",
    "Terminal could not be started": "Terminal konnte nicht gestartet werden",
    "Installation not started": "Installation nicht gestartet",
    "Default conditions and health thresholds for all profiles (applied when you save settings).": (
        "Standard-Bedingungen und Health-Schwellen für alle Profile (beim Speichern der Einstellungen angewendet)."
    ),
    "All scheduled backups are paused in settings": (
        "Alle geplanten Backups sind in den Einstellungen pausiert"
    ),
    "Backup cancelled": "Backup abgebrochen",
    "Cancel backup": "Backup abbrechen",
    "Cancel all running backups": "Alle laufenden Backups abbrechen",
    "Stops every active backup immediately and frees network bandwidth.": (
        "Beendet alle aktiven Backups sofort und gibt Netzwerkbandbreite frei."
    ),
    "Backup cancellation requested…": "Backup-Abbruch angefordert…",
    "No backup is running for this profile.": "Für dieses Profil läuft kein Backup.",
    "Could not cancel backup: {err}": "Backup konnte nicht abgebrochen werden: {err}",
    "Stopping {count} backup(s)…": "{count} Backup(s) werden gestoppt…",
    "No backup is currently running.": "Es läuft derzeit kein Backup.",
    "Could not cancel backups: {err}": "Backups konnten nicht abgebrochen werden: {err}",
    "Cancelled": "Abgebrochen",
    "Backup «{name}» was cancelled": "Backup «{name}» wurde abgebrochen",
    "Proxmox Backup Client installation cancelled": (
        "Installation des Proxmox Backup Client abgebrochen"
    ),
    "A backup for this profile is already running. Use Stop to cancel it first.": (
        "Für dieses Profil läuft bereits ein Backup. Zuerst mit Stopp abbrechen."
    ),
    "Backup server lock: another backup is already running for this backup group. Stop the running backup or wait until it finishes.": (
        "Backup-Server-Sperre: Für diese Backup-Gruppe läuft bereits ein Backup. Laufendes Backup stoppen oder warten, bis es fertig ist."
    ),
    "A backup for this backup target is already running on the server or in BackupPilot.": (
        "Für dieses Backup-Ziel läuft bereits ein Backup auf dem Server oder in BackupPilot."
    ),
    "Another backup is already running for this backup target.": (
        "Für dieses Backup-Ziel läuft bereits ein anderes Backup."
    ),
    "Interrupted (application restarted)": "Unterbrochen (Anwendung neu gestartet)",
    "Backup paths in this snapshot": "Backup-Pfade in diesem Snapshot",
    "Inside archive {name}": "Im Archiv {name}",
    "Expand to preview top-level folders": "Aufklappen, um Ordner der obersten Ebene anzuzeigen",
    "Archive, double-click to open, - to collapse preview": (
        "Archiv, Doppelklick zum Oeffnen, - zum Zuklappen der Vorschau"
    ),
    "Archive {name}": "Archiv {name}",
    "Search backup paths": "Backup-Pfade durchsuchen",
    "No backup archives in this snapshot. Check profile backup paths.": (
        "Keine Backup-Archive in diesem Snapshot. Backup-Pfade im Profil prüfen."
    ),
    "No backup paths match your search.": "Keine Backup-Pfade passen zur Suche.",
    "No files in this archive root (catalog empty or still loading).": (
        "Keine Dateien in der Archiv-Wurzel (Katalog leer oder wird geladen)."
    ),
    "Could not load preview for {archive}: {err}": (
        "Vorschau für {archive} konnte nicht geladen werden: {err}"
    ),
    "Restore blocked to avoid overwriting existing files.": (
        "Wiederherstellung blockiert, um vorhandene Dateien nicht zu überschreiben."
    ),
    "The backup finished successfully.": "Das Backup wurde erfolgreich abgeschlossen.",
    "Skipped by preflight": "Durch Vorabprüfung übersprungen",
    "unknown error": "unbekannter Fehler",
    "Profile enabled": "Profil aktiv",
    "Backup paths configured": "Backup-Pfade konfiguriert",
    "Backup path exists": "Backup-Pfad vorhanden",
    "Write permission on backup path": "Leserecht auf Backup-Pfad",
    "Read permission on backup path": "Leserecht auf Backup-Pfad",
    "no read permission: {path}": "Kein Leserecht: {path}",
    "proxmox-backup-client installed": "proxmox-backup-client installiert",
    "API token available": "API-Token verfügbar",
    "API token stored": "API-Token gespeichert",
    "On AC power": "Am Netzteil",
    "Required network active": "Erforderliches Netzwerk aktiv",
    "VPN connection active": "VPN-Verbindung aktiv",
    "PBS DNS resolution": "PBS-DNS-Auflösung",
    "PBS TCP port reachable": "PBS-TCP-Port erreichbar",
    "PBS authentication": "PBS-Anmeldung",
    "path does not exist: {path}": "Pfad existiert nicht: {path}",
    "no write permission: {path}": "Keine Schreibrechte: {path}",
    "required network not active ({names})": "Erforderliches Netzwerk nicht aktiv ({names})",
    "profile is disabled": "Profil ist deaktiviert",
    "no backup paths configured": "Keine Backup-Pfade konfiguriert",
    "proxmox-backup-client not found": "proxmox-backup-client nicht gefunden",
    "device not on AC power": "Gerät nicht am Netzteil",
    "VPN connection required but not active": "VPN-Verbindung erforderlich, aber nicht aktiv",
    "API token not available for background backups — open the profile and save again": (
        "API-Token für Hintergrund-Backups nicht verfügbar - Profil öffnen und erneut speichern"
    ),
    "Copyright © 2018–{year} OneSystems GmbH (Michael Kleger)": (
        "Copyright © 2018–{year} OneSystems GmbH (Michael Kleger)"
    ),
    "Check for updates": "Nach Updates suchen",
    "Tap to search for a newer version.": "Tippen, um nach einer neueren Version zu suchen.",
    "Checking for updates…": "Suche nach Updates…",
    "Check for updates now": "Jetzt nach Updates suchen",
    "Compare with releases on GitLab.": "Mit Releases auf GitLab vergleichen.",
    "Version, updates and support": "Version, Updates und Support",
    "Install available update": "Verfügbares Update installieren",
    "Version {version}": "Version {version}",
    "Release page": "Release-Seite",
    "Uses GitLab releases for this application.": "Nutzt GitLab-Releases für diese Anwendung.",
    "You are running the latest version.": "Sie verwenden die neueste Version.",
    "Version {version} is available.": "Version {version} ist verfügbar.",
    "Update check failed: {detail}": "Update-Prüfung fehlgeschlagen: {detail}",
    "New version available": "Neue Version verfügbar",
    "Version {version} can be installed.": "Version {version} kann installiert werden.",
    "Last check failed: {detail}": "Letzte Prüfung fehlgeschlagen: {detail}",
    "Installed version {version}": "Installierte Version {version}",
    "Updates not supported": "Updates nicht unterstützt",
    "Open release page": "Release-Seite öffnen",
    "Close": "Schliessen",
    "Version {version} is ready to install.": "Version {version} kann installiert werden.",
    "Install update": "Update installieren",
    "Not now": "Nicht jetzt",
    "Download and install": "Herunterladen und installieren",
    "Downloading update…": "Update wird heruntergeladen…",
    "Installing update…": "Update wird installiert…",
    "Update installed": "Update installiert",
    "Later": "Später",
    "Restart now": "Jetzt neu starten",
    "Update failed": "Update fehlgeschlagen",
    "Could not install the update: {detail}": "Update konnte nicht installiert werden: {detail}",
    "Checks GitLab once per day for new stable or beta releases and can install .deb or .rpm packages after confirmation.": (
        "Prüft GitLab einmal täglich auf neue Stable- oder Beta-Releases und kann nach Bestätigung .deb- oder .rpm-Pakete installieren."
    ),
    "Automatic installation is only available on Debian/Ubuntu (.deb) and Fedora/RHEL (.rpm) systems.": (
        "Automatische Installation ist nur auf Debian/Ubuntu (.deb) und Fedora/RHEL (.rpm) verfügbar."
    ),
    "Version {version} is ready to install.\n\n{notes}": (
        "Version {version} kann installiert werden.\n\n{notes}"
    ),
    "Restart BackupPilot to use the new version. The background service will be restarted.": (
        "Starten Sie BackupPilot neu, um die neue Version zu nutzen. Der Hintergrunddienst wird neu gestartet."
    ),
    "Click to copy to clipboard": "Klicken zum Kopieren in die Zwischenablage",
    "Copied to clipboard": "In die Zwischenablage kopiert",
    "Copy to clipboard": "In Zwischenablage kopieren",
    "Could not copy to clipboard": "Kopieren in die Zwischenablage fehlgeschlagen",
    "Backup in progress": "Backup läuft",
    "Live progress in the GNOME notification center while a backup is running.": (
        "Live-Fortschritt im GNOME-Benachrichtigungszentrum während ein Backup läuft."
    ),
    "Preparing backup…": "Backup wird vorbereitet…",
    "Update check could not be started (background task).": (
        "Update-Prüfung konnte nicht gestartet werden (Hintergrundaufgabe)."
    ),
    # Encryption keys & profile encryption
    "Encryption": "Verschlüsselung",
    "Create and manage backup encryption keys": (
        "Verschlüsselungsschlüssel anlegen und verwalten"
    ),
    "Encrypted": "Verschlüsselt",
    "Not encrypted": "Nicht verschlüsselt",
    "Encrypted with key «{name}»": "Verschlüsselt mit Schlüssel «{name}»",
    "Encrypted backup": "Verschlüsseltes Backup",
    "Encrypted snapshot": "Verschlüsselter Snapshot",
    "Encrypted — key not stored in BackupPilot": (
        "Verschlüsselt — Schlüssel nicht in BackupPilot hinterlegt"
    ),
    "{profile} ({count} encrypted snapshots)": (
        "{profile} ({count} verschlüsselte Snapshots)"
    ),
    "{profile} (assigned)": "{profile} (zugewiesen)",
    "Cannot delete while key is in use": "Löschen nicht möglich — Schlüssel wird noch verwendet",
    "Created {when}": "Erstellt {when}",
    "Usage: {usage}": "Verwendung: {usage}",
    "Last saved: {when}": "Extern gespeichert: {when}",
    "Backup encryption is active for this profile": (
        "Backup-Verschlüsselung ist für dieses Profil aktiv"
    ),
    "Backups from this profile are not encrypted": (
        "Backups dieses Profils sind nicht verschlüsselt"
    ),
    "If you lose an encryption key or its password, your backups cannot be restored — not even by administrators on the backup server.": (
        "Gehen Schlüssel oder Passwort verloren, sind verschlüsselte Backups nicht wiederherstellbar — auch nicht durch Administratoren auf dem Backup-Server."
    ),
    "Create": "Erstellen",
    "Create key": "Schlüssel erstellen",
    "Import key": "Schlüssel importieren",
    "No encryption keys yet. Create or import a key to use with profiles.": (
        "Noch keine Verschlüsselungsschlüssel. Erstellen oder importieren Sie einen Schlüssel für Ihre Profile."
    ),
    "Could not load keys: {err}": "Schlüssel konnten nicht geladen werden: {err}",
    "Save a copy of the key file": "Kopie der Schlüsseldatei speichern",
    "Delete key": "Schlüssel löschen",
    "Password stored": "Passwort gespeichert",
    "Password missing": "Passwort fehlt",
    "{status} — hint: {hint}": "{status} — Hinweis: {hint}",
    "Create encryption key": "Verschlüsselungsschlüssel erstellen",
    "A new PBS encryption key will be created. Save a backup copy immediately (password manager, USB, safe). Without key and password, encrypted backups cannot be restored.": (
        "Es wird ein neuer PBS-Verschlüsselungsschlüssel erzeugt. Speichern Sie sofort eine Sicherungskopie (Passwortmanager, USB, Tresor). Ohne Schlüssel und Passwort sind verschlüsselte Backups nicht wiederherstellbar."
    ),
    "Laptop backup key": "Laptop-Backup-Schlüssel",
    "Encryption password": "Verschlüsselungs-Passwort",
    "Confirm password": "Passwort bestätigen",
    "Password hint (optional)": "Passwort-Hinweis (optional)",
    "Passwords do not match.": "Die Passwörter stimmen nicht überein.",
    "Password must be at least 8 characters.": (
        "Das Passwort muss mindestens 8 Zeichen lang sein."
    ),
    "Key «{name}» created — save a backup copy now.": (
        "Schlüssel «{name}» erstellt — jetzt eine Sicherungskopie speichern."
    ),
    "Could not create key: {err}": "Schlüssel konnte nicht erstellt werden: {err}",
    "Save encryption key backup": "Sicherungskopie des Schlüssels speichern",
    "Store a copy of the key file outside BackupPilot. You need the key file and password to restore encrypted backups.": (
        "Bewahren Sie eine Kopie der Schlüsseldatei ausserhalb von BackupPilot auf. Für die Wiederherstellung verschlüsselter Backups benötigen Sie die Datei und das Passwort."
    ),
    "Save copy…": "Kopie speichern…",
    "Import encryption key": "Verschlüsselungsschlüssel importieren",
    "Choose file…": "Datei wählen…",
    "Select an existing PBS encryption key file (JSON).": (
        "Bestehende PBS-Verschlüsselungsschlüssel-Datei wählen (JSON)."
    ),
    "Select encryption key file": "Verschlüsselungsschlüssel-Datei wählen",
    "Name is required.": "Name ist erforderlich.",
    "Key «{name}» imported.": "Schlüssel «{name}» importiert.",
    "Import failed: {err}": "Import fehlgeschlagen: {err}",
    "Key file is missing on disk.": "Schlüsseldatei fehlt auf der Festplatte.",
    "Could not save key file.": "Schlüsseldatei konnte nicht gespeichert werden.",
    "Key file saved. Keep it separate from this computer and from the backup server.": (
        "Schlüsseldatei gespeichert. Bewahren Sie sie getrennt von diesem Rechner und vom Backup-Server auf."
    ),
    "Delete key «{name}»?": "Schlüssel «{name}» löschen?",
    "Profiles using this key must be changed first.": (
        "Profile mit diesem Schlüssel müssen zuerst geändert werden."
    ),
    "Encryption key deleted.": "Verschlüsselungsschlüssel gelöscht.",
    "Could not delete: {err}": "Löschen fehlgeschlagen: {err}",
    "Encryption key required for restore": (
        "Verschlüsselungsschlüssel für Wiederherstellung erforderlich"
    ),
    "Without this key and its password, encrypted backups cannot be restored — not even by server administrators. Store a backup copy under Encryption in the sidebar.": (
        "Ohne diesen Schlüssel und sein Passwort sind verschlüsselte Backups nicht wiederherstellbar — auch nicht durch Server-Administratoren. Speichern Sie eine Sicherungskopie unter Verschlüsselung in der Seitenleiste."
    ),
    "Backup encryption": "Backup-Verschlüsselung",
    "Encrypt new backups with a PBS key. Existing snapshots stay as they are.": (
        "Neue Backups mit einem PBS-Schlüssel verschlüsseln. Bestehende Snapshots bleiben unverändert."
    ),
    "— No encryption —": "— Keine Verschlüsselung —",
    "Encryption key": "Verschlüsselungsschlüssel",
    "Manage keys under Encryption in the sidebar": (
        "Schlüssel unter Verschlüsselung in der Seitenleiste verwalten"
    ),
    "Created {when}": "Erstellt {when}",
    "Created": "Erstellt",
    "In use": "Verwendung",
    "Last saved": "Extern gespeichert",
    "Not used": "Nicht verwendet",
    "Never": "Nie",
    "Hint: {hint}": "Hinweis: {hint}",
    "In use by: {profiles}": "In Verwendung durch: {profiles}",
    "Not used by any profile": "Von keinem Profil verwendet",
    "Last saved outside BackupPilot: {when}": "Zuletzt ausserhalb von BackupPilot gespeichert: {when}",
    "Never saved outside BackupPilot": "Noch nie ausserhalb von BackupPilot gespeichert",
    # Logs, dashboard, tray, mount (new msgids after source repair)
    "Log": "Protokoll",
    "Latest backup events. Click a row for details.": (
        "Neueste Backup-Ereignisse. Zeile anklicken für Details."
    ),
    "View all": "Alle anzeigen",
    "Disabled , scheduled backups are paused": (
        "Deaktiviert, geplante Backups sind pausiert"
    ),
    "cancelled": "abgebrochen",
    "All scheduled backups are paused": "Alle geplanten Backups sind pausiert",
    "{critical} backup job(s) critically overdue, {warn} warning(s)": (
        "{critical} Backup-Auftrag/Aufträge kritisch überfällig, {warn} Warnung(en)"
    ),
    "{warn} backup job(s) need attention": (
        "{warn} Backup-Auftrag/Aufträge benötigen Aufmerksamkeit"
    ),
    "Application update available": "Anwendungs-Update verfügbar",
    "Type: system event ({kind})": "Typ: Systemereignis ({kind})",
    "Profile: {name} (id {id})": "Profil: {name} (ID {id})",
    "Run id: {id}": "Lauf-ID: {id}",
    "Status: {status}": "Status: {status}",
    "Started (UTC): {ts}": "Gestartet (UTC): {ts}",
    "Finished (UTC): {ts}": "Beendet (UTC): {ts}",
    "Duration: {secs} s": "Dauer: {secs} s",
    "Bytes uploaded: {bytes}": "Hochgeladene Bytes: {bytes}",
    "Snapshot id: {snap}": "Snapshot-ID: {snap}",
    "Error / details:": "Fehler / Details:",
    "Log entry": "Protokolleintrag",
    "{when} , {reason}": "{when}, {reason}",
    "{status}, {when} , {msg}": "{status}, {when}, {msg}",
    "{status}, {when} , {size}": "{status}, {when}, {size}",
    "{when} , not started (preflight or schedule)": (
        "{when}, nicht gestartet (Vorabprüfung oder Zeitplan)"
    ),
    "{size} saved to backup server": "{size} auf Backup-Server gespeichert",
    "{when} , snapshot {snap}": "{when}, Snapshot {snap}",
    "{when} , {size_part}, snapshot {snap}": (
        "{when}, {size_part}, Snapshot {snap}"
    ),
    "{when} , {size_part}": "{when}, {size_part}",
    "Proxmox Backup as a Service , hosted in Switzerland, operated by experts.": (
        "Proxmox Backup as a Service, gehostet in der Schweiz, betrieben von Experten."
    ),
    "Last run: {label} , {msg}": "Letzter Lauf: {label}, {msg}",
    "Without this key and its password, encrypted backups cannot be restored , not even by server administrators. Store a backup copy under Encryption in the sidebar.": (
        "Ohne diesen Schlüssel und sein Passwort sind verschlüsselte Backups nicht wiederherstellbar, auch nicht durch Server-Administratoren. Speichern Sie eine Sicherungskopie unter Verschlüsselung in der Seitenleiste."
    ),
    "(No encryption)": "(Keine Verschlüsselung)",
    "API token not available for background backups , open the profile and save again": (
        "API-Token für Hintergrund-Backups nicht verfügbar, Profil öffnen und erneut speichern"
    ),
    "No checks yet , tap «Run preflight now».": (
        "Noch keine Prüfungen, tippe auf «Vorabprüfung jetzt ausführen»."
    ),
    "If you lose an encryption key or its password, your backups cannot be restored , not even by administrators on the backup server.": (
        "Gehen Schlüssel oder Passwort verloren, sind verschlüsselte Backups nicht wiederherstellbar, auch nicht durch Administratoren auf dem Backup-Server."
    ),
    "{status} , hint: {hint}": "{status}, Hinweis: {hint}",
    "Key «{name}» created , save a backup copy now.": (
        "Schlüssel «{name}» erstellt, jetzt eine Sicherungskopie speichern."
    ),
    "Restore to original location": "An ursprünglichen Ort wiederherstellen",
    "Use the backup source path that matches the selected archive (when available)": (
        "Backup-Quellpfad verwenden, der zum gewählten Archiv passt (falls verfügbar)"
    ),
    "Encrypted , key not stored in BackupPilot": (
        "Verschlüsselt, Schlüssel nicht in BackupPilot hinterlegt"
    ),
    "Choose a restore folder or enable restore to original location.": (
        "Wiederherstellungsordner wählen oder Wiederherstellung am ursprünglichen Ort aktivieren."
    ),
    "Loading…": "Wird geladen…",
    "Archive , double-click to open, - to collapse preview": (
        "Archiv, Doppelklick zum Öffnen, Minus zum Zuklappen der Vorschau"
    ),
    "Mount read-only in file manager…": "Schreibgeschützt im Dateimanager einbinden…",
    "Mounted , click to open in file manager": (
        "Eingebunden, Klick zum Öffnen im Dateimanager"
    ),
    "Mounting…": "Wird eingebunden…",
    "Folder , double-click to open, checkbox to restore all contents": (
        "Ordner, Doppelklick zum Öffnen, Kontrollkästchen stellt gesamten Inhalt wieder her"
    ),
    "Copyright © 2018-{year} OneSystems GmbH (Michael Kleger)": (
        "Copyright © 2018-{year} OneSystems GmbH (Michael Kleger)"
    ),
    "Could not open a terminal , install script copied to clipboard. Run it in a root shell.": (
        "Terminal konnte nicht geöffnet werden, Installationsskript in Zwischenablage kopiert. In einer Root-Shell ausführen."
    ),
    "BackupPilot , installing Proxmox Backup Client": (
        "BackupPilot, installiert Proxmox Backup Client"
    ),
    "Use the static client binary or build from source , see the Proxmox PBS documentation.": (
        "Statische Client-Binärdatei nutzen oder aus Quellen bauen, siehe Proxmox-PBS-Dokumentation."
    ),
    "Keep log entries (days)": "Protokolleinträge behalten (Tage)",
    "Backup runs and system events older than this are removed automatically. Set to 0 to keep all entries.": (
        "Backup-Läufe und Systemereignisse älter als dieser Wert werden automatisch entfernt. 0 behält alle Einträge."
    ),
    "Checked daily by the background service against GitLab releases.": (
        "Täglich vom Hintergrunddienst gegen GitLab-Releases geprüft."
    ),
    "Activity log": "Aktivitätsprotokoll",
    "Limits how much backup history is stored locally. The background service removes older entries once per day.": (
        "Begrenzt, wie viel Backup-Verlauf lokal gespeichert wird. Der Hintergrunddienst entfernt ältere Einträge einmal täglich."
    ),
    "Log retention must be 0 (keep all) or between 7 and 3650 days.": (
        "Protokoll-Aufbewahrung muss 0 (alles behalten) oder zwischen 7 und 3650 Tagen sein."
    ),
    "Resume scheduled backups": "Geplante Backups fortsetzen",
    "Pause scheduled backups": "Geplante Backups pausieren",
    "Open restore": "Wiederherstellung öffnen",
    "Open log": "Protokoll öffnen",
    "Backup running , see Overview for progress. Large backups can take a while.": (
        "Backup läuft, Fortschritt unter Übersicht. Grosse Backups können eine Weile dauern."
    ),
    "Last update check failed: {detail}": "Letzte Update-Prüfung fehlgeschlagen: {detail}",
    "Cancelling backup…": "Backup wird abgebrochen…",
    "Backup «{name}»": "Backup «{name}»",
    "Backup cancelled: {name}": "Backup abgebrochen: {name}",
    "The backup was stopped.": "Das Backup wurde gestoppt.",
    "Mount backup in file manager?": "Backup im Dateimanager einbinden?",
    "Mount «{path}» from snapshot {snapshot} as a read-only folder?\n\n• Requires network access to the backup server while browsing\n• May use noticeable CPU and bandwidth when opening many files\n• Only mount backups you trust\n• Disconnect the mount from the overview when you are done": (
        "«{path}» aus Snapshot {snapshot} als schreibgeschützten Ordner einbinden?\n\n"
        "• Erfordert Netzwerkzugang zum Backup-Server beim Durchsuchen\n"
        "• Kann spürbare CPU-Last und Bandbreite beim Öffnen vieler Dateien verursachen\n"
        "• Nur Backups einbinden, denen Sie vertrauen\n"
        "• Einbindung auf der Übersicht trennen, wenn Sie fertig sind"
    ),
    "Mount": "Einbinden",
    "Mounted at {path}": "Eingebunden unter {path}",
    "Mount failed: {err}": "Einbinden fehlgeschlagen: {err}",
    "Stop exposing «{path}» at {mount}?\n\nFiles in the file manager will no longer be available until you mount again.": (
        "Freigabe von «{path}» unter {mount} beenden?\n\n"
        "Dateien im Dateimanager sind erst nach erneutem Einbinden wieder verfügbar."
    ),
    "Disconnect failed: {err}": "Trennen fehlgeschlagen: {err}",
    "«{path}» mounted read-only": "«{path}» schreibgeschützt eingebunden",
    "Mount disconnected.": "Einbindung getrennt.",
    "Could not disconnect mount.": "Einbindung konnte nicht getrennt werden.",
    "FUSE is not available on this system. Install fuse3 to mount backups in the file manager.": (
        "FUSE ist auf diesem System nicht verfügbar. Installieren Sie fuse3, um Backups im Dateimanager einzubinden."
    ),
    "Filter": "Filter",
    "Backup log": "Backup-Protokoll",
    "Click an entry for full details (timestamps, snapshot id, error text).": (
        "Eintrag anklicken für vollständige Details (Zeitstempel, Snapshot-ID, Fehlertext)."
    ),
    "All profiles": "Alle Profile",
    "Loading log…": "Protokoll wird geladen…",
    "Could not load log:": "Protokoll konnte nicht geladen werden:",
    "No log entries yet.": "Noch keine Protokolleinträge.",
    "No log entries match the current filters.": (
        "Keine Protokolleinträge passen zu den aktuellen Filtern."
    ),
    "In progress": "Läuft",
    "System events": "Systemereignisse",
    "Successful": "Erfolgreich",
    "Failed": "Fehlgeschlagen",
    "Skipped": "Übersprungen",
    "Backup mounted read-only in file manager": (
        "Backup schreibgeschützt im Dateimanager eingebunden"
    ),
    "Backup mount failed": "Backup-Einbindung fehlgeschlagen",
    "Backup mount": "Backup-Einbindung",
    "Backup mount disconnected": "Backup-Einbindung getrennt",
    "Backup mount disconnect failed": "Trennen der Backup-Einbindung fehlgeschlagen",
    "Backup mount disconnect": "Backup-Einbindung trennen",
    "File restore started": "Datei-Wiederherstellung gestartet",
    "File restore completed": "Datei-Wiederherstellung abgeschlossen",
    "File restore failed": "Datei-Wiederherstellung fehlgeschlagen",
    "File restore": "Datei-Wiederherstellung",
    "Key file": "Schlüsseldatei",
    "No file selected yet": "Noch keine Datei gewählt",
    "Choose the PBS key file (JSON), enter a display name and the encryption password, then tap Import.": (
        "PBS-Schlüsseldatei (JSON) wählen, Anzeigename und Verschlüsselungs-Passwort eingeben, dann Importieren."
    ),
    "Could not read the selected file path.": (
        "Pfad der gewählten Datei konnte nicht gelesen werden."
    ),
    "Encryption password is required.": "Verschlüsselungs-Passwort ist erforderlich.",
    "Choose a key file first.": "Zuerst eine Schlüsseldatei wählen.",
    "Import": "Importieren",
}


def escape_po(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def format_msgstr(text: str) -> str:
    if "\n" not in text:
        return f'msgstr "{escape_po(text)}"\n'
    lines = text.split("\n")
    out = 'msgstr ""\n'
    for i, line in enumerate(lines):
        suffix = "\\n" if i < len(lines) - 1 else ""
        out += f'"{escape_po(line)}{suffix}"\n'
    return out


def parse_msgid(lines: list[str], start: int) -> tuple[str, int]:
    i = start
    parts: list[str] = []
    while i < len(lines):
        line = lines[i].strip()
        if line.startswith("msgid "):
            m = re.match(r'msgid "(.*)"', line)
            if m:
                parts.append(m.group(1))
            i += 1
            continue
        if line.startswith('"'):
            m = re.match(r'"(.*)"', line)
            if m:
                parts.append(m.group(1).replace("\\n", "\n"))
            i += 1
            continue
        break
    return "".join(parts), i


def fill_po(path: Path, translations: dict[str, str] | None, copy_en: bool) -> int:
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    filled = 0
    i = 0
    while i < len(lines):
        if lines[i].startswith("msgid "):
            msgid, next_i = parse_msgid(lines, i)
            i = next_i
            if i < len(lines) and lines[i].startswith("msgstr "):
                if lines[i].strip() == 'msgstr ""':
                    # multiline msgstr?
                    j = i + 1
                    while j < len(lines) and lines[j].startswith('"'):
                        j += 1
                    if j == i + 1:  # truly empty
                        text = None
                        if copy_en:
                            text = msgid
                        elif translations and msgid in translations:
                            text = translations[msgid]
                        if text is not None:
                            lines[i : j] = [format_msgstr(text)]
                            filled += 1
                            i = i + 1
                            continue
                i += 1
                continue
        i += 1

    path.write_text("".join(lines), encoding="utf-8")
    return filled


def main() -> int:
    n_de = fill_po(PO_DIR / "de.po", DE, copy_en=False)
    n_en = fill_po(PO_DIR / "en.po", None, copy_en=True)
    print(f"de.po: {n_de} entries filled")
    print(f"en.po: {n_en} entries filled")
    return 0


if __name__ == "__main__":
    sys.exit(main())
