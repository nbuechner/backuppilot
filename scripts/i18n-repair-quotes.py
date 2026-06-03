#!/usr/bin/env python3
"""Repair tr("foo"") and tr("foo"); damage from a broken i18n-fix-source run."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def repair_file(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    original = text

    # Only fix tr("msg"") double-quote typos (do not collapse "" empty strings).
    text = re.sub(r'\btr\("([^"\\]*(?:\\.[^"\\]*)*)""\)', r'tr("\1")', text)
    text = re.sub(r'\btr_fmt\("([^"\\]*(?:\\.[^"\\]*)*)""\)', r'tr_fmt("\1")', text)

    # tr("msg"); -> tr("msg"));
    text = re.sub(r'\btr\("([^"\\]*(?:\\.[^"\\]*)*)"\)\s*;', r'tr("\1"));', text)
    text = re.sub(r'\btr_fmt\("([^"\\]*(?:\\.[^"\\]*)*)"\)\s*;', r'tr_fmt("\1"));', text)

    # &tr("msg"); missing closing paren for tr()
    text = re.sub(
        r'&tr\("([^"\\]*(?:\\.[^"\\]*)*)"\)\s*;',
        r'&tr("\1"));',
        text,
    )
    text = re.sub(
        r'&tr_fmt\("([^"\\]*(?:\\.[^"\\]*)*)"\)\s*;',
        r'&tr_fmt("\1"));',
        text,
    )

    # tr("msg") at EOL without closing paren: tr("msg");
    text = re.sub(
        r'\btr\("([^"\\]*(?:\\.[^"\\]*)*)"\)\s*\n',
        r'tr("\1"))\n',
        text,
    )

    # push_str("\n"" -> push_str("\n");
    text = text.replace('push_str("\\n""', 'push_str("\\n");')
    text = text.replace('push_str("\\n…""', 'push_str("\\n…");')

    # push_str("&amp;"", without closing
    text = re.sub(
        r'push_str\("(&amp;|&lt;|&gt;)""',
        r'push_str("\1");',
        text,
    )

    if text == original:
        return False
    path.write_text(text, encoding="utf-8")
    return True


def main() -> int:
    n = sum(
        repair_file(path)
        for path in sorted(ROOT.rglob("*.rs"))
        if "target" not in path.parts
    )
    print(f"Repaired {n} file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
