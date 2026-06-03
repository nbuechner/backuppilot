#!/usr/bin/env python3
"""Fill CLI gettext entries in all language .po files."""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "po"

# msgid -> translations per language
CLI: dict[str, dict[str, str]] = {
    "No backups are currently running.": {
        "de": "Derzeit laufen keine Backups.",
        "en": "No backups are currently running.",
        "fr": "Aucune sauvegarde en cours.",
        "it": "Nessun backup in esecuzione.",
    },
    "No snapshots found for profile {name}.": {
        "de": "Keine Snapshots für Profil {name}.",
        "en": "No snapshots found for profile {name}.",
        "fr": "Aucun snapshot pour le profil {name}.",
        "it": "Nessuno snapshot per il profilo {name}.",
    },
    "No archives in snapshot {snapshot}.": {
        "de": "Keine Archive im Snapshot {snapshot}.",
        "en": "No archives in snapshot {snapshot}.",
        "fr": "Aucune archive dans le snapshot {snapshot}.",
        "it": "Nessun archivio nello snapshot {snapshot}.",
    },
    "No entries under '{path}' in {snapshot}/{archive}.": {
        "de": "Keine Einträge unter „{path}“ in {snapshot}/{archive}.",
        "en": "No entries under '{path}' in {snapshot}/{archive}.",
        "fr": "Aucune entrée sous « {path} » dans {snapshot}/{archive}.",
        "it": "Nessuna voce sotto «{path}» in {snapshot}/{archive}.",
    },
    "dir": {"de": "ordner", "en": "dir", "fr": "dossier", "it": "cartella"},
    "file": {"de": "datei", "en": "file", "fr": "fichier", "it": "file"},
    "Restore completed: {target}": {
        "de": "Wiederherstellung abgeschlossen: {target}",
        "en": "Restore completed: {target}",
        "fr": "Restauration terminée : {target}",
        "it": "Ripristino completato: {target}",
    },
    "Conflict": {"de": "Konflikt", "en": "Conflict", "fr": "Conflit", "it": "Conflitto"},
    "No active mounts (including from earlier CLI runs).": {
        "de": "Keine aktiven Mounts (auch nicht aus früheren CLI-Läufen).",
        "en": "No active mounts (including from earlier CLI runs).",
        "fr": "Aucun montage actif (y compris d'anciennes exécutions CLI).",
        "it": "Nessun mount attivo (anche da precedenti esecuzioni CLI).",
    },
    "Mounted at: {path}": {
        "de": "Eingebunden unter: {path}",
        "en": "Mounted at: {path}",
        "fr": "Monté sous : {path}",
        "it": "Montato in: {path}",
    },
    "Mount ID: {id}": {
        "de": "Mount-ID: {id}",
        "en": "Mount ID: {id}",
        "fr": "ID de montage : {id}",
        "it": "ID mount: {id}",
    },
    "All mounts were disconnected.": {
        "de": "Alle Mounts wurden getrennt.",
        "en": "All mounts were disconnected.",
        "fr": "Tous les montages ont été déconnectés.",
        "it": "Tutti i mount sono stati disconnessi.",
    },
    "Provide a mount id (from `mounts`) or use --all.": {
        "de": "Mount-ID angeben (aus `mounts`) oder --all verwenden.",
        "en": "Provide a mount id (from `mounts`) or use --all.",
        "fr": "Indiquez un ID de montage (via `mounts`) ou utilisez --all.",
        "it": "Fornire un ID mount (da `mounts`) o usare --all.",
    },
    "Mount disconnected: {id}": {
        "de": "Mount getrennt: {id}",
        "en": "Mount disconnected: {id}",
        "fr": "Montage déconnecté : {id}",
        "it": "Mount disconnesso: {id}",
    },
    "encrypted": {
        "de": "verschlüsselt",
        "en": "encrypted",
        "fr": "chiffré",
        "it": "crittografato",
    },
    "plain": {"de": "unverschlüsselt", "en": "plain", "fr": "non chiffré", "it": "non crittografato"},
    "No active FUSE mounts under ~/.local/share/backuppilot/mounts/.": {
        "de": "Keine aktiven FUSE-Mounts unter ~/.local/share/backuppilot/mounts/.",
        "en": "No active FUSE mounts under ~/.local/share/backuppilot/mounts/.",
        "fr": "Aucun montage FUSE actif sous ~/.local/share/backuppilot/mounts/.",
        "it": "Nessun mount FUSE attivo in ~/.local/share/backuppilot/mounts/.",
    },
    "GUI mounts are managed by the daemon (see the app for an overview).": {
        "de": "GUI-Mounts werden im Daemon verwaltet (Übersicht in der App).",
        "en": "GUI mounts are managed by the daemon (see the app for an overview).",
        "fr": "Les montages GUI sont gérés par le démon (voir l'application).",
        "it": "I mount GUI sono gestiti dal daemon (vedi l'app).",
    },
    "Disconnect all {count} mounts": {
        "de": "Alle {count} Mounts trennen",
        "en": "Disconnect all {count} mounts",
        "fr": "Déconnecter les {count} montages",
        "it": "Disconnetti tutti i {count} mount",
    },
    "Disconnect mount": {
        "de": "Mount trennen",
        "en": "Disconnect mount",
        "fr": "Déconnecter le montage",
        "it": "Disconnetti mount",
    },
    "Restore files to disk": {
        "de": "Dateien auf die Festplatte wiederherstellen",
        "en": "Restore files to disk",
        "fr": "Restaurer les fichiers sur le disque",
        "it": "Ripristina file su disco",
    },
    "Mount archive read-only (file manager)": {
        "de": "Archiv read-only einbinden (Dateimanager)",
        "en": "Mount archive read-only (file manager)",
        "fr": "Monter l'archive en lecture seule (gestionnaire de fichiers)",
        "it": "Monta archivio in sola lettura (file manager)",
    },
    "Browse files only": {
        "de": "Nur Dateien anzeigen (durchsuchen)",
        "en": "Browse files only",
        "fr": "Parcourir les fichiers uniquement",
        "it": "Sfoglia solo i file",
    },
    "What would you like to do?": {
        "de": "Was möchten Sie tun?",
        "en": "What would you like to do?",
        "fr": "Que souhaitez-vous faire ?",
        "it": "Cosa vuoi fare?",
    },
    "Cancel": {"de": "Abbrechen", "en": "Cancel", "fr": "Annuler", "it": "Annulla"},
    "No profiles configured.": {
        "de": "Keine Profile konfiguriert.",
        "en": "No profiles configured.",
        "fr": "Aucun profil configuré.",
        "it": "Nessun profilo configurato.",
    },
    "Choose profile": {
        "de": "Profil wählen",
        "en": "Choose profile",
        "fr": "Choisir le profil",
        "it": "Scegli profilo",
    },
    "Loading snapshots from PBS …": {
        "de": "Lade Snapshots von PBS …",
        "en": "Loading snapshots from PBS …",
        "fr": "Chargement des snapshots depuis PBS …",
        "it": "Caricamento snapshot da PBS …",
    },
    "No snapshots for profile {name}.": {
        "de": "Keine Snapshots für Profil {name}.",
        "en": "No snapshots for profile {name}.",
        "fr": "Aucun snapshot pour le profil {name}.",
        "it": "Nessuno snapshot per il profilo {name}.",
    },
    "Choose snapshot": {
        "de": "Snapshot wählen",
        "en": "Choose snapshot",
        "fr": "Choisir le snapshot",
        "it": "Scegli snapshot",
    },
    "Resolving archives …": {
        "de": "Ermittle Archive …",
        "en": "Resolving archives …",
        "fr": "Résolution des archives …",
        "it": "Ricerca archivi …",
    },
    "No archives found — check profile paths in the app or load the PBS manifest.": {
        "de": "Keine Archive gefunden — Profil-Pfade in der GUI prüfen oder PBS-Manifest laden.",
        "en": "No archives found — check profile paths in the app or load the PBS manifest.",
        "fr": "Aucune archive — vérifiez les chemins du profil dans l'app ou chargez le manifeste PBS.",
        "it": "Nessun archivio — controlla i percorsi nel profilo nell'app o carica il manifest PBS.",
    },
    "Choose archive / backup path": {
        "de": "Archiv / Backup-Pfad wählen",
        "en": "Choose archive / backup path",
        "fr": "Choisir l'archive / le chemin de sauvegarde",
        "it": "Scegli archivio / percorso backup",
    },
    "FUSE is not available (install the fuse3 package).": {
        "de": "FUSE nicht verfügbar (Paket fuse3 installieren).",
        "en": "FUSE is not available (install the fuse3 package).",
        "fr": "FUSE indisponible (installez le paquet fuse3).",
        "it": "FUSE non disponibile (installare il pacchetto fuse3).",
    },
    "Mount archive read-only? (network access to PBS; only trusted backups)": {
        "de": "Archiv read-only einbinden? (Netzwerkzugriff auf PBS, nur vertrauenswürdige Backups)",
        "en": "Mount archive read-only? (network access to PBS; only trusted backups)",
        "fr": "Monter l'archive en lecture seule ? (accès réseau PBS ; sauvegardes de confiance uniquement)",
        "it": "Montare l'archivio in sola lettura? (accesso di rete a PBS; solo backup attendibili)",
    },
    "Open in file manager?": {
        "de": "Im Dateimanager öffnen?",
        "en": "Open in file manager?",
        "fr": "Ouvrir dans le gestionnaire de fichiers ?",
        "it": "Aprire nel file manager?",
    },
    "Guided mode does not support --json.": {
        "de": "Geführter Modus unterstützt kein --json.",
        "en": "Guided mode does not support --json.",
        "fr": "Le mode guidé ne prend pas en charge --json.",
        "it": "La modalità guidata non supporta --json.",
    },
    "Guided mode requires an interactive terminal.": {
        "de": "Geführter Modus braucht ein interaktives Terminal.",
        "en": "Guided mode requires an interactive terminal.",
        "fr": "Le mode guidé nécessite un terminal interactif.",
        "it": "La modalità guidata richiede un terminale interattivo.",
    },
    "Entire archive": {
        "de": "Gesamtes Archiv",
        "en": "Entire archive",
        "fr": "Archive entière",
        "it": "Intero archivio",
    },
    "Select specific files or folders": {
        "de": "Bestimmte Dateien/Ordner auswählen",
        "en": "Select specific files or folders",
        "fr": "Sélectionner des fichiers ou dossiers",
        "it": "Seleziona file o cartelle specifici",
    },
    "Enter glob patterns (e.g. Documents/**)": {
        "de": "Glob-Muster eingeben (z. B. Documents/**)",
        "en": "Enter glob patterns (e.g. Documents/**)",
        "fr": "Saisir des motifs glob (ex. Documents/**)",
        "it": "Inserire pattern glob (es. Documents/**)",
    },
    "Restore scope": {
        "de": "Umfang der Wiederherstellung",
        "en": "Restore scope",
        "fr": "Portée de la restauration",
        "it": "Ambito del ripristino",
    },
    "Glob patterns (comma-separated for multiple)": {
        "de": "Glob-Muster (kommagetrennt für mehrere)",
        "en": "Glob patterns (comma-separated for multiple)",
        "fr": "Motifs glob (séparés par des virgules)",
        "it": "Pattern glob (separati da virgola)",
    },
    "Restore in progress (may take a long time) …": {
        "de": "Wiederherstellung läuft (kann lange dauern) …",
        "en": "Restore in progress (may take a long time) …",
        "fr": "Restauration en cours (peut prendre du temps) …",
        "it": "Ripristino in corso (può richiedere tempo) …",
    },
    "Select files or folders with Enter. Then choose Done.": {
        "de": "Dateien oder Ordner antippen (Enter). Danach «Fertig» wählen.",
        "en": "Select files or folders with Enter. Then choose Done.",
        "fr": "Sélectionnez avec Entrée. Puis choisissez Terminé.",
        "it": "Seleziona con Invio. Poi scegli Fine.",
    },
    "No space bar needed — each entry runs an action immediately.": {
        "de": "Keine Leertaste nötig — jeder Eintrag führt sofort eine Aktion aus.",
        "en": "No space bar needed — each entry runs an action immediately.",
        "fr": "Pas besoin de barre d'espace — chaque entrée agit immédiatement.",
        "it": "Non serve la barra spaziatrice — ogni voce esegue un'azione subito.",
    },
    "Archive root": {
        "de": "Archiv-Wurzel",
        "en": "Archive root",
        "fr": "Racine de l'archive",
        "it": "Radice archivio",
    },
    "{location} — selected: {count}": {
        "de": "{location} — ausgewählt: {count}",
        "en": "{location} — selected: {count}",
        "fr": "{location} — sélectionné : {count}",
        "it": "{location} — selezionati: {count}",
    },
    "Please select at least one file or folder.": {
        "de": "Bitte mindestens eine Datei oder einen Ordner auswählen.",
        "en": "Please select at least one file or folder.",
        "fr": "Veuillez sélectionner au moins un fichier ou dossier.",
        "it": "Seleziona almeno un file o una cartella.",
    },
    "► Done — choose target folder (nothing selected yet)": {
        "de": "► Fertig — Zielordner wählen (noch nichts ausgewählt)",
        "en": "► Done — choose target folder (nothing selected yet)",
        "fr": "► Terminé — choisir le dossier cible (rien de sélectionné)",
        "it": "► Fine — scegli cartella di destinazione (nessuna selezione)",
    },
    "► Done — restore {count} item(s), choose target folder": {
        "de": "► Fertig — {count} Element(e) wiederherstellen, Zielordner wählen",
        "en": "► Done — restore {count} item(s), choose target folder",
        "fr": "► Terminé — restaurer {count} élément(s), choisir le dossier cible",
        "it": "► Fine — ripristina {count} elemento/i, scegli destinazione",
    },
    "↑ Parent folder": {
        "de": "↑ Übergeordneter Ordner",
        "en": "↑ Parent folder",
        "fr": "↑ Dossier parent",
        "it": "↑ Cartella superiore",
    },
    "− Remove folder: {name}": {
        "de": "− Ordner abwählen: {name}",
        "en": "− Remove folder: {name}",
        "fr": "− Retirer le dossier : {name}",
        "it": "− Rimuovi cartella: {name}",
    },
    "→ Open folder: {name}": {
        "de": "→ Ordner öffnen: {name}",
        "en": "→ Open folder: {name}",
        "fr": "→ Ouvrir le dossier : {name}",
        "it": "→ Apri cartella: {name}",
    },
    "+ Select folder: {name}": {
        "de": "+ Ganzen Ordner auswählen: {name}",
        "en": "+ Select folder: {name}",
        "fr": "+ Sélectionner le dossier : {name}",
        "it": "+ Seleziona cartella: {name}",
    },
    "− Deselect file: {name}": {
        "de": "− Datei abwählen: {name}",
        "en": "− Deselect file: {name}",
        "fr": "− Désélectionner le fichier : {name}",
        "it": "− Deseleziona file: {name}",
    },
    "+ Select file: {name}": {
        "de": "+ Datei auswählen: {name}",
        "en": "+ Select file: {name}",
        "fr": "+ Sélectionner le fichier : {name}",
        "it": "+ Seleziona file: {name}",
    },
    "unknown": {"de": "nicht ermittelbar", "en": "unknown", "fr": "inconnu", "it": "sconosciuto"},
    "Original path ({path})": {
        "de": "Originalpfad ({path})",
        "en": "Original path ({path})",
        "fr": "Chemin d'origine ({path})",
        "it": "Percorso originale ({path})",
    },
    "Enter another target folder …": {
        "de": "Anderen Zielordner eingeben …",
        "en": "Enter another target folder …",
        "fr": "Saisir un autre dossier cible …",
        "it": "Inserire un'altra cartella di destinazione …",
    },
    "Restore to": {
        "de": "Wohin wiederherstellen?",
        "en": "Restore to",
        "fr": "Restaurer vers",
        "it": "Ripristina in",
    },
    "Target directory on disk": {
        "de": "Zielverzeichnis auf der Festplatte",
        "en": "Target directory on disk",
        "fr": "Répertoire cible sur le disque",
        "it": "Directory di destinazione su disco",
    },
    "No target directory specified.": {
        "de": "Kein Zielverzeichnis angegeben.",
        "en": "No target directory specified.",
        "fr": "Aucun répertoire cible indiqué.",
        "it": "Nessuna directory di destinazione specificata.",
    },
    "Overwrite existing files under {target}?": {
        "de": "Vorhandene Dateien unter {target} überschreiben?",
        "en": "Overwrite existing files under {target}?",
        "fr": "Écraser les fichiers existants sous {target} ?",
        "it": "Sovrascrivere i file esistenti in {target}?",
    },
    "Overwrite existing files in {target}?": {
        "de": "Vorhandene Dateien in {target} überschreiben?",
        "en": "Overwrite existing files in {target}?",
        "fr": "Écraser les fichiers existants dans {target} ?",
        "it": "Sovrascrivere i file esistenti in {target}?",
    },
    "Summary:": {"de": "Zusammenfassung:", "en": "Summary:", "fr": "Résumé :", "it": "Riepilogo:"},
    "Scope: entire archive": {
        "de": "Umfang: gesamtes Archiv",
        "en": "Scope: entire archive",
        "fr": "Portée : archive entière",
        "it": "Ambito: intero archivio",
    },
    "Scope: {count} path(s)/pattern(s)": {
        "de": "Umfang: {count} Pfad(e)/Muster",
        "en": "Scope: {count} path(s)/pattern(s)",
        "fr": "Portée : {count} chemin(s)/motif(s)",
        "it": "Ambito: {count} percorso/i/pattern",
    },
    "Target: {target}": {
        "de": "Ziel: {target}",
        "en": "Target: {target}",
        "fr": "Cible : {target}",
        "it": "Destinazione: {target}",
    },
    "yes": {"de": "ja", "en": "yes", "fr": "oui", "it": "sì"},
    "no (abort on conflicts)": {
        "de": "nein (Abbruch bei Konflikten)",
        "en": "no (abort on conflicts)",
        "fr": "non (arrêt en cas de conflit)",
        "it": "no (interrompi in caso di conflitto)",
    },
    "Overwrite: {value}": {
        "de": "Überschreiben: {value}",
        "en": "Overwrite: {value}",
        "fr": "Écraser : {value}",
        "it": "Sovrascrivi: {value}",
    },
    "Start restore now?": {
        "de": "Wiederherstellung jetzt starten?",
        "en": "Start restore now?",
        "fr": "Démarrer la restauration maintenant ?",
        "it": "Avviare il ripristino ora?",
    },
    "← Exit": {"de": "← Beenden", "en": "← Exit", "fr": "← Quitter", "it": "← Esci"},
    "Contents of {archive}": {
        "de": "Inhalt von {archive}",
        "en": "Contents of {archive}",
        "fr": "Contenu de {archive}",
        "it": "Contenuto di {archive}",
    },
    "Contents of /{path}": {
        "de": "Inhalt von /{path}",
        "en": "Contents of /{path}",
        "fr": "Contenu de /{path}",
        "it": "Contenuto di /{path}",
    },
}


