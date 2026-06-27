<div align="center">

<img src="packaging/tcode.png" width="120" alt="Tcode logo">

# Tcode

### A fast, borderless tiling-terminal workspace for Linux

Pick a number — get that many terminal panes in a balanced grid. Keyboard-driven,
with a file sidebar, a universal viewer for **code · images · PDFs · office · CSV**,
a searchable **clipboard history**, and a built-in **screenshot annotator**.

[![License: MIT](https://img.shields.io/badge/license-MIT-F2660C)](LICENSE)
&nbsp;![Platform: Linux](https://img.shields.io/badge/platform-Linux-F2660C?logo=linux&logoColor=white)
&nbsp;![Built with Rust](https://img.shields.io/badge/built%20with-Rust-F2660C?logo=rust&logoColor=white)

<img src="docs/screenshot.png" width="860" alt="Tcode: a 2×2 terminal grid with the file sidebar">

</div>

---

## Quick start

Three steps. No source code, no Rust toolchain, no setup.

#### 1 · Download

Grab the latest **`tcode_<version>_amd64.deb`** from the
[**Releases**](https://github.com/moamen1358/Tcode/releases/latest) page.

#### 2 · Install

```bash
sudo apt install ./tcode_*.deb
```

`apt` pulls in the handful of system libraries Tcode uses — nothing else to set up.

#### 3 · Run

Launch **Tcode** from your app menu, or from a terminal:

```bash
tcode        # choose how many panes
tcode 4      # jump straight to a 2×2 grid in the current folder
```

That's it — you're running it exactly the way it's meant to be used. To update
later, just run **`tcode update`** (it fetches and installs the newest release).

---

## Features

### ◧ Tiling terminal grid

Run `tcode N` and get **N** terminal panes in a balanced, equal-split grid — every
pane the same size, no fiddly dragging. Each pane is a plain login shell, so there
are no surprises. Move focus with `Alt`+arrows, zoom one pane full-screen with
`Alt+Z`, or rebuild the whole grid instantly with `Alt+1`…`Alt+9`.

### 🗂 Universal viewer

Ctrl+click any file path in a terminal — or pick a file in the sidebar — and it
opens in a tabbed viewer beside your panes: syntax-highlighted **code**, **images**,
**PDFs**, **office** documents (Word / PowerPoint / Excel, via LibreOffice), and
**CSV** as a real table. The panel is width-capped, so it never squeezes the terminals.

### 📋 Clipboard history

Every clip you copy is captured into a searchable history. Press **`Alt+V`** for the
command palette: type to filter, **Enter** to copy a past entry back, **pin** the ones
you reuse, and **delete** anything you don't want kept — each remembers when it was
captured. History stays in memory by default; set `clipboard_persist = true` to keep
it across restarts.

<p align="center">
  <img src="docs/clipboard.png" width="820" alt="Tcode's Alt+V clipboard palette: a searchable list of copied entries with capture times and pin / delete actions">
</p>

### 📸 Frame — capture &amp; annotate

The titlebar camera grabs any window or region (via the desktop screenshot portal),
then hands it to **Frame** — a built-in canvas where you draw boxes, arrows, freehand
pen, highlighter, and text in any color. **Save** exports a PNG (also copied to your
clipboard) and collects it in the screenshots strip (`Alt+P`), ready to drag straight
into a terminal. A freshly captured shot also floats over the grid as a preview you can
reposition or dismiss first.

<p align="center">
  <img src="docs/frame.png" width="820" alt="Frame annotating a captured screenshot — toolbar and color palette on top, boxes and arrows on the grid, and the screenshots strip down the right edge">
</p>

---

## Keyboard shortcuts

Every binding is also listed **in the app** — open the gear (⚙) in the titlebar.

| Key | Action |
|-----|--------|
| `Alt` + arrow keys / `h` `j` `k` `l` | Move focus between panes |
| `Alt` + `1` … `9` | Rebuild the grid with N panes |
| `Alt` + `N` | New terminal (add a pane) |
| `Alt` + `Z` | Zoom the focused pane / restore |
| `Alt` + `F` | Toggle fullscreen |
| `Alt` + `B` | Toggle the file sidebar |
| `Alt` + `V` | Clipboard history palette |
| `Alt` + `P` | Screenshots strip |
| `Alt` + `Q` | Quit |
| `Ctrl` + `Shift` + `C` / `V` | Copy / paste in the focused terminal |
| `Ctrl` + `+` / `−` / `0` | Zoom the whole UI in / out / reset |

Ctrl+click a path or URL in any terminal to open it; right-click for Copy / Paste.

---

## Configuration

Everything has a sensible default, so a config file is **optional**. To tweak things,
create `~/.config/tcode/config.toml`:

```toml
font              = "Martian Mono"   # bundled; or any installed font
font_size         = 11
startup_command   = ""               # a command to run in every pane on open, e.g. "tmux"
clipboard_persist = false            # keep clipboard history across restarts
scale             = 1.0              # whole-UI zoom (0.5–3.0)
# [theme] background / foreground / accent / surface / border / palette  (Tokyo Night by default)
```

PDF / office / screenshot features light up when `poppler-utils`, `libreoffice`, and
`xdg-desktop-portal` are installed (the `.deb` recommends them automatically).

---

## Build from source

Prefer to build it yourself?

```bash
sudo apt install -y build-essential pkg-config \
  libgtk-4-dev libvte-2.91-gtk4-dev libgtksourceview-5-dev
git clone https://github.com/moamen1358/Tcode && cd Tcode
./packaging/install.sh                         # build + install for your user
# …or just run it in place:
cargo build --release && ./target/release/tcode 4
```

**Run it three ways** — all built from the single version in `Cargo.toml`:

```bash
./run.sh native    # host binary (cargo build + run)
./run.sh docker    # container image  (tcode:<version>)
./run.sh deb       # build + install the .deb, then run
```

Maintainers: `./packaging/build-deb.sh` builds the `.deb`; pushing a `v*` tag publishes
it to Releases automatically (see `.github/workflows/release.yml`).

---

## License

[MIT](LICENSE) © 2026 moamen. Bundled assets keep their own licenses: the
**Martian Mono** font (SIL OFL) and **Tabler Icons** outline shapes (MIT).
