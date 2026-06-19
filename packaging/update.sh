#!/usr/bin/env bash
# Update Tessera to the latest version: pull the newest source and reinstall.
# Run directly (./packaging/update.sh) or via `tessera update`.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "Fetching the latest Tessera…"
if [ -d .git ]; then
    git pull --ff-only
else
    echo "Note: not a git checkout — skipping pull; reinstalling the current source." >&2
fi

exec ./packaging/install.sh
