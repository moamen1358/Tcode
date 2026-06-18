# Tessera — Build Log

Every meaningful command run to set up and build this project, in order.
This is the **source of truth for the Dockerfile** — if it's needed to build/run
Tessera, it's recorded here.

**Host:** Pop!_OS 24.04 LTS (Ubuntu `noble` base), Wayland session.

---

## 1. System dependencies (apt — needs root)

```bash
sudo apt install -y build-essential libgtk-4-dev libvte-2.91-gtk4-dev \
    libgtksourceview-5-dev pkg-config
```

Provides: GTK **4.14.5** (`gtk4.pc`), VTE **0.76.0** (`vte-2.91-gtk4.pc`),
GtkSourceView **5.12** (`gtksourceview-5.pc`, for the neovim-style editor),
C toolchain, pkg-config.

## 2. Rust toolchain (rustup — user-local, no root)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile default
. "$HOME/.cargo/env"
```

Result: `rustc`/`cargo` **1.96.0** (gtk4 0.11 needs MSRV ≥ 1.83).

## 3. Build & test

```bash
cargo test -p tessera-core      # pure-logic unit tests — 13 passing
cargo clippy -- -D warnings     # lint gate
cargo build --release -p tessera
./target/release/tessera 4      # 4-pane grid; or `tessera` for the picker
```

## Crate versions (pinned, verified against the system libs)

| Crate | Version | Features |
|-------|---------|----------|
| gtk4  | 0.11    | `v4_14`  |
| vte4  | 0.10    | `v0_76`  |
| sourceview5 | 0.11 | (default) |

(Crate version does not set the min system GTK; the highest enabled feature does.
Never enable `v4_16+`, `v0_78+`, or vte4's `gtk_v4_18` on this system.)

## 4. Docker (containerized run with display forwarding)

```bash
docker build -t tessera:latest .     # multi-stage; compiles the GTK stack inside
./run-docker.sh 4                    # runs in-container, mounts Wayland socket + $PWD
```

The runtime image installs only: `libgtk-4-1 libvte-2.91-gtk4-0
libgtksourceview-5-0 librsvg2-common fonts-jetbrains-mono fonts-dejavu-core
libgl1-mesa-dri libegl1 libgles2`. See `Dockerfile` and `run-docker.sh`.
(`librsvg2-common` provides the gdk-pixbuf SVG loader so the bundled file-type
icons render — see below.)

## 5. Bundled file-type icons

The sidebar's colored, per-type icons come from the **Material Icon Theme**
(MIT — `crates/tessera/assets/icons/ATTRIBUTION.txt`). The SVGs are embedded in
the binary at build time (`include_str!`) and written to `~/.cache/tessera/icons`
at startup, so there is **no runtime download**. The folder icon is recolored to
a neutral grey. Rendering SVGs needs the gdk-pixbuf SVG loader (`librsvg2-common`).
