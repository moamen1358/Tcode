<div align="center">

<img src="packaging/tcode.png" width="120" alt="Tcode logo">

# Tcode

**A fast, borderless tiling-terminal workspace for Linux.**

Pick a number → get that many terminal panes in a balanced grid. Keyboard-driven,
with a file sidebar, a universal viewer for code · images · PDFs · office · CSV,
a searchable clipboard history, and a built-in screenshot annotator.

![License](https://img.shields.io/badge/license-MIT-F2660C)
![Platform](https://img.shields.io/badge/platform-Linux-F2660C?logo=linux&logoColor=white)
![Rust](https://img.shields.io/badge/built%20with-Rust-F2660C?logo=rust&logoColor=white)

</div>

<p align="center">
  <img src="docs/screenshot.png" width="820" alt="Tcode: a 2×2 terminal grid with the file sidebar">
</p>

## Download &amp; install

Grab the latest **`.deb`** from the [**Releases**](https://github.com/moamen1358/Tcode/releases/latest)
page, then install it:

```bash
sudo apt install ./tcode_*.deb
```

That's it — **Tcode** shows up in your app launcher (or run `tcode`). No source
code or Rust toolchain needed; `apt` pulls in the few system libraries it uses.

## Update

```bash
tcode update
```

Checks GitHub, downloads the newest release `.deb`, and installs it. (Or just
download the new `.deb` and `apt install` it again.)

## Usage

```bash
tcode          # session picker
tcode 4        # open a 2x2 grid in the current folder
tcode --help
```

Each pane is a plain terminal running your login shell — no surprises, no agents
started for you. Set a `startup_command` (see [Configuration](#configuration)) if
you want one to run in every pane on open.

## Universal viewer

Ctrl+click any file path in a terminal — or pick a file in the sidebar — and it
opens in a tabbed viewer beside your panes: syntax-highlighted **code**,
**images**, **PDFs**, **office** documents (Word / PowerPoint / Excel, rendered
through LibreOffice), and **CSV** as a real table. The panel is width-capped so
it never squeezes the terminals.

## Clipboard history

Every clip you copy is captured into a searchable history. Press **`Alt+V`** for
the command palette: type to filter, **Enter** to copy a past entry back to the
clipboard, **pin** the ones you reuse to the top, and **delete** anything you
don't want kept — each entry remembers when it was captured. History lives in
memory by default; set `clipboard_persist = true` to keep it across restarts.

<p align="center">
  <img src="docs/clipboard.png" width="820" alt="Tcode's Alt+V clipboard palette: a searchable list of copied entries with capture times and pin / delete actions">
</p>

## Frame — capture &amp; annotate

The titlebar camera grabs any window or region (via the desktop screenshot
portal), then hands it to **Frame** — a built-in annotation canvas where you draw
boxes, arrows, freehand pen, highlighter, and text in any color. **Save** exports
a PNG (also copied to your clipboard) and collects it in the screenshots strip
(toggle with `Alt+P`), ready to drag into a terminal.

A freshly captured shot also floats over the grid as a preview you can reposition
or dismiss before annotating.

<p align="center">
  <img src="docs/frame.png" width="820" alt="Frame annotating a captured screenshot — toolbar and color palette on top, boxes and arrows drawn on the grid, and the screenshots strip down the right edge">
</p>

## Keybindings

| Key | Action |
|-----|--------|
| `Alt+h/j/k/l` or `Alt`+arrow keys | Move focus between panes |
| `Alt+z` | Zoom the focused pane / restore |
| `Alt+n` | New terminal (add a pane) |
| `Alt+1` … `Alt+9` | Rebuild the grid with N panes |
| `Alt+b` | Toggle the file sidebar |
| `Alt+v` | Clipboard history palette |
| `Alt+p` | Screenshots strip |
| `Alt+f` | Toggle fullscreen (no titlebar) |
| `Alt+q` | Quit |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | Copy / paste in the focused terminal |
| `Ctrl +` / `Ctrl -` / `Ctrl 0` | Zoom the whole UI in / out / reset |

Ctrl+click a path or URL in any terminal to open it; right-click for Copy / Paste.

## Configuration

Optional `~/.config/tcode/config.toml` — every field has a default, so it's only
there if you want it:

```toml
font              = "Martian Mono"   # bundled; or any installed font
font_size         = 11
startup_command   = ""               # a command to run in every pane on open, e.g. "tmux"
clipboard_persist = false            # keep clipboard history across restarts
scale             = 1.0              # whole-UI zoom (0.5–3.0)
# [theme] background / foreground / accent / surface / border / palette (Tokyo Night by default)
```

PDF / office / screenshot features light up if you also have `poppler-utils`,
`libreoffice`, and `xdg-desktop-portal` installed.

## Build from source

Prefer to build it yourself?

```bash
sudo apt install -y build-essential pkg-config \
  libgtk-4-dev libvte-2.91-gtk4-dev libgtksourceview-5-dev
git clone https://github.com/moamen1358/Tcode && cd Tcode
./packaging/install.sh                         # build + install for your user
# or just: cargo build --release && ./target/release/tcode 4
```

**Run it three ways** — all built from the same version in `Cargo.toml`:

```bash
./run.sh native    # host binary (cargo build + run)
./run.sh docker    # container image  (tcode:<version>)
./run.sh deb       # build + install the .deb, then run
```

Maintainers: `./packaging/build-deb.sh` builds the `.deb`; pushing a `v*` tag
publishes it to Releases automatically (see `.github/workflows/release.yml`).

## License

[MIT](LICENSE) © 2026 moamen. Bundled assets keep their own licenses: the
**Martian Mono** font (SIL OFL) and **Tabler Icons** outline shapes (MIT).
