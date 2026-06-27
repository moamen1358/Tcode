# Tcode — project memory

Tcode is a fast, borderless **tiling-terminal workspace** for Linux: pick a
number → get that many terminal panes (plain login shells) in a balanced grid. It
has a file sidebar, a universal file viewer (code / images / PDFs / office / CSV),
a searchable **clipboard history** (Alt+V), saved sessions, and a built-in
screenshot annotator (**Frame**). Built in Rust with GTK4 + VTE.

## Repo facts
- GitHub `moamen1358/Tcode` — **PUBLIC** (made public 2026-06-27 so `tcode update` works
  for end users; was previously private). NOTE: pre-v1.3.0 screenshots in **git history**
  still show the `apollo-accounts-export.csv` filename in the sidebar — only the filename, not
  the data — so a history scrub is the remaining cleanup if that matters.
- Default branch `main`. Version lives in root `Cargo.toml` (`[workspace.package]`).
- Cargo workspace:
  - `crates/tcode-core` — pure logic (grid geometry, config, sessions), unit-tested, **no GTK**.
  - `crates/tcode` — the GTK4 app (binary `tcode`).

## Build · run · test · package
```bash
cargo build --release
cargo test --workspace
cargo clippy --workspace --all-targets   # kept warning-free
./target/release/tcode        # session picker
./target/release/tcode 4      # straight to a 2x2 grid
./packaging/build-deb.sh        # -> dist/tcode_<ver>_amd64.deb
./run.sh native|docker|deb [N]  # run it 3 ways, all versioned from Cargo.toml
```
- Release: `gh release create vX.Y.Z dist/*.deb` (or push a `v*` tag → CI in `.github/workflows/release.yml`).
- Self-update for installed users: `tcode update` (downloads the latest release `.deb`).
- Build-from-source install: `./packaging/install.sh`.
- **One runner — `run.sh`**: `./run.sh native|docker|deb [pane-count]`. native = host
  binary; docker = container image `tcode:<ver>` with Wayland/X11 forwarding (Dockerfile +
  `docker/tcode-profile.sh`); deb = build + install the package. **Version is single-sourced
  from `Cargo.toml`** — the binary (`env!("CARGO_PKG_VERSION")`, exposed via `tcode --version`),
  the `.deb`, and the Docker tag + OCI `image.version` label must always match. `tcode --version`
  prints before GTK init, so it's a headless smoke test for any mode.

## Conventions
- Conventional Commits (`feat:`/`fix:`/`docs:`/`chore:`/`perf:`), ending with the
  `Co-Authored-By` trailer.
- The user usually works on `main` and wants it pushed; bump the version + cut a
  release + rebuild the `.deb` when they say "update the deb / github".
- Keep the repo **lean — only the tool and the files it uses**: no sample/demo files, no
  redundant scripts. (Already removed: `samples/`, `package.json`, `run-docker.sh` (folded into
  `run.sh`), `docs/superpowers/`, `docs/BUILD_LOG.md`.) `target/`/`dist/` are build output —
  gitignored, regenerate on every build, so they reappear after building/testing; `cargo clean`
  + `rm -rf dist` to tidy.
- The repo + local folder are named **`Tcode`** (capital T): GitHub `moamen1358/Tcode`,
  folder `~/Desktop/Tcode`. The binary/command/package/Docker-image stay lowercase `tcode`;
  app_id is `dev.tcode.Tcode`.
