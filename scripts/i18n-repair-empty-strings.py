#!/usr/bin/env python3
"""Restore empty string literals broken by replacing \"\" with \"."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

REPLACEMENTS = [
    (r'setlocale\(LocaleCategory::LcAll, "\)', r'setlocale(LocaleCategory::LcAll, "")'),
    (r'\.unwrap_or\("\)', r'.unwrap_or("")'),
    (r'\.as_deref\(\)\.unwrap_or\("\)', r'.as_deref().unwrap_or("")'),
    (r'error\.unwrap_or\("\)', r'error.unwrap_or("")'),
]


def repair_file(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    original = text
    for pattern, repl in REPLACEMENTS:
        text = re.sub(pattern, repl, text)
    if text == original:
        return False
    path.write_text(text, encoding="utf-8")
    return True


def main() -> int:
    n = 0
    for path in sorted(ROOT.rglob("*.rs")):
        if "target" in path.parts:
            continue
        if repair_file(path):
            n += 1
            print(path.relative_to(ROOT))
    print(f"Repaired {n} file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
