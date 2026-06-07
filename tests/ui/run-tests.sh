#!/usr/bin/env bash
# Run WebdriverIO UI tests against the Tauri app via tauri-driver on Windows.
# tauri-driver launches the app itself — no need to pre-start it.
#
# Usage:
#   ./run-tests.sh                         # all specs
#   ./run-tests.sh --spec specs/profiles   # single spec

set -e

WINDOWS_HOST="user@10.99.158.244"
DRIVER_PORT=4444

cleanup() {
    echo "==> Cleaning up..."
    kill "$TUNNEL_PID" 2>/dev/null || true
    ssh "$WINDOWS_HOST" "powershell -Command \"Stop-Process -Name tauri-driver -Force -ErrorAction SilentlyContinue; Stop-Process -Name backuppilot-tauri -Force -ErrorAction SilentlyContinue\"" 2>/dev/null || true
}
trap cleanup EXIT

echo "==> Killing any existing tauri-driver / app on Windows..."
ssh "$WINDOWS_HOST" "powershell -Command \"Stop-Process -Name tauri-driver -Force -EA SilentlyContinue; Stop-Process -Name backuppilot-tauri -Force -EA SilentlyContinue\"" 2>/dev/null || true

echo "==> Starting tauri-driver on Windows (port $DRIVER_PORT)..."
ssh "$WINDOWS_HOST" "powershell -Command \"Start-Process tauri-driver -ArgumentList '--port $DRIVER_PORT' -WindowStyle Hidden\"" &
sleep 2

echo "==> Opening SSH tunnel localhost:$DRIVER_PORT -> Windows:$DRIVER_PORT..."
ssh -N -L "${DRIVER_PORT}:localhost:${DRIVER_PORT}" "$WINDOWS_HOST" &
TUNNEL_PID=$!
sleep 1

echo "==> Verifying tunnel (tauri-driver status)..."
if ! curl -sf "http://localhost:$DRIVER_PORT/status" > /dev/null 2>&1; then
    echo "WARN: tauri-driver not yet responding, waiting a bit more..."
    sleep 3
    if ! curl -sf "http://localhost:$DRIVER_PORT/status" > /dev/null 2>&1; then
        echo "ERROR: tauri-driver not reachable at localhost:$DRIVER_PORT"
        exit 1
    fi
fi
echo "==> tauri-driver is up."

echo "==> Running WebdriverIO tests..."
cd "$(dirname "$0")"
npm install --silent
npx wdio run wdio.conf.ts "$@"
