#!/usr/bin/env bash
# Compile App/po/*.po → App/locale/<lang>/LC_MESSAGES/backuppilot.mo
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PO_DIR="$ROOT/po"
LOCALE_DIR="$ROOT/locale"

if ! command -v msgfmt >/dev/null 2>&1; then
  echo "msgfmt not found. Install gettext: sudo apt install gettext" >&2
  exit 1
fi

mkdir -p "$LOCALE_DIR"

while IFS= read -r lang || [[ -n "$lang" ]]; do
  [[ -z "$lang" || "$lang" =~ ^# ]] && continue
  po="$PO_DIR/$lang.po"
  [[ -f "$po" ]] || { echo "missing $po" >&2; exit 1; }
  out_dir="$LOCALE_DIR/$lang/LC_MESSAGES"
  mkdir -p "$out_dir"
  msgfmt -o "$out_dir/backuppilot.mo" "$po"
  echo "→ $out_dir/backuppilot.mo"
done < "$PO_DIR/LINGUAS"
