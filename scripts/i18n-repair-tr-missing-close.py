#!/usr/bin/env python3
"""Close tr() calls missing ')' before ; } or end of line."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MSG = r'([^"\\]*(?:\\.[^"\\]*)*)'


def repair_file(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    original = text

    text = re.sub(rf'\btr\("{MSG}"\s*;', r'tr("\1");', text)
    text = re.sub(rf'\btr\("{MSG}"\s*([}}])', r'tr("\1")\2', text)
    text = re.sub(rf'\btr\("{MSG}"\s*$', r'tr("\1")', text, flags=re.M)

    text = text.replace('push_str("\\n\\n";', 'push_str("\\n\\n");')
    text = text.replace('push_str("\\n…";', 'push_str("\\n…");')

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
