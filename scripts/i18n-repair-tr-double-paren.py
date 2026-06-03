#!/usr/bin/env python3
"""Remove extra ')' wrongly added after tr() in match arms and similar."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MSG = r'([^"\\]*(?:\\.[^"\\]*)*)'


def repair_file(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    original = text

    text = re.sub(rf'(=>\s*)tr\("{MSG}"\)\),', r'\1tr("\2"),', text)
    text = re.sub(rf'(\s+"[^"]+"\s*=>\s*)tr\("{MSG}"\)\),', r'\1tr("\2"),', text)

    text = re.sub(
        rf'(&tr\("{MSG}"\)\)),(\s*\n\s*\);)',
        r'&tr("\2"),\3',
        text,
    )

    text = re.sub(rf'\btr\("{MSG}"\)\)\);', r'tr("\1");', text)
    text = re.sub(rf'&tr\("{MSG}"\)\)\);', r'&tr("\1");', text)

    text = re.sub(
        rf'Some\(tr\("{MSG}"\)\)\);',
        r'Some(tr("\1"));',
        text,
    )

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
