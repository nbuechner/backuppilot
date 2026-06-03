#!/usr/bin/env python3
"""Fill fr/it (and missing de) from English msgids, then normalize all .po files."""
from __future__ import annotations

import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PO_DIR = ROOT / "po"
POT = PO_DIR / "backuppilot.pot"
LANGS = ["de", "en", "fr", "it"]

try:
    import polib
except ImportError:
    print("Install polib: pip install polib", file=sys.stderr)
    sys.exit(1)

try:
    from deep_translator import GoogleTranslator
except ImportError:
    GoogleTranslator = None  # type: ignore[misc, assignment]


def run_normalize() -> None:
    script = ROOT / "scripts" / "normalize-po.py"
    subprocess.run([sys.executable, str(script)], check=True)


def load_po(lang: str) -> polib.POFile:
    path = PO_DIR / f"{lang}.po"
    if path.is_file():
        return polib.pofile(str(path))
    po = polib.POFile(str(path))
    po.metadata = {
        "Project-Id-Version": "BackupPilot",
        "MIME-Version": "1.0",
        "Content-Type": "text/plain; charset=UTF-8",
        "Content-Transfer-Encoding": "8bit",
        "Language": lang,
    }
    return po


def merge_from_pot(po: polib.POFile, pot: polib.POFile) -> None:
    existing = {e.msgid: e for e in po if e.msgid}
    for pe in pot:
        if not pe.msgid or pe.msgid in existing:
            continue
        entry = polib.POEntry(msgid=pe.msgid, msgstr="")
        if pe.msgid_plural:
            entry.msgid_plural = pe.msgid_plural
            entry.msgstr_plural = {0: "", 1: ""}
        po.append(entry)
        existing[pe.msgid] = entry


def needs_translation(entry: polib.POEntry) -> bool:
    if entry.obsolete:
        return False
    if entry.msgid_plural:
        return not any(entry.msgstr_plural.values())
    return not (entry.msgstr or "").strip()


def translate_text(text: str, dest: str) -> str:
    if not text.strip():
        return text
    if GoogleTranslator is None:
        return text
  # Preserve placeholders and quotes for PBS patterns
    protected: list[str] = []

    def shield(m: re.Match[str]) -> str:
        protected.append(m.group(0))
        return f"__PH{len(protected) - 1}__"

    work = text
    for pat in (
        r"\{[a-zA-Z0-9_]+\}",
        r"«[^»]+»",
        r"“[^”]+”",
        r"`[^`]+`",
        r"\*\*[^*]+\*\*",
    ):
        work = re.sub(pat, shield, work)

    try:
        out = GoogleTranslator(source="en", target=dest).translate(work)
    except Exception as err:
        print(f"  translate warning ({dest}): {err}", file=sys.stderr)
        return text

    if not out:
        return text

    for i, val in enumerate(protected):
        out = out.replace(f"__PH{i}__", val)
    return out


def fill_language(po: polib.POFile, lang: str, *, from_en: bool) -> int:
    filled = 0
    dest = {"de": "de", "fr": "fr", "it": "it"}.get(lang)
    for entry in po:
        if not needs_translation(entry):
            continue
        if lang == "en":
            if entry.msgid_plural:
                entry.msgstr_plural = {0: entry.msgid, 1: entry.msgid_plural}
            else:
                entry.msgstr = entry.msgid
            filled += 1
            continue
        if not dest:
            continue
        if entry.msgid_plural:
            s0 = translate_text(entry.msgid, dest) if from_en or lang != "de" else entry.msgid
            s1 = (
                translate_text(entry.msgid_plural, dest)
                if from_en or lang != "de"
                else entry.msgid_plural
            )
            entry.msgstr_plural = {0: s0, 1: s1}
        else:
            source = entry.msgid
            entry.msgstr = (
                translate_text(source, dest)
                if (from_en or not (entry.msgstr or "").strip())
                else entry.msgstr
            )
        filled += 1
        if filled % 25 == 0:
            time.sleep(0.3)
    return filled


def main() -> int:
    if not POT.is_file():
        print("Run scripts/i18n-update.sh first (missing backuppilot.pot)", file=sys.stderr)
        return 1

    pot = polib.pofile(str(POT))
    if GoogleTranslator is None:
        print("deep-translator not installed; only merging and English copy.", file=sys.stderr)

    for lang in LANGS:
        po = load_po(lang)
        merge_from_pot(po, pot)
        if lang == "en":
            n = fill_language(po, lang, from_en=True)
        elif lang == "de":
            n = fill_language(po, lang, from_en=GoogleTranslator is not None)
        else:
            n = fill_language(po, lang, from_en=True)
        po.save(str(PO_DIR / f"{lang}.po"))
        print(f"{lang}.po: filled/updated {n} entries")

    run_normalize()
    print("Normalized all languages (no em dashes; German ss not ß).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