def apply_lang(lang: str) -> int:
    path = ROOT / f"{lang}.po"
    data = path.read_text(encoding="utf-8")
    filled = 0
    for msgid, langs in CLI.items():
        if lang not in langs:
            continue
        text = langs[lang]
        if "\n" in msgid:
            continue
        old = f'msgid "{msgid}"\nmsgstr ""'
        new = f'msgid "{msgid}"\nmsgstr "{text}"'
        if old in data:
            data = data.replace(old, new, 1)
            filled += 1
    multi_id = (
        "No archives found — check profile paths in the app or load the PBS manifest."
    )
    if multi_id in CLI:
        mstr = CLI[multi_id].get(lang, "")
        old = (
            'msgid ""\n'
            '"No archives found — check profile paths in the app or load the PBS manifest."\n'
            'msgstr ""'
        )
        new = (
            'msgid ""\n'
            '"No archives found — check profile paths in the app or load the PBS manifest."\n'
            f'msgstr ""\n"{mstr}"'
        )
        if old in data:
            data = data.replace(old, new, 1)
            filled += 1
    path.write_text(data, encoding="utf-8")
    return filled


def main() -> int:
    total = 0
    for lang in ("de", "en", "fr", "it"):
        n = apply_lang(lang)
        print(f"{lang}.po: {n} CLI strings filled")
        total += n
    print(f"total: {total}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
