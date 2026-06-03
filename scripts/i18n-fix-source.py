#!/usr/bin/env python3
"""Replace em/en dashes in user-facing tr()/tr_fmt() string literals (UTF-8)."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

REPLACEMENTS = [
    (" — ", ", "),
    (" —", ","),
    ("— ", ""),
    ("—", ","),
    (" – ", "-"),
    ("–", "-"),
    (" − ", "-"),
    ("−", "-"),
]


def fix_string_content(s: str) -> str:
    for old, new in REPLACEMENTS:
        s = s.replace(old, new)
    s = re.sub(r"\s*,\s*", ", ", s)
    s = re.sub(r",\s*,", ",", s)
    s = re.sub(r" \.", ".", s)
    s = re.sub(r"\s{2,}", " ", s)
    return s.strip()


def fix_file(path: Path) -> int:
    text = path.read_text(encoding="utf-8")
    original = text

    def repl_double(m: re.Match[str]) -> str:
        open_q, body, close_q = m.group(1), m.group(2), m.group(3)
        return f"tr({open_q}{fix_string_content(body)}{close_q}"

    text = re.sub(
        r'tr\(\s*(")([^"\\]*(?:\\.[^"\\]*)*)(")\s*\)',
        repl_double,
        text,
    )

    def repl_fmt(m: re.Match[str]) -> str:
        open_q, body, close_q = m.group(1), m.group(2), m.group(3)
        return f"tr_fmt({open_q}{fix_string_content(body)}{close_q}"

    text = re.sub(
        r'tr_fmt\(\s*(")([^"\\]*(?:\\.[^"\\]*)*)(")\s*,',
        repl_fmt,
        text,
    )

    if text != original:
        path.write_text(text, encoding="utf-8")
        return 1
    return 0


def main() -> int:
    changed = 0
    for path in sorted(ROOT.rglob("*.rs")):
        if "target" in path.parts:
            continue
        changed += fix_file(path)
    print(f"Updated {changed} Rust source file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
