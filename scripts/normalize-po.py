#!/usr/bin/env python3
"""Normalize .po msgstr: no em/en dashes; German uses ss instead of ß and proper umlauts."""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PO_DIR = ROOT / "po"

try:
    import polib
except ImportError:
    polib = None  # type: ignore[assignment]

# Prefer umlauts over ae/oe/ue digraphs in German UI text (never ß).
DE_UMLAUT_FIXES = {
    "Oeffnen": "Öffnen",
    "oeffnen": "öffnen",
    "Zurueck": "Zurück",
    "zurueck": "zurück",
    "fuer": "für",
    "Fuer": "Für",
    "muessen": "müssen",
    "Muessen": "Müssen",
    "naechste": "nächste",
    "Naechste": "Nächste",
    "waehlen": "wählen",
    "Waehlen": "Wählen",
    "Pruefung": "Prüfung",
    "pruefung": "prüfung",
    "Pruefen": "Prüfen",
    "pruefen": "prüfen",
    "ueber": "über",
    "Ueber": "Über",
    "groesser": "grösser",
    "Groesser": "Grösser",
    "grossen": "groessen",
    "Grossen": "Groessen",
    "grosse": "groesse",
    "Grosse": "Groesse",
}


def strip_dashes(text: str) -> str:
    if not text:
        return text
    text = text.replace("\u2014", ", ").replace("\u2013", "-")
    text = re.sub(r" +, ", ", ", text)
    text = re.sub(r", ,", ",", text)
    return text


def normalize_german(text: str) -> str:
    text = strip_dashes(text)
    text = text.replace("ß", "ss").replace("ẞ", "SS")
    for old, new in DE_UMLAUT_FIXES.items():
        text = text.replace(old, new)
    return text


def normalize_other(text: str) -> str:
    return strip_dashes(text)


def normalize_entry_strings(
    strings: dict[int, str], normalizer
) -> tuple[dict[int, str], int]:
    changed = 0
    out: dict[int, str] = {}
    for key, value in strings.items():
        new = normalizer(value)
        if new != value:
            changed += 1
        out[key] = new
    return out, changed


def process_with_polib(path: Path, lang: str) -> int:
    normalizer = normalize_german if lang == "de" else normalize_other
    po = polib.pofile(str(path))
    changed = 0
    for entry in po:
        if entry.obsolete:
            continue
        if entry.msgid_plural:
            new_plural, n = normalize_entry_strings(entry.msgstr_plural, normalizer)
            if n:
                entry.msgstr_plural = new_plural
                changed += n
        elif entry.msgstr:
            new = normalizer(entry.msgstr)
            if new != entry.msgstr:
                entry.msgstr = new
                changed += 1
    po.save(str(path))
    return changed


def process_po_regex(path: Path, lang: str) -> int:
    """Fallback when polib is not installed."""
    content = path.read_text(encoding="utf-8")
    changed = 0
    normalizer = normalize_german if lang == "de" else normalize_other
    in_msgstr = False
    lines: list[str] = []

    for line in content.splitlines(keepends=True):
        if line.startswith("msgid "):
            in_msgstr = False
            lines.append(line)
            continue
        if line.startswith("msgid_plural "):
            in_msgstr = False
            lines.append(line)
            continue
        if line.startswith("msgstr"):
            in_msgstr = True
            m = re.match(r'^msgstr(?:\[\d+\])? "(.*)"$', line.rstrip("\n"))
            if m:
                inner = m.group(1)
                new = normalizer(inner)
                if new != inner:
                    changed += 1
                esc = new.replace("\\", "\\\\").replace('"', '\\"')
                prefix = line.split('"', 1)[0]
                lines.append(f'{prefix}"{esc}"\n')
            else:
                lines.append(line)
            continue
        if in_msgstr and line.startswith('"'):
            m = re.match(r'^"(.*)"(\\n)?$', line.rstrip("\n"))
            if m:
                inner = m.group(1)
                suffix = m.group(2) or ""
                new = normalizer(inner)
                if new != inner:
                    changed += 1
                esc = new.replace("\\", "\\\\").replace('"', '\\"')
                lines.append(f'"{esc}"{suffix}\n')
            else:
                lines.append(line)
            continue
        if line.strip() == "" or line.startswith("#"):
            in_msgstr = False
        lines.append(line)

    path.write_text("".join(lines), encoding="utf-8")
    return changed


def process_po(path: Path, lang: str) -> int:
    if polib is not None:
        return process_with_polib(path, lang)
    return process_po_regex(path, lang)


def main() -> int:
    for lang in ["de", "en", "fr", "it"]:
        po = PO_DIR / f"{lang}.po"
        if not po.is_file():
            print(f"skip missing {po}")
            continue
        n = process_po(po, lang)
        print(f"{po.name}: normalized {n} string(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
