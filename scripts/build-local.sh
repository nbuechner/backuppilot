#!/usr/bin/env bash
# Build BackupPilot release packages locally.
#   Linux .deb  — built inside Docker or LXD containers (no host pollution)
#   Windows NSIS — built on the Windows host via SSH, installer copied back
#
# Cargo, npm, and rustup caches survive between runs (Docker volumes / host dirs).
#
# Usage: ./scripts/build-local.sh [ubuntu2404|ubuntu2604|windows|all]
# BUILD_BACKEND=lxd   force LXD  (default: docker if available, else lxd)
# BUILD_BACKEND=docker force Docker
set -euo pipefail

TARGET="${1:-all}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$REPO/dist"
WIN_HOST="${WIN_HOST:-user@10.99.158.244}"
WIN_REPO='C:\Users\user\backuppilot'

mkdir -p "$DIST"

VERSION=$(grep '^version' "$REPO/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
echo "BackupPilot v${VERSION}"

# ── Build backend detection ───────────────────────────────────────────────────
if [[ "${BUILD_BACKEND:-}" == "lxd" ]]; then
  BUILDER="lxd"
elif [[ "${BUILD_BACKEND:-}" == "docker" ]]; then
  BUILDER="docker"
elif command -v lxc &>/dev/null; then
  BUILDER="lxd"
elif command -v docker &>/dev/null; then
  BUILDER="docker"
else
  echo "ERROR: neither docker nor lxc found in PATH"; exit 1
fi
echo "Build backend: $BUILDER"

# ── LXD helper ───────────────────────────────────────────────────────────────
# Persistent host-side cache dirs (survive container deletion, like Docker volumes)
LXD_CACHE="${HOME}/.cache/backuppilot-build"

lxd_run() {
  local label="$1"   # e.g. "2404"
  local image="$2"   # e.g. "ubuntu:24.04"
  local script="$3"  # build script content
  local extra_devs="${4:-}"  # optional extra lxc config device add lines
  local cname="bp-build-${label}-$$"

  mkdir -p \
    "$LXD_CACHE/cargo-${label}" \
    "$LXD_CACHE/target-${label}"

  echo "  Launching LXD container $cname ($image)..."
  lxc launch "$image" "$cname"
  # Always clean up the container, even on error
  # shellcheck disable=SC2064
  trap "echo '  Cleaning up $cname...'; lxc delete --force '$cname' 2>/dev/null || true" EXIT INT TERM

  # Wait until the container is fully booted
  lxc exec "$cname" -- cloud-init status --wait 2>/dev/null || sleep 5

  # Source: read-only; world-readable so no UID shift needed
  lxc config device add "$cname" src  disk source="$REPO" path=/src readonly=true
  # Output dir: shift=true maps container root UID to host dir owner
  lxc config device add "$cname" dist disk source="$DIST"                      path=/dist         shift=true
  # Persistent build caches (cargo registry + compiled artifacts)
  lxc config device add "$cname" cargo disk source="$LXD_CACHE/cargo-${label}" path=/root/.cargo  shift=true
  lxc config device add "$cname" tgt   disk source="$LXD_CACHE/target-${label}" path=/build/target shift=true

  # Extra per-build devices (e.g. node_modules cache)
  if [[ -n "$extra_devs" ]]; then
    eval "$extra_devs"
  fi

  lxc exec "$cname" \
    --env CARGO_TARGET_DIR=/build/target \
    --env DEBIAN_FRONTEND=noninteractive \
    -- bash -c "$script"

  echo "  Deleting $cname..."
  lxc delete --force "$cname"
  trap - EXIT INT TERM
}

# ── ubuntu-24.04: Tauri / WebKitGTK ─────────────────────────────────────────
build_2404() {
  echo ""
  echo "=== ubuntu-24.04 (Tauri / WebKitGTK) ==="

  read -r -d '' SCRIPT << 'EOF' || true
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -y -q --no-install-recommends \
  curl ca-certificates build-essential \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libdbus-1-dev pkg-config libssl-dev

if [ ! -f /root/.cargo/bin/rustup ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --no-modify-path
fi
export PATH="/root/.cargo/bin:$PATH"
rustup default stable 2>/dev/null || rustup toolchain install stable --no-self-update

if ! command -v node &>/dev/null; then
  curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
  apt-get install -y nodejs
fi

cd /src
cd crates/backuppilot-tauri/ui && npm ci --prefer-offline && npm run build && cd /src

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

printf 'Package: backuppilot\nVersion: %s\nArchitecture: amd64\nMaintainer: BackupPilot Contributors <noreply@buechner.me>\nDepends: fuse3, libwebkit2gtk-4.1-0\nRecommends: proxmox-backup-client\nDescription: BackupPilot - Proxmox Backup Server GUI client\n Provides scheduled backups, restore, snapshot browsing, and\n read-only FUSE mounts of PBS archives.\n' "$VERSION" > "$PKG/DEBIAN/control"
cp data/DEBIAN/postinst "$PKG/DEBIAN/postinst"
chmod 755 "$PKG/DEBIAN/postinst"
cp data/DEBIAN/prerm "$PKG/DEBIAN/prerm"
chmod 755 "$PKG/DEBIAN/prerm"

dpkg-deb --build "$PKG"
cp "${PKG}.deb" /dist/
echo "Done: backuppilot_${VERSION}_ubuntu2404_amd64.deb"
EOF

  if [[ "$BUILDER" == "lxd" ]]; then
    mkdir -p "$LXD_CACHE/node-2404"
    # Capture cname for the extra_devs eval (lxd_run sets cname before calling eval)
    EXTRA='lxc config device add "$cname" node disk source="'"$LXD_CACHE"'/node-2404" path=/src/crates/backuppilot-tauri/ui/node_modules shift=true'
    lxd_run "2404" "ubuntu:24.04" "$SCRIPT" "$EXTRA"
  else
    docker run --rm \
      -v "$REPO:/src" \
      -v "backuppilot-target-2404:/build/target" \
      -v "backuppilot-cargo-2404:/root/.cargo" \
      -v "backuppilot-node-2404:/src/crates/backuppilot-tauri/ui/node_modules" \
      -v "$DIST:/dist" \
      -e CARGO_TARGET_DIR=/build/target \
      ubuntu:24.04 \
      bash -c "$SCRIPT"
  fi
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
rustup default stable 2>/dev/null || rustup toolchain install stable --no-self-update

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

printf 'Package: backuppilot\nVersion: %s\nArchitecture: amd64\nMaintainer: BackupPilot Contributors <noreply@buechner.me>\nDepends: fuse3, libadwaita-1-0 (>= 1.6)\nRecommends: proxmox-backup-client\nDescription: BackupPilot - Proxmox Backup Server GUI client\n Provides scheduled backups, restore, snapshot browsing, and\n read-only FUSE mounts of PBS archives.\n' "$VERSION" > "$PKG/DEBIAN/control"
cp data/DEBIAN/postinst "$PKG/DEBIAN/postinst"
chmod 755 "$PKG/DEBIAN/postinst"
cp data/DEBIAN/prerm "$PKG/DEBIAN/prerm"
chmod 755 "$PKG/DEBIAN/prerm"

dpkg-deb --build "$PKG"
cp "${PKG}.deb" /dist/
echo "Done: backuppilot_${VERSION}_ubuntu2604_amd64.deb"
EOF

  if [[ "$BUILDER" == "lxd" ]]; then
    lxd_run "2604" "ubuntu:26.04" "$SCRIPT"
  else
    docker run --rm \
      -v "$REPO:/src" \
      -v "backuppilot-target-2604:/build/target" \
      -v "backuppilot-cargo-2604:/root/.cargo" \
      -v "$DIST:/dist" \
      -e CARGO_TARGET_DIR=/build/target \
      ubuntu:26.04 \
      bash -c "$SCRIPT"
  fi
}

# ── Windows: NSIS installer via SSH ──────────────────────────────────────────
build_windows() {
  echo ""
  echo "=== Windows NSIS installer (via SSH to $WIN_HOST) ==="

  # Kill any running instances so the OS releases the locked executables
  ssh "$WIN_HOST" "taskkill /F /IM backuppilot-daemon.exe /IM backuppilot-tauri.exe 2>nul & exit 0" || true

  # Sync latest code
  ssh "$WIN_HOST" "cd /d ${WIN_REPO} && git pull"

  # Build frontend
  ssh "$WIN_HOST" "cd /d ${WIN_REPO}\\crates\\backuppilot-tauri\\ui && npm ci --prefer-offline && npm run build"

  # Build daemon (must exist before Tauri bundles resources)
  ssh "$WIN_HOST" "cd /d ${WIN_REPO} && cargo build -p backuppilot-daemon --release"

  # Build Tauri NSIS installer
  ssh "$WIN_HOST" "cd /d ${WIN_REPO}\\crates\\backuppilot-tauri && npx --yes @tauri-apps/cli@v2 build --bundles nsis"

  # Copy installer back to local dist/
  scp "${WIN_HOST}:${WIN_REPO//\\/\/}/target/release/bundle/nsis/*.exe" "$DIST/"

  # Also drop it on the Windows Desktop for easy access
  ssh "$WIN_HOST" "copy ${WIN_REPO}\\target\\release\\bundle\\nsis\\*.exe C:\\Users\\user\\Desktop\\ >nul"

  echo "Done: Windows installer copied to dist/ and Windows Desktop"
}

case "$TARGET" in
  ubuntu2404) build_2404 ;;
  ubuntu2604) build_2604 ;;
  windows)    build_windows ;;
  all)        build_2404; build_2604; build_windows ;;
  *) echo "Usage: $0 [ubuntu2404|ubuntu2604|windows|all]"; exit 1 ;;
esac

echo ""
echo "Packages in dist/:"
ls -lh "$DIST"/ 2>/dev/null || true
