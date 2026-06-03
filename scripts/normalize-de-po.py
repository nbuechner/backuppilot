#!/usr/bin/env python3
"""Normalize German msgstr: ss instead of ß, no em/en dashes, fix ae/oe/ue to umlauts where obvious."""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PO = ROOT / "po" / "de.po"

UMLAUT_FIXES = {
    "Oeffnen": "Öffnen",
    "oeffnen": "öffnen",
    "Ausserhalb": "Ausserhalb",  # Swiss style: often "ausserhalb" is intentional; keep or use "außerhalb"
    "ausserhalb": "ausserhalb",
    "Schliessen": "Schliessen",
    "schliessen": "schliessen",
    "Zurueck": "Zurück",
    "zurueck": "zurück",
    "fuer": "für",
    "Fuer": "Für",
    "muessen": "müssen",
    "Muessen": "Müssen",
    "naechste": "nächste",
    "Naechste": "Nächste",
}


def normalize_msgstr(text: str) -> str:
    if not text:
        return text
    text = text.replace("ß", "ss").replace("ẞ", "SS")
    text = text.replace("—", ", ").replace("–", "-")
    # collapse duplicate spaces after replacements
    text = re.sub(r" , ", ", ", text)
    text = re.sub(r", ,", ",", text)
    for old, new in UMLAUT_FIXES.items():
        text = text.replace(old, new)
    return text


def process_po(path: Path) -> int:
    content = path.read_text(encoding="utf-8")
    changed = 0

    def repl_msgstr_line(m: re.Match[str]) -> str:
        nonlocal changed
        inner = m.group(1)
        new = normalize_msgstr(inner)
        if new != inner:
            changed += 1
        escaped = new.replace("\\", "\\\\").replace('"', '\\"')
        return f'msgstr "{escaped}"'

    # single-line msgstr
    out = re.sub(r'^msgstr "(.*)"$', repl_msgstr_line, content, flags=re.M)

    # multiline continuation lines inside msgstr
    def repl_cont(m: re.Match[str]) -> str:
        nonlocal changed
        inner = m.group(1)
        new = normalize_msgstr(inner)
        if new != inner:
            changed += 1
        escaped = new.replace("\\", "\\\\").replace('"', '\\"')
        suffix = m.group(2) or ""
        return f'"{escaped}{suffix}"'

    out = re.sub(r'^"(.*)"(\\n)?"$', repl_cont, out, flags=re.M)
    path.write_text(out, encoding="utf-8")
    return changed


def main() -> int:
    n = process_po(PO)
    print(f"Normalized {n} msgstr fragment(s) in {PO}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
