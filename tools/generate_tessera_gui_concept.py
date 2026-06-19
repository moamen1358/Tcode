#!/usr/bin/env python3
"""Generate a Tessera GUI concept image with OpenAI GPT Image 2.

Requires:
  OPENAI_API_KEY=... python3 tools/generate_tessera_gui_concept.py

The script uses only Python's standard library and writes:
  docs/tessera-gui-concept-ai.png
"""

from __future__ import annotations

import base64
import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.request


OUT = Path("docs/tessera-gui-concept-ai.png")

PROMPT = """Generate a high-fidelity improved GUI concept for Tessera, a Linux
tiling-terminal workspace app, showing better tool layout and coloring for
implementation inspiration.

Style: polished desktop application UI screenshot mockup, modern GTK/Linux
design, production-quality developer tool UI, not a website landing page.

Composition: 16:10 widescreen app window filling the image, no device frame,
straight-on screenshot view, dense but organized workspace.

UI layout:
- Thin compact top command bar with small icon buttons for file panel, add
  terminal, screenshot capture, gallery, editor toggle, fullscreen, and
  close/minimize controls.
- Left sidebar with a file tree rooted at coding_Space, small monochrome file
  icons, clear hover/selected rows, and compact screenshot thumbnails at the
  bottom.
- Central balanced 2x2 terminal grid with subtle separators, one active pane
  outlined in warm amber, realistic shell prompts and command output.
- Right docked editor/preview panel with tabs: main.rs, README.md, preview.png;
  show a code editor with line numbers and a document/image preview area.
- Integrated BridgeShot annotation toolbar over the workspace with clear tools:
  Box, Arrow, Text, Pen, Highlight, Undo, Clear, Cancel, Save, plus color
  swatches.

Color palette: refined dark theme; deep charcoal backgrounds (#111318,
#181b22), slightly lighter panels, muted blue primary accent, warm amber active
focus, soft green success, restrained red warning, off-white text, subtle gray
borders. Avoid a one-color blue/purple look.

Interaction states: active terminal focus ring, selected file row, active
annotation tool, Save as the only stronger call-to-action, inactive buttons
subdued but readable.

Text constraints: Use only short UI text: Tessera, coding_Space, src,
README.md, main.rs, preview.png, Box, Arrow, Text, Pen, Highlight, Undo, Clear,
Cancel, Save, and small terminal/code snippets. Keep text plausible and not
garbled.

Avoid: overlapping UI, giant hero section, marketing copy, decorative blobs or
orbs, bright neon, cartoon style, clutter, rounded pill-heavy controls,
illegible tiny text, watermark."""


def main() -> int:
    api_key = os.environ.get("OPENAI_API_KEY")
    if not api_key:
        print("OPENAI_API_KEY is not set; cannot call GPT Image 2.", file=sys.stderr)
        return 2

    payload = {
        "model": "gpt-image-2",
        "prompt": PROMPT,
        "size": "1536x1024",
        "quality": "high",
        "output_format": "png",
        "n": 1,
    }
    request = urllib.request.Request(
        "https://api.openai.com/v1/images/generations",
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(request, timeout=300) as response:
            body = response.read()
    except urllib.error.HTTPError as err:
        detail = err.read().decode("utf-8", errors="replace")
        print(f"OpenAI API error {err.code}: {detail}", file=sys.stderr)
        return 1
    except urllib.error.URLError as err:
        print(f"Network error: {err}", file=sys.stderr)
        return 1

    data = json.loads(body)
    try:
        b64 = data["data"][0]["b64_json"]
    except (KeyError, IndexError, TypeError) as err:
        print(f"Unexpected API response shape: {err}\n{data}", file=sys.stderr)
        return 1

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(base64.b64decode(b64))
    print(OUT)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
