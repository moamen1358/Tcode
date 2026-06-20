#!/usr/bin/env bash
# Run Tessera one of three ways. Every mode is built from the SINGLE version in
# Cargo.toml, so the host binary, the .deb and the Docker image are always in sync.
#
#   ./run.sh native [N]   compile the release binary and run it on the host
#   ./run.sh docker [N]   build + run the container image  (tessera:<version>)
#   ./run.sh deb    [N]   build + install the .deb, then run the installed app
#
# N = optional pane count (e.g. 4 -> a 2x2 grid). No N -> the session picker.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"

# Single source of truth for the version — shared by all three modes.
VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
ARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
TYPE="${1:-}"
N="${2:-}"

usage() {
    cat <<EOF
Tessera v$VERSION — run it three ways (all the same version):

  ./run.sh native [N]   compile + run the release binary on the host
  ./run.sh docker [N]   build + run the Docker image  (tessera:$VERSION)
  ./run.sh deb    [N]   build + install the .deb, then run the installed app

  N   optional pane count, e.g.  ./run.sh native 4
EOF
}

case "$TYPE" in
  native)
    echo "▶ native · Tessera v$VERSION (host binary)"
    cargo build --release -p tessera
    exec ./target/release/tessera ${N:+"$N"}
    ;;
  docker)
    echo "▶ docker · Tessera v$VERSION (image tessera:$VERSION)"
    exec ./run-docker.sh ${N:+"$N"}
    ;;
  deb)
    echo "▶ deb · Tessera v$VERSION (system package)"
    DEB="dist/tessera_${VERSION}_${ARCH}.deb"
    [ -f "$DEB" ] || ./packaging/build-deb.sh
    installed="$(dpkg-query -W -f='${Version}' tessera 2>/dev/null || true)"
    if [ "$installed" != "$VERSION" ]; then
        echo "Installing $DEB (needs your password)…"
        pkexec apt-get install -y --allow-downgrades "$HERE/$DEB"
    fi
    exec tessera ${N:+"$N"}
    ;;
  ""|-h|--help|help)
    usage
    ;;
  *)
    echo "Unknown type: '$TYPE'" >&2
    usage
    exit 1
    ;;
esac
