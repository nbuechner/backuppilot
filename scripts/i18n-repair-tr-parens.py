#!/usr/bin/env python3
"""Repair tr()/tr_fmt() parentheses broken by a bad empty-string replace."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MSG = r'([^"\\]*(?:\\.[^"\\]*)*)'


def repair_file(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    original = text

    # &tr("msg"))); -> &tr("msg"));
    text = re.sub(rf'&tr\("{MSG}"\)\)\)\);', r'&tr("\1"));', text)
    text = re.sub(rf'&tr_fmt\("{MSG}"\)\)\)\);', r'&tr_fmt("\1"));', text)

    # => tr("msg",  -> => tr("msg"),
    text = re.sub(rf'(=>\s*)tr\("{MSG}",(\s*$)', r'\1tr("\2"),\3', text, flags=re.M)

    # => tr("msg"), at EOL (missing ')')
    text = re.sub(rf'(=>\s*)tr\("{MSG}"\),(\s*$)', r'\1tr("\2")),\3', text, flags=re.M)
    text = re.sub(rf'(=>\s*)tr_fmt\("{MSG}"\),(\s*$)', r'\1tr_fmt("\2")),\3', text, flags=re.M)

    # Some(tr("msg"), / &tr("msg"),
    text = re.sub(rf'Some\(tr\("{MSG}"\),', r'Some(tr("\1")),', text)
    text = re.sub(rf'Some\(&tr\("{MSG}"\),', r'Some(&tr("\1")),', text)
    text = re.sub(rf'Some\(&tr_fmt\("{MSG}"\),', r'Some(&tr_fmt("\1")),', text)

    # (key, tr("msg"), tuple elements
    text = re.sub(rf'\(\s*([^,]+),\s*tr\("{MSG}"\),', r'(\1, tr("\2")),', text)

    # &tr("msg"), at EOL in arg lists
    text = re.sub(rf'&tr\("{MSG}"\),(\s*$)', r'&tr("\1")),\2', text, flags=re.M)

    # gtk::Label::new(Some(&tr("msg"))); -> ...")));
    text = re.sub(rf'Some\(&tr\("{MSG}"\)\)\);', r'Some(&tr("\1")));', text)

    # push_str("&amp;"", etc.
    text = re.sub(r'push_str\("(&amp;|&lt;|&gt;)";', r'push_str("\1");', text)

    # join(", "")) -> join(", ")
    text = text.replace('.join(", ""))', '.join(", ")')

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
