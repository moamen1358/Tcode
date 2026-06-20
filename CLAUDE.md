# Tessera — project memory

Tessera is a fast, borderless **tiling-terminal workspace** for Linux: pick a
number → get that many terminal panes in a balanced grid. It has a file sidebar,
a universal file viewer (code / images / PDFs / office / CSV), saved sessions,
and a built-in screenshot annotator (**Frame**). Built in Rust with GTK4 + VTE.

## Repo facts
- GitHub `moamen1358/tessera` — **PRIVATE** (source stays private by choice).
- Default branch `main`. Version lives in root `Cargo.toml` (`[workspace.package]`).
- Cargo workspace:
  - `crates/tessera-core` — pure logic (grid geometry, config, sessions), unit-tested, **no GTK**.
  - `crates/tessera` — the GTK4 app (binary `tessera`).

## Build · run · test · package
```bash
cargo build --release
cargo test --workspace
cargo clippy --workspace --all-targets   # kept warning-free
./target/release/tessera        # session picker
./target/release/tessera 4      # straight to a 2x2 grid
./packaging/build-deb.sh        # -> dist/tessera_<ver>_amd64.deb
```
- Release: `gh release create vX.Y.Z dist/*.deb` (or push a `v*` tag → CI in `.github/workflows/release.yml`).
- Self-update for installed users: `tessera update` (downloads the latest release `.deb`).
- Build-from-source install: `./packaging/install.sh`.

## Conventions
- Conventional Commits (`feat:`/`fix:`/`docs:`/`chore:`/`perf:`), ending with the
  `Co-Authored-By` trailer.
- The user usually works on `main` and wants it pushed; bump the version + cut a
  release + rebuild the `.deb` when they say "update the deb / github".
- Environment is **COSMIC / Wayland**: automated screenshots don't work (grim's
  protocol is unsupported; the portal needs interactive confirmation). Preview by
  launching the build (`setsid ./target/release/tessera … &`); the user captures
  screenshots when needed. The Bash shell here is **zsh** (no `mapfile`; unquoted
  `$var` doesn't word-split; foreground `sleep` is blocked).

## Code map
- `app.rs` — window, titlebar (logo + grouped controls), session open/reveal/build,
  a `Stack` of live sessions. `keys.rs` — Alt shortcuts. `session_picker.rs` — launch screens.
- `grid.rs` + `tessera-core/grid.rs` — tiling grid (nested GTK `Paned`) + pure geometry.
- `pane.rs` — a VTE terminal pane (shell spawned only once sized; Ctrl+click links).
- `sidebar.rs` + `icons.rs` — file tree + file-type icons. `editor.rs` — tabbed viewer.
- `preview.rs` — PDF/office → page images on a worker thread.
- `frame.rs` + `frame/*` — capture (XDG portal) → annotate → save.
- `theme.rs` — global CSS (brand orange accent `#ff9e64`). `config.rs` — `~/.config/tessera/config.toml`.

## Keybindings
`Alt+h/j/k/l` **or** `Alt+arrows` move pane focus · `Alt+z` zoom pane · `Alt+n` new
pane · `Alt+1..9` rebuild grid · `Alt+b` sidebar · `Alt+p` screenshots strip ·
`Alt+f` fullscreen · `Alt+q` quit · `Ctrl+Shift+C/V` copy/paste · `Ctrl +/-/0` UI zoom.

## Branding
- Logo: a hand-brushed orange **T** on transparent (raster PNG, so the icon set is
  PNG not SVG): `packaging/tessera.png` (master), `packaging/icons/tessera-{48,64,128,256}.png`,
  embedded titlebar logo `crates/tessera/assets/tessera.png`, and large on the
  session screens. Brand orange `#ff9e64` / logo `#F2660C`.

## Open items / gotchas
- **Desktop file basename must equal the GTK `APP_ID`** (`dev.tessera.Tessera` in
  `main.rs`): the installed entry is `dev.tessera.Tessera.desktop`, not
  `tessera.desktop`. On Wayland the compositor maps a window to its launcher
  entry by `app_id == desktop-basename`; a mismatch means the running window/dock
  shows **no icon**. Keep `StartupWMClass=dev.tessera.Tessera` for X11.
- **Distribution**: the repo is private, so `tessera update` (unauthenticated
  GitHub API) returns 404 for end users. Making updates work publicly without
  exposing source needs a separate **public "releases" repo**. Deferred by the user.
- `apollo-accounts-export.csv` at the repo root is the user's private business
  data (gitignored) — never commit it, and keep it out of any public screenshot.
- `logos/` holds logo experiments (gitignored).
