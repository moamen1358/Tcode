# Tessera — Minimal Tiling Terminal Workspace

**Status:** Approved design (2026-06-18)
**Author:** Moamen + Claude
**Inspiration:** A stripped-down, minimal clone of BridgeMind's *BridgeSpace* — pick a number, get that many terminals in a grid. No chrome, keyboard-driven, fast.

---

## 1. Summary

Tessera is a **borderless, keyboard-driven Linux GUI app** that opens *N* terminal panes
in an auto-tiled grid. You pick the number on launch; it tiles that many real shells.
No titlebar, no menus, no toolbars — "just a working space," like neovim splits.

Built in **Rust** with **GTK4** + **VTE** (the proven terminal-emulator widget behind
GNOME Terminal / Tilix). Reusing VTE means we build the *workspace*, not a terminal from
scratch — that's what makes it both **fast at runtime** and **fast to ship**.

## 2. Goals / Non-goals

**Goals**
- Pick *N* → instantly get *N* tiled terminal panes.
- Minimal, beautiful, dark UI. Borderless. Subtle gaps + rounded corners + active-pane highlight.
- Fast: native machine code, GPU-composited, instant startup.
- Keyboard-driven (neovim-style): move focus, zoom a pane, re-grid, quit — no mouse needed.
- Each pane is the user's `$SHELL`, with an *optional* startup command auto-run on open.
- Zero-config: runs beautifully with no config file; everything is overridable.

**Non-goals (v1 — may add later)**
- ❌ Drag-to-resize panes (equal auto-grid + zoom covers this).
- ❌ AI-agent config / orchestration (just a shell + optional startup command).
- ❌ Tabs, settings GUI, session save/restore, scrollback-search UI.
- ❌ Cross-platform (Linux-only v1; GTK4 keeps a Mac/Windows door open later).

## 3. Target environment

- **OS:** Pop!_OS 24.04 LTS (Ubuntu `noble` base), **Wayland** session (GTK4 native).
- **System libs:** GTK **4.14.5** (`libgtk-4-dev`), VTE **0.76.0** (`libvte-2.91-gtk4-dev`), `build-essential`, `pkg-config`.
- **Toolchain:** Rust stable via `rustup`.

## 4. User experience / flow

1. **Launch** → a clean, minimal **picker** appears: large number buttons `1 2 3 4 6 8 9`
   (press the digit key or click).
2. Pick a number → picker disappears, the **grid of that many terminals** fills the screen.
3. Each pane runs `$SHELL` in the directory Tessera was launched from. If `startup_command`
   is set in config, every pane auto-runs it first (e.g. `claude`).
4. **Power option:** `tessera 4` launches straight into 4 panes, skipping the picker.
   Range 1–16 via arg; picker/`Alt+digit` cover 1–9.

## 5. Keybindings (neovim-style, Alt-based to avoid clobbering shell `Ctrl` keys)

| Key            | Action                                            |
|----------------|---------------------------------------------------|
| `Alt+h/j/k/l`  | Move focus left / down / up / right between panes |
| `Alt+z`        | Toggle zoom: focused pane fills window / back to grid |
| `Alt+r`        | Restart the focused pane (re-spawn shell/startup cmd) |
| `Alt+1`..`Alt+9` | Rebuild the grid with that many panes           |
| `Alt+f`        | Toggle fullscreen (immersive, no titlebar)        |
| `Alt+q`        | Quit                                              |

> `Alt+digit` rebuilds the grid from scratch — the current panes and their running
> processes are closed (SIGHUP). It is a fresh layout, not a resize of existing panes.

## 6. Architecture — Cargo workspace (two crates for isolation/testability)

```
coding_Space/
├─ Cargo.toml                 # [workspace]
├─ crates/
│  ├─ tessera-core/           # PURE logic, NO GTK deps — unit tested without a display
│  │  └─ src/lib.rs           #   - grid::layout(n) -> Vec<usize> (panes per row)
│  │                          #   - grid::neighbor(...) focus navigation math
│  │                          #   - config::Config (serde) + load_or_default()
│  └─ tessera/                # the GTK4 binary
│     └─ src/
│        ├─ main.rs           # GTK Application bootstrap, CLI arg parse
│        ├─ app.rs            # window (borderless), CSS load, state, re-grid
│        ├─ picker.rs         # the number-picker screen
│        ├─ grid.rs           # build nested homogeneous Boxes from layout(n); zoom
│        ├─ pane.rs           # one VTE Terminal: spawn shell+cmd, colors, exit overlay
│        └─ keys.rs           # EventControllerKey -> actions
```

