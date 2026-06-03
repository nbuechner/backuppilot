#!/usr/bin/env bash
# Regeneriert hicolor-Icons und Branding aus Marketing/Icon-Dark.png (Dock, Fenster, Tray).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC="$ROOT/Marketing/Icon-Dark.png"
OUT="$ROOT/App/data/icons/hicolor"
NAME="${BACKUPPILOT_APP_ID:-ch.onesystems.backuppilot}.png"
BRAND="$ROOT/App/data/branding"

if [[ ! -f "$SRC" ]]; then
  echo "Fehler: App-Icon fehlt: $SRC" >&2
  echo "Bitte Marketing/Icon-Dark.png bereitstellen (wird beim Build zwingend verwendet)." >&2
  exit 1
fi

python3 << PY
from pathlib import Path
from PIL import Image

src = Path("$SRC")
out_base = Path("$OUT")
name = "$NAME"
img = Image.open(src).convert("RGBA")
sizes = (16, 32, 48, 64, 128, 256, 512)

# Alte Icon-Dateinamen entfernen (z. B. nach App-ID-Umbenennung)
for old in out_base.rglob("*.png"):
    if old.name != name:
        old.unlink()
        print(f"→ entfernt: {old}")

for size in sizes:
    d = out_base / f"{size}x{size}" / "apps"
    d.mkdir(parents=True, exist_ok=True)
    img.resize((size, size), Image.Resampling.LANCZOS).save(d / name, optimize=True)
    print(f"→ {d / name}")

# index.theme nur für ~/.local (install-local); System-RPM/DEB nutzen hicolor-icon-theme.
index = out_base / "index.theme"
dirs = "\n".join(f"{s}x{s}/apps" for s in sizes)
index.write_text(
    f"""[Icon Theme]
Name=BackupPilot
Comment=BackupPilot application icons
Directories={",".join(f"{s}x{s}/apps" for s in sizes)}

"""
    + "\n".join(
        f"""[{s}x{s}/apps]
Size={s}
Type=Fixed
"""
        for s in sizes
    )
    + "\n",
    encoding="utf-8",
)
print(f"→ {index}")
PY

mkdir -p "$BRAND"
cp -f "$SRC" "$BRAND/icon.png"
for pair in "Logo.png:logo.png" "Logo-Dark.png:logo-dark.png"; do
  src_file="${pair%%:*}"
  dst_file="${pair##*:}"
  if [[ -f "$ROOT/Marketing/$src_file" ]]; then
    cp -f "$ROOT/Marketing/$src_file" "$BRAND/$dst_file"
  fi
done
echo "→ $BRAND/ (icon.png aus Icon-Dark.png)"
