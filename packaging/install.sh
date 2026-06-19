#!/usr/bin/env bash
# Install Tessera as a desktop application for the current user (no root needed).
set -euo pipefail
cd "$(dirname "$0")/.."
REPO="$(pwd)"

echo "Building + installing the tessera binary (cargo install)…"
cargo install --path crates/tessera --force

BIN="$(command -v tessera || echo "$HOME/.cargo/bin/tessera")"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}/tessera"
mkdir -p "$APPS" "$ICONS/scalable/apps" "$ICONS/256x256/apps" "$DATA"

# Bake the absolute binary path into Exec so the launcher works regardless of
# the desktop session's PATH (it often doesn't include ~/.cargo/bin).
sed "s|^Exec=.*|Exec=$BIN|" packaging/tessera.desktop >"$APPS/tessera.desktop"
install -m644 packaging/tessera.svg "$ICONS/scalable/apps/tessera.svg"
install -m644 packaging/icons/tessera-256.png "$ICONS/256x256/apps/tessera.png"

# Record the source location so `tessera update` can find it later.
printf '%s\n' "$REPO" >"$DATA/source"

update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -f -t "$ICONS" 2>/dev/null || true

echo "Done — 'Tessera' should appear in your app launcher (or run: tessera)."
echo "Update any time with:  tessera update"
