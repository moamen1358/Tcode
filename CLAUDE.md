# Tcode — project memory

Tcode is a fast, borderless **tiling-terminal workspace** for Linux: pick a
number → get that many terminal panes in a balanced grid. It has a file sidebar,
a universal file viewer (code / images / PDFs / office / CSV), saved sessions,
and a built-in screenshot annotator (**Frame**). Built in Rust with GTK4 + VTE.

## Repo facts
- GitHub `moamen1358/Tcode` — **PRIVATE** (source stays private by choice).
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
- Environment is **COSMIC / Wayland**: automated screenshots don't work (grim's
  protocol is unsupported; the portal needs interactive confirmation). Preview by
  launching the build (`setsid ./target/release/tcode … &`); the user captures
  screenshots when needed. The Bash shell here is **zsh** (no `mapfile`; unquoted
  `$var` doesn't word-split; foreground `sleep` is blocked).

## Code map
- `app.rs` — window, titlebar (logo + grouped controls), session open/reveal/build,
  a `Stack` of live sessions. `keys.rs` — Alt shortcuts. `session_picker.rs` — launch screens.
- `grid.rs` + `tcode-core/grid.rs` — **fixed** equal-split grid (nested homogeneous
  GTK `Box`es — every pane the same size, **not** draggable) + pure geometry.
- `pane.rs` — a VTE terminal pane (shell spawned only once sized; Ctrl+click links).
  Each pane has a 1px border (theme `border` color) so you can see the grid cells.
- `sidebar.rs` + `icons.rs` — file tree + file-type icons. `editor.rs` — tabbed viewer.
- `preview.rs` — PDF/office → page images on a worker thread.
- `frame.rs` + `frame/*` — capture (XDG portal) → annotate → save.
- `theme.rs` — global CSS (brand orange accent `#ff9e64`). `config.rs` — `~/.config/tcode/config.toml`.

## Keybindings
`Alt+h/j/k/l` **or** `Alt+arrows` move pane focus · `Alt+z` zoom pane · `Alt+n` new
pane · `Alt+1..9` rebuild grid · `Alt+b` sidebar · `Alt+p` screenshots strip ·
`Alt+f` fullscreen · `Alt+q` quit · `Ctrl+Shift+C/V` copy/paste · `Ctrl +/-/0` UI zoom.

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
- **Distribution**: the repo is private, so `tcode update` (unauthenticated
  GitHub API) returns 404 for end users. Making updates work publicly without
  exposing source needs a separate **public "releases" repo**. Deferred by the user.
- `apollo-accounts-export.csv` at the repo root is the user's private business
  data (gitignored) — never commit it, and keep it out of any public screenshot.
- `logos/` holds logo experiments (gitignored).
