#!/usr/bin/env bash
# Build Tessera from source and install it for the current user (no root needed).
# Most people don't need this — just download the .deb from the Releases page.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "Building + installing the tessera binary (cargo install)…"
cargo install --path crates/tessera --force

BIN="$(command -v tessera || echo "$HOME/.cargo/bin/tessera")"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor"
mkdir -p "$APPS" "$ICONS/scalable/apps" "$ICONS/256x256/apps"

# Bake the absolute binary path into Exec so the launcher works regardless of
# the desktop session's PATH (it often doesn't include ~/.cargo/bin).
sed "s|^Exec=.*|Exec=$BIN|" packaging/tessera.desktop >"$APPS/tessera.desktop"
install -m644 packaging/tessera.svg "$ICONS/scalable/apps/tessera.svg"
install -m644 packaging/icons/tessera-256.png "$ICONS/256x256/apps/tessera.png"

update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -f -t "$ICONS" 2>/dev/null || true

echo "Done — 'Tessera' should appear in your app launcher (or run: tessera)."
