<div align="center">

<img src="packaging/tessera.svg" width="120" alt="Tessera logo">

# Tessera

**A fast, borderless tiling-terminal workspace for Linux.**

Pick a number — get that many terminal panes in a balanced grid. Keyboard-driven
like vim, GPU-composited, with a built-in file tree and a universal file viewer.
Built in Rust with GTK4 + VTE.

![License](https://img.shields.io/badge/license-MIT-F2660C)
![Platform](https://img.shields.io/badge/platform-Linux-F2660C?logo=linux&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.83%2B-F2660C?logo=rust&logoColor=white)
![GTK](https://img.shields.io/badge/GTK-4.14-F2660C)

</div>

<p align="center">
  <img src="docs/screenshot.png" width="820" alt="Tessera screenshot: a 2x2 terminal grid with a file sidebar and a tabbed editor">
</p>

---

## Contents

- [Features](#features)
- [Install](#install)
- [Updating](#updating)
- [Usage](#usage)
- [Keybindings](#keybindings)
- [Configuration](#configuration)
- [Optional runtime tools](#optional-runtime-tools)
- [Run in Docker](#run-in-docker)
- [Project layout](#project-layout)
- [Building &amp; contributing](#building--contributing)
- [License](#license)

## Features

- **Instant tiling.** Pick **1–16 panes** (a picker on launch, or `tessera 4`) and
  Tessera arranges them in a balanced auto-grid. Drag any border to resize.
- **Borderless &amp; GPU-composited.** A thin titlebar (minimize / maximize / close),
  with `Alt+f` for an immersive, header-less fullscreen.
- **Keyboard-driven.** vim-style focus movement, pane zoom, grid rebuild, and
  sidebar / fullscreen toggles — all from the home row.
- **Live panes.** The **`+`** button (or `Alt+n`) adds a terminal; when a shell
  exits its pane disappears and the rest re-tile (exit the last → the app closes).
  Each pane runs your `$SHELL`, with an optional auto-run startup command.
- **File sidebar.** A VS Code-style tree on the left — click a folder to expand,
  click a file to open it in the viewer. Toggle with `Alt+b`.
- **Universal file viewer.** A tabbed panel that opens code/text in an editable,
  syntax-highlighted editor (`Ctrl+S` saves), images on a zoom/pan canvas, PDFs &amp;
  office docs as scrollable zoomable pages, and CSV/TSV as a rainbow table.
- **Screenshot &amp; annotate.** The titlebar camera captures any window/region via
  the desktop portal, opens an annotation canvas (box / arrow / text / pen /
  highlight), and saves to a strip you can drag into a terminal.
- **Smart clicks.** **Ctrl+click** a path or URL in any terminal to open it (files
  in the viewer, URLs in the browser); **right-click** for Copy / Paste / Select All.
- **Drag-and-drop.** Drop a file onto a pane → its shell-quoted path is inserted
  into that terminal — handy for handing a file to a CLI agent.
- **Sessions.** Named workspaces remember their folder, pane layout, split sizes,
  and open files, and reopen exactly where you left off. Switch from the titlebar;
  background sessions keep running.
- **Self-updating.** `tessera update` pulls the latest version and reinstalls.

## Install

System dependencies (Ubuntu / Pop!\_OS 24.04):

```bash
sudo apt install -y build-essential libgtk-4-dev libvte-2.91-gtk4-dev \
  libgtksourceview-5-dev pkg-config
```

Rust toolchain (if you don't have it): <https://rustup.rs> — MSRV **1.83**.

### As a desktop app (recommended)

```bash
git clone https://github.com/moamen1358/tessera
cd tessera
./packaging/install.sh
```

This `cargo install`s the `tessera` binary, drops a `.desktop` launcher + the
orange icon into `~/.local/share`, and records the source path for updates.
**Tessera** then appears in your application menu (or just run `tessera`).

### Or build &amp; run directly

```bash
cargo build --release
./target/release/tessera        # opens the session picker
./target/release/tessera 4      # straight to a 2x2 grid
```

## Updating

```bash
tessera update
```

Pulls the newest source (`git pull --ff-only`), rebuilds, and reinstalls — using
the source path the installer recorded under `~/.local/share/tessera/source`. You
can also run `./packaging/update.sh` from the clone.

## Usage

```bash
tessera            # session picker
tessera 4          # open a 2x2 grid in the current directory
tessera --help     # all commands
tessera --version
```

`TESSERA_RESUME=<id> tessera` opens a saved session directly (handy for scripting
a launch straight into a known session).

## Keybindings

| Key | Action |
|-----|--------|
| `Alt+h` / `j` / `k` / `l` | Move focus between panes |
| `Alt+z` | Zoom the focused pane / restore |
| `Alt+n` | New terminal (add a pane) |
| `Alt+b` | Toggle the file sidebar |
| `Alt+f` | Toggle fullscreen (no titlebar) |
| `Alt+1` … `Alt+9` | Rebuild the grid with N panes |
| `Alt+p` | Toggle the screenshots strip |
| `Alt+q` | Quit |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | Copy / paste in the focused terminal |

## Configuration

Optional `~/.config/tessera/config.toml` — every field has a default, so it works
with no config at all:

```toml
font            = "Martian Mono"   # ships bundled; or any installed font name
font_size       = 11
startup_command = ""               # e.g. "claude" to auto-launch in every pane
clipboard_persist = false          # opt in to saving clipboard history on disk
scale           = 1.0              # whole-UI zoom (0.5–3.0)

[theme]                            # defaults are Tokyo Night
background = "#1a1b26"
foreground = "#c0caf5"
accent     = "#7aa2f7"             # active-pane border
surface    = "#16161e"             # sidebar / tab-bar / titlebar
border     = "#2f3549"
# palette  = [ ... 16 ANSI hex colors ... ]
```

## Optional runtime tools

The file viewer and screenshots degrade gracefully if these are missing:

```bash
sudo apt install -y poppler-utils libreoffice xdg-desktop-portal
```

- `poppler-utils` (`pdftoppm`) — render **PDF** previews
- `libreoffice` (`soffice`) — render **office** docs (docx / xlsx / pptx / odt …)
- `xdg-desktop-portal` (+ a backend, e.g. `…-gnome` or `…-cosmic`) — **screenshot** capture

## Run in Docker

```bash
./run-docker.sh 4
```

Builds the image on first run, then launches Tessera **inside the container** with
your host Wayland socket mounted (X11 fallback). Your current directory is mounted
at `/work`, so the panes operate on your real files.

## Project layout

```
crates/tessera-core   pure logic (grid geometry, config, sessions) — unit-tested, no GTK
crates/tessera        the GTK4 app: window, panes, grid, picker, keys, sidebar, viewer, frame
packaging/            install / update scripts, .desktop entry, app icon (SVG + PNG)
docker/               container profile
docs/                 design notes + build log
```

## Building &amp; contributing

```bash
cargo build                              # debug build
cargo test --workspace                   # unit tests (pure-logic crate)
cargo clippy --workspace --all-targets   # lints — kept warning-free
cargo build --release                    # optimized binary
```

Issues and PRs welcome. Please keep `cargo clippy` warning-free and `cargo test`
green.

**Tech:** Rust · `gtk4` 0.11 (`v4_14`) · `vte4` 0.10 (`v0_76`) · `sourceview5` ·
system GTK 4.14 / VTE 0.76 · Wayland (X11 fallback).

## License

[MIT](LICENSE) © 2026 moamen.

Bundled assets keep their own licenses: the **Martian Mono** font (SIL Open Font
License — `crates/tessera/assets/fonts/OFL.txt`) and the **Material Icon Theme**
shapes used for file-type icons (MIT — `crates/tessera/assets/icons/ATTRIBUTION.txt`).
