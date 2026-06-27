<div align="center">

<img src="packaging/tcode.png" width="104" alt="Tcode">

# Tcode

**A fast, borderless tiling-terminal workspace for Linux.**

Pick a number → get that many terminal panes in a clean grid.

[![Release](https://img.shields.io/badge/release-v1.3.0-F2660C)](https://github.com/moamen1358/Tcode/releases/latest)
&nbsp;[![License](https://img.shields.io/badge/license-MIT-F2660C)](LICENSE)
&nbsp;![Linux](https://img.shields.io/badge/platform-Linux-F2660C?logo=linux&logoColor=white)
&nbsp;![Rust](https://img.shields.io/badge/built%20with-Rust-F2660C?logo=rust&logoColor=white)

<br>

<img src="docs/screenshot.png" width="840" alt="Tcode — a 2×2 terminal grid with the file sidebar">

</div>

<br>

## Install

No source, no Rust, no setup — three steps.

**1.** Download **`tcode_1.3.0_amd64.deb`** from the [**latest release**](https://github.com/moamen1358/Tcode/releases/latest).

**2.** Install it:

```bash
sudo apt install ./tcode_1.3.0_amd64.deb
```

**3.** Run it:

```bash
tcode        # pick how many panes
tcode 4      # straight to a 2×2 grid
```

<br>

## Features

| | |
|---|---|
| **◧&nbsp; Tiling grid** | `tcode N` → **N** equal panes, no dragging. Move focus with the arrows, zoom one full-screen, rebuild instantly. |
| **🗂&nbsp; Universal viewer** | Open **code · images · PDFs · office · CSV** in a tab beside your panes. Ctrl+click a path to jump to it. |
| **📋&nbsp; Clipboard history** | Every copy is saved. <kbd>Alt</kbd>+<kbd>V</kbd> opens a searchable palette — re-copy, pin, or delete past clips. |
| **📸&nbsp; Frame** | Capture a window or region, annotate with boxes / arrows / text, then save and drag it into a terminal. |

<table>
<tr>
<td width="50%" valign="top">
<img src="docs/clipboard.png" alt="The Alt+V clipboard palette: a searchable list of copied entries with capture times and pin / delete actions">
<p align="center"><sub><b>Clipboard history</b> — search and re-use anything you've copied</sub></p>
</td>
<td width="50%" valign="top">
<img src="docs/frame.png" alt="Frame annotating a screenshot — toolbar and colors on top, boxes and arrows on the image, screenshots strip on the right">
<p align="center"><sub><b>Frame</b> — capture, annotate, save</sub></p>
</td>
</tr>
</table>

<br>

## Shortcuts

| Keys | Action |
|---|---|
| <kbd>Alt</kbd> + arrows | Move focus between panes |
| <kbd>Alt</kbd> + <kbd>1</kbd>…<kbd>9</kbd> | Rebuild the grid with N panes |
| <kbd>Alt</kbd> + <kbd>N</kbd> | New terminal |
| <kbd>Alt</kbd> + <kbd>Z</kbd> | Zoom the focused pane |
| <kbd>Alt</kbd> + <kbd>F</kbd> | Fullscreen |
| <kbd>Alt</kbd> + <kbd>B</kbd> | Toggle the file sidebar |
| <kbd>Alt</kbd> + <kbd>V</kbd> | Clipboard history |
| <kbd>Alt</kbd> + <kbd>P</kbd> | Screenshots strip |
| <kbd>Alt</kbd> + <kbd>Q</kbd> | Quit |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>C</kbd> / <kbd>V</kbd> | Copy / paste |
| <kbd>Ctrl</kbd> + <kbd>+</kbd> / <kbd>−</kbd> / <kbd>0</kbd> | Zoom the UI |

<sub>All shortcuts are also in the app — open the gear (⚙) in the titlebar.</sub>

<br>

<details>
<summary><b>Configuration</b></summary>

<br>

Everything has a sensible default — a config file is optional. To tweak, create `~/.config/tcode/config.toml`:

```toml
font              = "Martian Mono"   # bundled, or any installed font
font_size         = 11
startup_command   = ""               # run in every pane on open, e.g. "tmux"
clipboard_persist = false            # keep clipboard history across restarts
scale             = 1.0              # whole-UI zoom (0.5–3.0)
# [theme] background / foreground / accent / surface / border / palette
```

PDF, office, and screenshot features light up when `poppler-utils`, `libreoffice`, and `xdg-desktop-portal` are present — the `.deb` recommends them automatically.

</details>

<details>
<summary><b>Build from source</b></summary>

<br>

```bash
sudo apt install -y build-essential pkg-config \
  libgtk-4-dev libvte-2.91-gtk4-dev libgtksourceview-5-dev
git clone https://github.com/moamen1358/Tcode && cd Tcode
./packaging/install.sh                 # build + install for your user
# …or run it in place:
cargo build --release && ./target/release/tcode 4
```

Run it three ways, all versioned from `Cargo.toml`:

```bash
./run.sh native    # host binary
./run.sh docker    # container image
./run.sh deb       # build + install the .deb
```

Maintainers: `./packaging/build-deb.sh` builds the `.deb`; pushing a `v*` tag publishes it to Releases (`.github/workflows/release.yml`).

</details>

<br>

## License

[MIT](LICENSE) © 2026 moamen. Bundled **Martian Mono** (SIL OFL) and **Tabler Icons** (MIT) keep their own licenses.
