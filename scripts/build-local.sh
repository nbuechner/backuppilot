#!/usr/bin/env bash
# Build BackupPilot Linux .deb packages locally using Docker.
# Cargo and npm caches are kept in named Docker volumes so rebuilds are fast.
# Usage: ./scripts/build-local.sh [ubuntu2404|ubuntu2604|all]
set -euo pipefail

TARGET="${1:-all}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$REPO/dist"
mkdir -p "$DIST"

VERSION=$(grep '^version' "$REPO/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
echo "BackupPilot v${VERSION}"

# ── ubuntu-24.04: Tauri / WebKitGTK ─────────────────────────────────────────
build_2404() {
  echo ""
  echo "=== ubuntu-24.04 (Tauri / WebKitGTK) ==="

  # Inner script written to a variable to avoid quoting nightmares
  read -r -d '' SCRIPT << 'EOF' || true
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -y -q --no-install-recommends \
  curl ca-certificates build-essential \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libdbus-1-dev pkg-config libssl-dev

# Rust (cached in volume after first run)
if [ ! -f /root/.cargo/bin/rustup ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --no-modify-path
fi
export PATH="/root/.cargo/bin:$PATH"

# Node 22 (cached in volume after first run)
if ! command -v node &>/dev/null; then
  curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
  apt-get install -y nodejs
fi

cd /src

cd crates/backuppilot-tauri/ui
npm ci --prefer-offline
npm run build
cd /src

cargo build -p backuppilot-daemon -p backuppilot-cli -p backuppilot-tauri --release

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*= *"\(.*\)"/\1/')
PKG="/build/backuppilot_${VERSION}_ubuntu2404_amd64"
rm -rf "$PKG"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/usr/lib/systemd/user" \
         "$PKG/usr/share/applications" \
         "$PKG/usr/share/icons/hicolor/256x256/apps" \
         "$PKG/usr/share/icons/hicolor/512x512/apps"

T="/build/target/release"
cp "$T/backuppilot-tauri"  "$PKG/usr/bin/backuppilot"
cp "$T/backuppilot-daemon" "$PKG/usr/bin/"
cp "$T/backuppilot-cli"    "$PKG/usr/bin/"
cp data/backuppilot-daemon.service               "$PKG/usr/lib/systemd/user/"
cp data/ch.onesystems.backuppilot.desktop        "$PKG/usr/share/applications/"
cp data/icons/hicolor/256x256/apps/ch.onesystems.backuppilot.png \
   "$PKG/usr/share/icons/hicolor/256x256/apps/"
cp data/icons/hicolor/512x512/apps/ch.onesystems.backuppilot.png \
   "$PKG/usr/share/icons/hicolor/512x512/apps/"

printf 'Package: backuppilot\nVersion: %s\nArchitecture: amd64\nMaintainer: BackupPilot Contributors <noreply@buechner.me>\nDepends: proxmox-backup-client, fuse3, libwebkit2gtk-4.1-0\nDescription: BackupPilot - Proxmox Backup Server GUI client\n Provides scheduled backups, restore, snapshot browsing, and\n read-only FUSE mounts of PBS archives.\n' "$VERSION" > "$PKG/DEBIAN/control"
printf '#!/bin/sh\nset -e\nsystemctl --user daemon-reload 2>/dev/null || true\nsystemctl --user enable backuppilot-daemon.service 2>/dev/null || true\nsystemctl --user start backuppilot-daemon.service 2>/dev/null || true\n' > "$PKG/DEBIAN/postinst"
chmod 755 "$PKG/DEBIAN/postinst"
printf '#!/bin/sh\nset -e\nsystemctl --user stop backuppilot-daemon.service 2>/dev/null || true\nsystemctl --user disable backuppilot-daemon.service 2>/dev/null || true\n' > "$PKG/DEBIAN/prerm"
chmod 755 "$PKG/DEBIAN/prerm"

dpkg-deb --build "$PKG"
cp "${PKG}.deb" /dist/
echo "Done: backuppilot_${VERSION}_ubuntu2404_amd64.deb"
EOF

  docker run --rm \
    -v "$REPO:/src" \
    -v "backuppilot-target-2404:/build/target" \
    -v "backuppilot-cargo-2404:/root/.cargo" \
    -v "backuppilot-node-2404:/src/crates/backuppilot-tauri/ui/node_modules" \
    -v "$DIST:/dist" \
    -e CARGO_TARGET_DIR=/build/target \
    ubuntu:24.04 \
    bash -c "$SCRIPT"
}

# ── ubuntu-26.04: GTK native ─────────────────────────────────────────────────
build_2604() {
  echo ""
  echo "=== ubuntu-26.04 (GTK native) ==="

  read -r -d '' SCRIPT << 'EOF' || true
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -y -q --no-install-recommends \
  curl ca-certificates build-essential \
  libgtk-4-dev libadwaita-1-dev libdbus-1-dev pkg-config libssl-dev

if [ ! -f /root/.cargo/bin/rustup ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --no-modify-path
fi
export PATH="/root/.cargo/bin:$PATH"

cd /src
cargo build -p backuppilot-daemon -p backuppilot-gui -p backuppilot-cli --release

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*= *"\(.*\)"/\1/')
PKG="/build/backuppilot_${VERSION}_ubuntu2604_amd64"
rm -rf "$PKG"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/usr/lib/systemd/user" \
         "$PKG/usr/share/applications" \
         "$PKG/usr/share/icons/hicolor/256x256/apps" \
         "$PKG/usr/share/icons/hicolor/512x512/apps"

T="/build/target/release"
cp "$T/backuppilot"        "$PKG/usr/bin/"
cp "$T/backuppilot-daemon" "$PKG/usr/bin/"
cp "$T/backuppilot-cli"    "$PKG/usr/bin/"
cp data/backuppilot-daemon.service               "$PKG/usr/lib/systemd/user/"
cp data/ch.onesystems.backuppilot.desktop        "$PKG/usr/share/applications/"
cp data/icons/hicolor/256x256/apps/ch.onesystems.backuppilot.png \
   "$PKG/usr/share/icons/hicolor/256x256/apps/"
cp data/icons/hicolor/512x512/apps/ch.onesystems.backuppilot.png \
   "$PKG/usr/share/icons/hicolor/512x512/apps/"

printf 'Package: backuppilot\nVersion: %s\nArchitecture: amd64\nMaintainer: BackupPilot Contributors <noreply@buechner.me>\nDepends: proxmox-backup-client, fuse3, libadwaita-1-0 (>= 1.6)\nDescription: BackupPilot - Proxmox Backup Server GUI client\n Provides scheduled backups, restore, snapshot browsing, and\n read-only FUSE mounts of PBS archives.\n' "$VERSION" > "$PKG/DEBIAN/control"
printf '#!/bin/sh\nset -e\nsystemctl --user daemon-reload 2>/dev/null || true\nsystemctl --user enable backuppilot-daemon.service 2>/dev/null || true\nsystemctl --user start backuppilot-daemon.service 2>/dev/null || true\n' > "$PKG/DEBIAN/postinst"
chmod 755 "$PKG/DEBIAN/postinst"
printf '#!/bin/sh\nset -e\nsystemctl --user stop backuppilot-daemon.service 2>/dev/null || true\nsystemctl --user disable backuppilot-daemon.service 2>/dev/null || true\n' > "$PKG/DEBIAN/prerm"
chmod 755 "$PKG/DEBIAN/prerm"

dpkg-deb --build "$PKG"
cp "${PKG}.deb" /dist/
echo "Done: backuppilot_${VERSION}_ubuntu2604_amd64.deb"
EOF

  docker run --rm \
    -v "$REPO:/src" \
    -v "backuppilot-target-2604:/build/target" \
    -v "backuppilot-cargo-2604:/root/.cargo" \
    -v "$DIST:/dist" \
    -e CARGO_TARGET_DIR=/build/target \
    ubuntu:26.04 \
    bash -c "$SCRIPT"
}

case "$TARGET" in
  ubuntu2404) build_2404 ;;
  ubuntu2604) build_2604 ;;
  all) build_2404; build_2604 ;;
  *) echo "Usage: $0 [ubuntu2404|ubuntu2604|all]"; exit 1 ;;
esac

echo ""
echo "Packages in dist/:"
ls -lh "$DIST"/*.deb 2>/dev/null || true