**Why a workspace:** `tessera-core` has no GTK dependency, so its logic (the grid math and
config parsing — the only parts with real branching) is unit-tested with plain `cargo test`,
even on a machine without GTK installed. The `tessera` crate is the thin GTK wiring layer,
verified by building + running.

## 7. Grid geometry (balanced, no empty cells)

A pure function `layout(n) -> Vec<usize>` returning panes-per-row:

```
rows  = max(1, round(sqrt(n)))
base  = n / rows            # integer division
extra = n % rows
# first `extra` rows get (base+1) panes, the rest get `base` panes
```

Rendered as a **vertical `gtk4::Box` of horizontal `gtk4::Box`es**, all `homogeneous`,
with `spacing = gap`. Every pane fills its row; rows fill the window. No empty cells, no spans.

| N | layout      | shape           |
|---|-------------|-----------------|
| 1 | `[1]`       | full window     |
| 2 | `[2]`       | side-by-side    |
| 3 | `[2,1]`     | 2 over 1 (full) |
| 4 | `[2,2]`     | 2×2             |
| 5 | `[3,2]`     | 3 over 2        |
| 6 | `[3,3]`     | 2×3             |
| 9 | `[3,3,3]`   | 3×3             |

**Focus navigation:** track `(row, col)`. `h/l` move within the row (clamped); `j/k` move
between rows, clamping `col` to the target row's width. Pure math in `tessera-core`, tested.

**Zoom (no reparenting):** to zoom the focused pane, `set_visible(false)` on every *other*
row and every *other* pane in the focused row — hidden `Box` children take zero space in
GTK4, so the focused pane expands to fill. Unzoom restores visibility. Simple and robust.

## 8. Pane / terminal (vte4)

- Wrap a `vte4::Terminal` in a styled container (`gtk4::Overlay` for the exit message).
- `spawn_async` a login shell (`$SHELL`, fallback `/bin/sh`) in the launch CWD, env inherited.
- If `startup_command` is set, spawn via the shell so it runs then drops to an interactive
  prompt: `sh -c '<startup_command>; exec $SHELL'` (exact form finalized against the vte4 API).
- Colors (bg / fg / 16-color palette / accent) set via the **vte4 API** (`set_colors`,
  `set_color_background`, …) using `gdk::RGBA` — **not** CSS (CSS can't reach VTE's cell colors).
- `connect_child_exited` → overlay a dim `[exited — Alt+r to restart]` label; `Alt+r` re-spawns.

> ⚠️ **Verification risk:** the exact `vte4::Terminal::spawn_async` argument list is
> version-sensitive. Pinned crate versions and this signature are verified by the
> `verify-gtk4-vte4-api` research workflow **and** confirmed by the compiler before relying on it.

## 9. Styling / theme (the "beautiful" part)

- Window chrome: a minimal client-side `HeaderBar` titlebar (minimize/maximize/close) so the
  window can always be closed and restored — COSMIC double-decorates a `decorated(false)`
  window, so CSD is the reliable approach. `Alt+f` toggles immersive fullscreen (GTK hides
  the titlebar) for a true no-header workspace; `Alt+q` quits.
- GTK4 CSS via `CssProvider` + `add_provider_for_display`. Style classes toggled with
  `add_css_class("active")` on the focused pane's container.
- Default look: dark palette, `gap = 8px`, `border-radius = 8px`, ~6px inner padding,
  active pane gets a thin accent border; inactive panes very slightly dimmed.
- Default font: `JetBrains Mono` → `Fira Code` → `monospace`, size 11.
- Default palette (overridable): Catppuccin-Mocha-like — bg `#1e1e2e`, fg `#cdd6f4`,
  accent `#89b4fa`, plus a 16-color ANSI palette.

## 10. Config (`~/.config/tessera/config.toml`, all optional)

