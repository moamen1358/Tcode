# Tessera

A minimal, fast, **borderless tiling-terminal workspace** for Linux. Pick a number →
get that many terminal panes in a balanced grid. Keyboard-driven like neovim.
Built in Rust with GTK4 + VTE.

## Features

- Minimal dark, GPU-composited GUI: a thin titlebar (minimize / maximize / close), and
  `Alt+f` for immersive fullscreen with no header at all.
- Pick **1–16 panes** (a picker on launch, or `tessera 4`); balanced auto-grid.
- **Add / remove panes live**: the **`+`** button (or `Alt+n`) adds a terminal; when a
  shell exits, its pane disappears and the rest re-tile (exit the last → the app closes).
- Each pane is your `$SHELL` (with an optional auto-run startup command).
- **File sidebar** (left): a VS Code-style tree; click a folder to expand it, click
  a file to open it. Toggle with the **sidebar button** in the titlebar or `Alt+b`.
- **Universal file viewer** (right): a tabbed panel that opens code/text in an
  editable, syntax-highlighted editor (`Ctrl+S` saves, `Esc` closes the tab),
  images on a zoom/pan canvas, PDFs and office docs as scrollable zoomable pages,
  and CSV/TSV as a "rainbow" table.
- **BridgeShot** screenshots: the titlebar camera captures any window/region via
  the desktop portal, opens an annotation canvas (box / arrow / text / pen /
  highlight), and saves to a strip at the bottom of the sidebar you can drag into
  a terminal.
- **Ctrl+click** a path or URL in any terminal to open it (files in the viewer,
  URLs in the browser); **right-click** for Copy / Paste / Select All.
- **Drag-and-drop** a file/image onto the grid → its path is inserted into the
  focused pane (handy for passing an image to a CLI agent).
- **Zoom** the focused pane to fullscreen and back. Fully keyboard-driven.

## Keybindings

| Key            | Action                              |
|----------------|-------------------------------------|
| `Alt+h/j/k/l`  | Move focus between panes            |
| `Alt+z`        | Zoom the focused pane / restore     |
| `Alt+n`        | New terminal (add a pane)           |
| `Alt+b`        | Toggle the file sidebar             |
| `Alt+f`        | Toggle fullscreen (no titlebar)     |
| `Alt+1`..`Alt+9` | Rebuild the grid with N panes     |
| `Alt+p`        | Toggle the screenshots strip        |
| `Alt+q`        | Quit                                |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | Copy / paste in the focused terminal |

## Build & run (native)

System dependencies (Ubuntu / Pop!_OS 24.04):

```bash
sudo apt install -y build-essential libgtk-4-dev libvte-2.91-gtk4-dev \
  libgtksourceview-5-dev pkg-config
```

Rust toolchain (if you don't have it): <https://rustup.rs>

```bash
cargo build --release
./target/release/tessera        # opens the picker
./target/release/tessera 4      # straight to a 2x2 grid
cargo install --path crates/tessera   # optional: put `tessera` on your PATH
```

Optional runtime tools — the file viewer and screenshots degrade gracefully if
these are missing:

```bash
sudo apt install -y poppler-utils libreoffice xdg-desktop-portal
```

- `poppler-utils` (`pdftoppm`) — render **PDF** previews
- `libreoffice` (`soffice`) — render **office** docs (docx / xlsx / pptx / odt …)
- `xdg-desktop-portal` (+ a backend, e.g. `xdg-desktop-portal-gnome` or
  `…-cosmic`) — **screenshot** capture

## Install as a desktop app

```bash
./packaging/install.sh
```

Builds and `cargo install`s the binary, then drops a `.desktop` launcher + icon
into `~/.local/share`, so **Tessera** appears in your application menu. No root.

## Run in Docker (display forwarding)

```bash
./run-docker.sh 4
```

Builds the image on first run, then launches the app **inside the container** with
your host **Wayland** socket mounted (X11 fallback). Your current directory is
mounted at `/work`, so the panes operate on your real files.

## Config

Optional `~/.config/tessera/config.toml` — every field has a default, so it works
with no config at all:

```toml
font            = "Martian Mono"   # ships bundled; or any installed font name
font_size       = 11
startup_command = ""               # e.g. "claude" to auto-launch in every pane

[theme]                            # defaults are Tokyo Night
background = "#1a1b26"
foreground = "#c0caf5"
accent     = "#7aa2f7"             # active-pane border
# palette  = [ ... 16 ANSI hex colors ... ]
```

## Project layout

```
crates/tessera-core   pure logic (grid geometry + config), unit-tested, no GTK
crates/tessera        the GTK4 app: window, panes, grid, picker, keys, sidebar
docs/                 design spec, implementation plan, build log
Dockerfile, run-docker.sh   containerized run with display forwarding
```

## Tech

Rust · `gtk4` 0.11 (`v4_14`) · `vte4` 0.10 (`v0_76`) · system GTK 4.14 / VTE 0.76 · Wayland.
