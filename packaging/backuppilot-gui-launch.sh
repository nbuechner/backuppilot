#!/usr/bin/env bash
# Launch wrapper for the BackupPilot GUI when started from a systemd user unit.
# The unit sets BACKUPPILOT_GUI to the absolute path of the GUI binary and
# passes the graphical session environment via PassEnvironment=.
set -euo pipefail

if [[ -z "${BACKUPPILOT_GUI:-}" ]]; then
    echo "backuppilot-gui-launch: BACKUPPILOT_GUI is not set" >&2
    exit 1
fi

if [[ ! -x "$BACKUPPILOT_GUI" ]]; then
    echo "backuppilot-gui-launch: $BACKUPPILOT_GUI is not executable" >&2
    exit 1
fi

exec "$BACKUPPILOT_GUI" "$@"