```toml
font            = "JetBrains Mono"
font_size       = 11
gap             = 8
startup_command = ""          # empty = just the shell

[theme]
background = "#1e1e2e"
foreground = "#cdd6f4"
accent     = "#89b4fa"        # active-pane border
palette    = ["#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
              "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
              "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af",
              "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8"]
```

`config::load_or_default()` returns defaults when the file is missing/partial. Parsing +
defaulting is unit-tested in `tessera-core`.

## 11. Error handling

- **Shell/command exits:** pane shows dim `[exited — Alt+r to restart]`; `Alt+r` re-spawns.
  No auto-respawn loop (a failing command won't thrash).
- **Bad startup command:** runs through the shell, so the shell prints the normal error;
  the pane stays usable.
- **Malformed config:** log a warning to stderr, fall back to defaults, keep running.
- **Window close / quit:** child processes get `SIGHUP` via PTY teardown; no orphans.

## 12. Testing strategy

- **Unit tests (`tessera-core`):** `grid::layout` for N=1..16 (shape + pane count == N),
  `grid::neighbor` focus navigation (clamping at edges, jagged last row), `config` parse +
  defaults + partial/overlapping fields. No display required.
- **Build gate:** `cargo build` + `cargo clippy -- -D warnings` for the whole workspace.
- **Manual / run verification:** launch the app, confirm picker → grid, typing works,
  `Alt+hjkl` moves focus, `Alt+z` zooms, `Alt+digit` re-grids, `Alt+q` quits. Done via the
  `run` tooling once built (it's a GUI — no headless integration test in v1).

## 13. Build & run

```bash
# one-time system deps (sudo):
sudo apt install -y build-essential libgtk-4-dev libvte-2.91-gtk4-dev pkg-config
# toolchain: rustup (user-local)

cargo build --release            # workspace
cargo test -p tessera-core       # pure-logic tests
./target/release/tessera 4       # 4 panes, skip picker
./target/release/tessera         # picker
# optional: cargo install --path crates/tessera   -> `tessera` on PATH
```

## 14. Open verification items (closed before/at implementation)

1. Exact compatible crate versions: `gtk4`, `vte4`, `glib`/`gio` for GTK 4.14 / VTE 0.76.
2. Exact `vte4::Terminal::spawn_async` signature + child-exited handling.
3. Confirm hidden GTK4 `Box` children consume zero space (zoom approach).
4. `EventControllerKey` closure signature + ALT modifier matching + propagation.

All four are researched by the `verify-gtk4-vte4-api` workflow and **confirmed by the
Rust compiler** before any code depends on them.

---

## 15. v1.1 additions (approved 2026-06-18)

Extensions added after the base grid was verified working. APIs verified by the
`verify-gtk4-feature-apis` workflow + compiler.

### 15.1 File-tree sidebar (left, VS Code-style)
- Collapsible panel on the left, toggled with **`Alt+b`**. Rooted at the launch directory.
- Expandable tree: `gtk4::DirectoryList` + `TreeListModel` + `ListView` + `TreeExpander`.
- Click a **folder** → `cd` the focused pane into it (`feed_child("cd <path>\n")`) + expand.
- Click a **file** → insert its path into the focused pane (`feed_child`, no newline).
- Window layout becomes `HBox [ sidebar | grid ]`; sidebar has a default width, hideable.

### 15.2 Image drag-and-drop
- A `gtk4::DropTarget` (accepting `gdk::FileList`) on the grid.
- On drop, the file's path is inserted into the focused pane (`feed_child`, no newline) —
  so an image can be passed to a CLI agent.

### 15.3 Build-command logging
- Every meaningful build/setup command is recorded in `docs/BUILD_LOG.md` — the
  source of truth for the Dockerfile.

### 15.4 Dockerized run (display forwarding)
- Multi-stage `Dockerfile` (Ubuntu 24.04): builder compiles the release binary; the
  runtime image carries only GTK/VTE runtime libs + the binary.
- `run-docker.sh` launches the container with the host **Wayland** socket mounted
  (X11 fallback) so the GUI renders on the host display.

### 15.5 New keybinding
- **`Alt+b`** → toggle the file-tree sidebar (added to the §5 set).
