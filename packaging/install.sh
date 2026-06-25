#!/usr/bin/env bash
# Build Tcode from source and install it for the current user (no root needed).
# Most people don't need this — just download the .deb from the Releases page.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "Building + installing the tcode binary (cargo install)…"
cargo install --path crates/tcode --force

BIN="$(command -v tcode || echo "$HOME/.cargo/bin/tcode")"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor"
mkdir -p "$APPS"

# Bake the absolute binary path into Exec so the launcher works regardless of
# the desktop session's PATH (it often doesn't include ~/.cargo/bin). The file is
# named after the GTK app_id (dev.tcode.Tcode) so Wayland maps the running
# window to this entry and shows its icon.
rm -f "$APPS/tcode.desktop"   # drop the old misnamed entry from earlier installs
sed "s|^Exec=.*|Exec=$BIN|" packaging/dev.tcode.Tcode.desktop >"$APPS/dev.tcode.Tcode.desktop"
for sz in 48 64 128 256; do
    mkdir -p "$ICONS/${sz}x${sz}/apps"
    install -m644 "packaging/icons/tcode-${sz}.png" "$ICONS/${sz}x${sz}/apps/tcode.png"
    install -m644 "packaging/icons/tcode-${sz}.png" "$ICONS/${sz}x${sz}/apps/dev.tcode.Tcode.png"
done

update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -f -t "$ICONS" 2>/dev/null || true

echo "Done — 'Tcode' should appear in your app launcher (or run: tcode)."
