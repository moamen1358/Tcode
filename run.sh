#!/usr/bin/env bash
# Run Tcode one of three ways. Every mode is built from the SINGLE version in
# Cargo.toml, so the host binary, the .deb and the Docker image stay in sync.
#
#   ./run.sh native [N]   compile the release binary and run it on the host
#   ./run.sh docker [N]   build + run the container image  (tcode:<version>)
#   ./run.sh deb    [N]   build + install the .deb, then run the installed app
#
# N = optional pane count (e.g. 4 -> a 2x2 grid). No N -> the session picker.
# Panes open in the directory you run this from.
set -euo pipefail
WHERE="$(pwd)"                         # where you invoked it — panes open here
HERE="$(cd "$(dirname "$0")" && pwd)"  # the repo — where we build

VERSION="$(grep -m1 '^version' "$HERE/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
ARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
TYPE="${1:-}"
N="${2:-}"

usage() {
    cat <<EOF
Tcode v$VERSION — run it three ways (all the same version):

  ./run.sh native [N]   compile + run the release binary on the host
  ./run.sh docker [N]   build + run the Docker image  (tcode:$VERSION)
  ./run.sh deb    [N]   build + install the .deb, then run the installed app

  N   optional pane count, e.g.  ./run.sh native 4
EOF
}

# Render Tcode from inside the container on the host display (prefers Wayland,
# falls back to X11). Mounts your invocation dir at /work so panes see your files.
run_docker() {
    local image="tcode:${VERSION}"
    if ! docker image inspect "$image" >/dev/null 2>&1; then
        echo "Building $image (first run — compiles the GTK stack, takes a few minutes)…"
        docker build --build-arg VERSION="$VERSION" -t "$image" -t "tcode:latest" "$HERE"
    fi
    local dri=(); [ -d /dev/dri ] && dri=(--device /dev/dri)   # optional GPU passthrough

    if [ -n "${WAYLAND_DISPLAY:-}" ] && [ -S "${XDG_RUNTIME_DIR:-}/${WAYLAND_DISPLAY}" ]; then
        echo "Launching on Wayland ($WAYLAND_DISPLAY)…"
        exec docker run --rm -it \
            -e WAYLAND_DISPLAY="$WAYLAND_DISPLAY" -e XDG_RUNTIME_DIR=/tmp \
            -e GDK_BACKEND=wayland -e GSK_RENDERER="${GSK_RENDERER:-}" \
            -v "${XDG_RUNTIME_DIR}/${WAYLAND_DISPLAY}:/tmp/${WAYLAND_DISPLAY}" \
            -v "${WHERE}:/work" -w /work "${dri[@]}" \
            "$image" ${N:+"$N"}
    else
        echo "Launching on X11 ($DISPLAY)…"
        # Grant X access for the container's lifetime only, then revoke it.
        xhost +local:docker >/dev/null 2>&1 || true
        local status=0
        docker run --rm -it \
            -e DISPLAY="$DISPLAY" -e GDK_BACKEND=x11 -e GSK_RENDERER="${GSK_RENDERER:-}" \
            -v /tmp/.X11-unix:/tmp/.X11-unix \
            -v "${WHERE}:/work" -w /work "${dri[@]}" \
            "$image" ${N:+"$N"} || status=$?
        xhost -local:docker >/dev/null 2>&1 || true
        exit "$status"
    fi
}

case "$TYPE" in
  native)
    echo "▶ native · Tcode v$VERSION (host binary)"
    ( cd "$HERE" && cargo build --release -p tcode )
    cd "$WHERE"; exec "$HERE/target/release/tcode" ${N:+"$N"}
    ;;
  docker)
    echo "▶ docker · Tcode v$VERSION (image tcode:$VERSION)"
    run_docker
    ;;
  deb)
    echo "▶ deb · Tcode v$VERSION (system package)"
    DEB="$HERE/dist/tcode_${VERSION}_${ARCH}.deb"
    [ -f "$DEB" ] || ( cd "$HERE" && ./packaging/build-deb.sh )
    installed="$(dpkg-query -W -f='${Version}' tcode 2>/dev/null || true)"
    if [ "$installed" != "$VERSION" ]; then
        echo "Installing the .deb (needs your password)…"
        pkexec apt-get install -y --allow-downgrades "$DEB"
    fi
    cd "$WHERE"; exec tcode ${N:+"$N"}
    ;;
  ""|-h|--help|help)
    usage
    ;;
  *)
    echo "Unknown type: '$TYPE'" >&2; usage; exit 1
    ;;
esac