- Environment is **COSMIC / Wayland**. Screenshot tooling that DOES work here:
  **`cosmic-screenshot --interactive=false --save-dir DIR`** (grabs ALL monitors —
  e.g. 7680×1600 for 3×2560 — so crop one with `ffmpeg -vf "crop=2560:H:X:0"`; no
  ImageMagick). **`wtype`** injects keys (`wtype -M alt v -m alt`, `wtype -k Escape`);
  **`wl-copy`** sets the clipboard; **Read the captured PNG to verify it**. `grim`
  fails (no `wlr-screencopy`); the Frame/portal capture needs interactive confirm.
  CAVEAT: multi-monitor → focus isn't guaranteed, so only blind-inject keys when the
  worst case is harmless (a menu opening); no mouse synthesis (no ydotool). Preview a
  build with `setsid ./target/release/tcode … &`. The Bash shell is **zsh** (no
  `mapfile`; unquoted `$var` doesn't word-split; foreground `sleep` is blocked).

## Code map
- `app.rs` — window, titlebar (logo + grouped controls + the gear "view settings"
  popover: font/scale steppers **plus a keyboard-shortcuts cheatsheet**), session
  open/reveal/build, a `Stack` of live sessions. `keys.rs` — Alt shortcuts.
  `session_picker.rs` — launch screens.
- `grid.rs` + `tcode-core/grid.rs` — **fixed** equal-split grid (nested homogeneous
  GTK `Box`es — every pane the same size, **not** draggable) + pure geometry.
- `pane.rs` — a VTE terminal pane (shell spawned only once sized; Ctrl+click links).
  Each pane has a 1px border (theme `border` color) so you can see the grid cells.
- `sidebar.rs` + `icons.rs` — file tree + file-type icons. `editor.rs` — tabbed viewer.
- `clipboard.rs` + `tcode-core/clipboard.rs` — clipboard-history model + the floating
  **Alt+V** command palette (search / copy / pin / delete; each entry keeps its capture
  time, persisted across restarts when `clipboard_persist` is on).
- `preview.rs` — PDF/office → page images on a worker thread.
- `frame.rs` + `frame/*` — capture (XDG portal) → annotate → save.
- `theme.rs` — global CSS (brand orange accent `#ff9e64`). `config.rs` — `~/.config/tcode/config.toml`.

## Keybindings
`Alt+h/j/k/l` **or** `Alt+arrows` move pane focus · `Alt+z` zoom pane · `Alt+n` new
pane · `Alt+1..9` rebuild grid · `Alt+b` sidebar · `Alt+v` clipboard palette · `Alt+p`
screenshots strip · `Alt+f` fullscreen · `Alt+q` quit · `Ctrl+Shift+C/V` copy/paste ·
`Ctrl +/-/0` UI zoom. (Also listed in-app via the titlebar gear popover.)

## Branding
- Logo: a hand-brushed orange **T** on transparent (raster PNG, so the icon set is
  PNG not SVG): `packaging/tcode.png` (master), `packaging/icons/tcode-{48,64,128,256}.png`,
  embedded titlebar logo `crates/tcode/assets/tcode.png`, and large on the
  session screens. Brand orange `#ff9e64` / logo `#F2660C`.

## Open items / gotchas
- **Terminal resize / reflow**: VTE rewrap-on-resize is **disabled**
  (`vte_terminal_set_rewrap_on_resize(false)` via the `vte4::ffi`, since the safe
  bindings dropped it) — leaving it on made a prompt with right-margin content (a
  right-aligned clock) reprint and stack on every resize, in the worst case filling
  the pane (see powerlevel10k#1200). On a window resize the terminals are also frozen
  (hidden) for the drag and revealed once it settles, so the child sees one SIGWINCH,
  not a burst. Drive that freeze **only** from the size-guarded toplevel-surface
  `layout` hook — never a Paned `position_notify`, which feeds back into a loop.
- **The side panels are width-capped** so they can't squeeze the terminals: the file
  viewer ≤ `VIEWER_MAX_WIDTH` (800px), the sidebar ≤ `SIDEBAR_MAX_WIDTH` (400px),
  enforced on every divider change (incl. a width restored from a saved session); the
  grid keeps a `MIN_TERMINAL_WIDTH` (480px) floor. All in `app.rs`.
- **Desktop file basename must equal the GTK `APP_ID`** (`dev.tcode.Tcode` in
  `main.rs`): the installed entry is `dev.tcode.Tcode.desktop`, not
  `tcode.desktop`. On Wayland the compositor maps a window to its launcher
  entry by `app_id == desktop-basename`; a mismatch means the running window/dock
  shows **no icon**. Keep `StartupWMClass=dev.tcode.Tcode` for X11.
- **Distribution**: now that the repo is **public**, `tcode update` (unauthenticated
  GitHub API) resolves the latest release for end users — no more 404, no separate
  "releases" repo needed. (The README no longer embeds a demo video — removed at the
  user's request; `Tcode-Demo.mp4` may still be attached to the v1.3.0 release.)
- **Multi-agent direction (built then fully removed)**: a "Conductor / Mission Control"
  experiment — auto-launching Claude/Codex/Hermes in panes, a per-session coordination
  bus, and an Alt+M activity board — lived on `feat/conductor`, then was **reverted in
  full** at the user's request (commit `90b7ded`). Panes are plain shells again. Those
  commits stay in history; **don't resurrect them**. v1.3.0 is the clean terminal
  workspace + the clipboard capture-time feature.
- `apollo-accounts-export.csv` at the repo root is the user's private business
  data (gitignored) — never commit it, and keep it out of any public screenshot.
  (The pre-v1.3.0 `docs/screenshot.png` + `docs/frame.png` leaked its name in the
  sidebar; replaced with clean demo-folder shots — but the old ones remain in git
  history, so scrub history before the repo ever goes public.)
- `logos/` holds logo experiments (gitignored).
