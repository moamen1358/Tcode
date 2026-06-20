#!/usr/bin/env bash
# Build a Debian package (.deb) for Tessera — a download-and-install bundle that
# needs no source code or Rust toolchain on the user's machine.
#
#   ./packaging/build-deb.sh        ->  dist/tessera_<version>_<arch>.deb
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
ARCH="$(dpkg --print-architecture)"
PKG="tessera_${VERSION}_${ARCH}"
STAGE="$(mktemp -d)/${PKG}"

echo "Building release binary…"
cargo build --release

echo "Staging package tree…"
install -Dm755 target/release/tessera         "$STAGE/usr/bin/tessera"
install -Dm644 packaging/dev.tessera.Tessera.desktop "$STAGE/usr/share/applications/dev.tessera.Tessera.desktop"
for sz in 48 64 128 256; do
    install -Dm644 "packaging/icons/tessera-${sz}.png" "$STAGE/usr/share/icons/hicolor/${sz}x${sz}/apps/tessera.png"
    # Also under the app_id name so launchers that look the icon up by app_id
    # (rather than reading Icon= from the matched .desktop) resolve it on Wayland.
    install -Dm644 "packaging/icons/tessera-${sz}.png" "$STAGE/usr/share/icons/hicolor/${sz}x${sz}/apps/dev.tessera.Tessera.png"
done
# On a system install the binary is on PATH and the icon is in the theme.
sed -i 's|^Exec=.*|Exec=tessera|; s|^Icon=.*|Icon=dev.tessera.Tessera|' "$STAGE/usr/share/applications/dev.tessera.Tessera.desktop"

INSTALLED_KB="$(du -sk "$STAGE/usr" | cut -f1)"
mkdir -p "$STAGE/DEBIAN"
cat >"$STAGE/DEBIAN/control" <<CTRL
Package: tessera
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Depends: libgtk-4-1, libvte-2.91-gtk4-0, libgtksourceview-5-0, librsvg2-common, curl
Recommends: poppler-utils, xdg-desktop-portal, policykit-1
Suggests: libreoffice
Installed-Size: ${INSTALLED_KB}
Maintainer: moamen <moamen1358@users.noreply.github.com>
Homepage: https://github.com/moamen1358/tessera
Description: Borderless tiling-terminal workspace
 Tessera is a fast, keyboard-driven tiling terminal for Linux: pick a number and
 get that many terminal panes in a balanced grid, with a file sidebar and a
 universal file viewer. Built in Rust with GTK4 and VTE.
CTRL

# Refresh the desktop + icon caches after install / removal.
cat >"$STAGE/DEBIAN/postinst" <<'POST'
#!/bin/sh
set -e
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
exit 0
POST
chmod 755 "$STAGE/DEBIAN/postinst"
cp "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/postrm"

mkdir -p dist
dpkg-deb --root-owner-group --build "$STAGE" "dist/${PKG}.deb" >/dev/null
echo "Built dist/${PKG}.deb"
