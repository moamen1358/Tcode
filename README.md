# Tessera

A minimal, fast, **borderless tiling-terminal workspace** for Linux. Pick a number →
get that many terminal panes in a balanced grid. Keyboard-driven like neovim.
Built in Rust with GTK4 + VTE.

## Features

- Minimal dark, GPU-composited GUI: a thin titlebar (minimize / maximize / close), and
  `Alt+f` for immersive fullscreen with no header at all.
- Pick **1–16 panes** (a picker on launch, or `tessera 4`); balanced auto-grid.
- Each pane is your `$SHELL` (with an optional auto-run startup command).
- **File sidebar** (left): click a folder to `cd` the focused pane into it; click a
  file to insert its path. `..` goes up. Toggle with the **☰** titlebar button or `Alt+b`.
- **Drag-and-drop** a file/image onto the grid → its path is inserted into the
  focused pane (handy for passing an image to a CLI agent).
- **Zoom** the focused pane to fullscreen and back.
- Fully keyboard-driven; click also works.

## Keybindings

| Key            | Action                              |
|----------------|-------------------------------------|
| `Alt+h/j/k/l`  | Move focus between panes            |
| `Alt+z`        | Zoom the focused pane / restore     |
| `Alt+r`        | Restart the focused pane            |
| `Alt+b`        | Toggle the file sidebar             |
| `Alt+f`        | Toggle fullscreen (no titlebar)     |
| `Alt+1`..`Alt+9` | Rebuild the grid with N panes     |
| `Alt+q`        | Quit                                |

## Build & run (native)

System dependencies (Ubuntu / Pop!_OS 24.04):

```bash
sudo apt install -y build-essential libgtk-4-dev libvte-2.91-gtk4-dev pkg-config
```

Rust toolchain (if you don't have it): <https://rustup.rs>

```bash
cargo build --release
./target/release/tessera        # opens the picker
./target/release/tessera 4      # straight to a 2x2 grid
cargo install --path crates/tessera   # optional: put `tessera` on your PATH
```

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
font            = "monospace"   # generic = your system mono; or any installed font name
font_size       = 11
gap             = 8
startup_command = ""        # e.g. "claude" to auto-launch in every pane

[theme]
background = "#1e1e2e"
foreground = "#cdd6f4"
accent     = "#89b4fa"      # active-pane border
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
