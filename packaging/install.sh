#!/usr/bin/env bash
# Install Tessera as a desktop application for the current user (no root needed).
set -euo pipefail
cd "$(dirname "$0")/.."

echo "Building + installing the tessera binary (cargo install)…"
cargo install --path crates/tessera

BIN="$(command -v tessera || echo "$HOME/.cargo/bin/tessera")"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$APPS" "$ICONS"

# Bake the absolute binary path into Exec so the launcher works regardless of
# the desktop session's PATH (it often doesn't include ~/.cargo/bin).
sed "s|^Exec=.*|Exec=$BIN|" packaging/tessera.desktop >"$APPS/tessera.desktop"
install -m644 packaging/tessera.svg "$ICONS/tessera.svg"

update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

echo "Done — 'Tessera' should appear in your app launcher (or run: tessera)."
