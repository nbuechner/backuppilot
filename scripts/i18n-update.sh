#!/usr/bin/env bash
# Refresh the template (backuppilot.pot) and merge into language files.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PO_DIR="$ROOT/po"
POT="$PO_DIR/backuppilot.pot"

if ! command -v xgettext >/dev/null 2>&1; then
  echo "xgettext not found. Install gettext: sudo apt install gettext" >&2
  exit 1
fi

mapfile -t sources < <(grep -v '^#' "$PO_DIR/POTFILES.in" | grep -v '^[[:space:]]*$' || true)

xgettext \
  --from-code=UTF-8 \
  --language=Rust \
  --keyword=tr \
  --keyword=tr_fmt:1 \
  --keyword=gettext \
  --output="$POT" \
  "${sources[@]/#/$ROOT/}" 2>/dev/null || xgettext \
  --from-code=UTF-8 \
  --keyword=tr \
  --keyword=tr_fmt:1 \
  --keyword=gettext \
  --output="$POT" \
  "${sources[@]/#/$ROOT/}"

while IFS= read -r lang || [[ -n "$lang" ]]; do
  [[ -z "$lang" || "$lang" =~ ^# ]] && continue
  po="$PO_DIR/$lang.po"
  if [[ -f "$po" ]]; then
    msgmerge --update --no-fuzzy-matching "$po" "$POT"
    echo "merged → $po"
  else
    msginit --no-translator --locale="$lang" --input="$POT" --output="$po"
    echo "created → $po"
  fi
done < "$PO_DIR/LINGUAS"

if [[ -x "$ROOT/scripts/fill-translations.py" ]]; then
  "$ROOT/scripts/fill-translations.py"
fi

I18N_PY=python3
if [[ -x "$ROOT/.venv-i18n/bin/python" ]]; then
  I18N_PY="$ROOT/.venv-i18n/bin/python"
fi

if [[ -x "$ROOT/scripts/sync-all-po.py" ]]; then
  if "$I18N_PY" -c "import polib" 2>/dev/null; then
    "$I18N_PY" "$ROOT/scripts/sync-all-po.py" || echo "warn: sync-all-po.py failed (see App/.venv-i18n or pip install polib deep-translator)" >&2
  fi
fi

echo "Run scripts/i18n-compile.sh to build .mo files."
