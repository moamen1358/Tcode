#!/usr/bin/env bash
# Run Tessera inside Docker, rendering on the host display.
# Usage:  ./run-docker.sh [pane-count]
#   e.g.  ./run-docker.sh 4
#
# Mounts the current directory at /work so the terminal panes operate on your
# host files. Prefers Wayland; falls back to X11.
set -euo pipefail

IMAGE="tessera:latest"
N="${1:-}"
HERE="$(cd "$(dirname "$0")" && pwd)"

# Build the image on first run (or after code changes: docker build -t tessera .).
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "Building $IMAGE (first run — compiles the GTK stack, takes a few minutes)…"
    docker build -t "$IMAGE" "$HERE"
fi

# Optional GPU passthrough (only if the host exposes /dev/dri).
DRI=()
[ -d /dev/dri ] && DRI=(--device /dev/dri)

WORK="$(pwd)"

if [ -n "${WAYLAND_DISPLAY:-}" ] && [ -S "${XDG_RUNTIME_DIR:-}/${WAYLAND_DISPLAY}" ]; then
    echo "Launching on Wayland ($WAYLAND_DISPLAY)…"
    exec docker run --rm -it \
        -e WAYLAND_DISPLAY="$WAYLAND_DISPLAY" \
        -e XDG_RUNTIME_DIR=/tmp \
        -e GDK_BACKEND=wayland \
        -e GSK_RENDERER="${GSK_RENDERER:-}" \
        -v "${XDG_RUNTIME_DIR}/${WAYLAND_DISPLAY}:/tmp/${WAYLAND_DISPLAY}" \
        -v "${WORK}:/work" -w /work \
        "${DRI[@]}" \
        "$IMAGE" ${N:+$N}
else
    echo "Launching on X11 ($DISPLAY)…"
    # Grant X access only for the container's lifetime, then revoke — narrower than
    # leaving `xhost +local:docker` enabled afterwards. (The Wayland path needs no
    # such grant; for stricter isolation use an XAUTHORITY cookie instead.)
    xhost +local:docker >/dev/null 2>&1 || true
    docker run --rm -it \
        -e DISPLAY="$DISPLAY" \
        -e GDK_BACKEND=x11 \
        -e GSK_RENDERER="${GSK_RENDERER:-}" \
        -v /tmp/.X11-unix:/tmp/.X11-unix \
        -v "${WORK}:/work" -w /work \
        "${DRI[@]}" \
        "$IMAGE" ${N:+$N} || true
    xhost -local:docker >/dev/null 2>&1 || true
fi
