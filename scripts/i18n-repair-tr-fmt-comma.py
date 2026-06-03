#!/usr/bin/env python3
"""Fix tr_fmt(\"msg\" &[) -> tr_fmt(\"msg\", &[) and close dangling &tr(\"msg\", lines."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MSG = r'([^"\\]*(?:\\.[^"\\]*)*)'


def repair_file(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    original = text

    text = re.sub(rf'tr_fmt\("{MSG}"\s*&\[', r'tr_fmt("\1", &[', text)

    # &tr("msg", at EOL -> &tr("msg")),
    text = re.sub(rf'&tr\("{MSG}",\s*$', r'&tr("\1")),', text, flags=re.M)

    # tr("msg", at EOL in match/return (not &tr)
    text = re.sub(rf'(?<!&)tr\("{MSG}",\s*$', r'tr("\1"),', text, flags=re.M)

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
