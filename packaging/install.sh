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
mkdir -p "$APPS"

# Bake the absolute binary path into Exec so the launcher works regardless of
# the desktop session's PATH (it often doesn't include ~/.cargo/bin). The file is
# named after the GTK app_id (dev.tessera.Tessera) so Wayland maps the running
# window to this entry and shows its icon.
rm -f "$APPS/tessera.desktop"   # drop the old misnamed entry from earlier installs
sed "s|^Exec=.*|Exec=$BIN|" packaging/dev.tessera.Tessera.desktop >"$APPS/dev.tessera.Tessera.desktop"
for sz in 48 64 128 256; do
    mkdir -p "$ICONS/${sz}x${sz}/apps"
    install -m644 "packaging/icons/tessera-${sz}.png" "$ICONS/${sz}x${sz}/apps/tessera.png"
done

update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -f -t "$ICONS" 2>/dev/null || true

echo "Done — 'Tessera' should appear in your app launcher (or run: tessera)."
