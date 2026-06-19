# BridgeShot v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn BridgeShot into a full screenshot tool — capture any window/region via the desktop portal, annotate with box/arrow/text/pen/highlighter in a chosen color, switch between this session's captures via a toggleable left thumbnail gallery, and export (saved PNG + image on the clipboard).

**Architecture:** The single 545-line `crates/tessera/src/bridgeshot.rs` becomes a module root that declares focused submodules (`tools`, `state`, `capture`, `canvas`, `export`, `gallery`). `bridgeshot.rs` stays the orchestrator (`launch`) — Rust resolves `mod foo;` inside `bridgeshot.rs` to `src/bridgeshot/foo.rs`, so no file rename and `main.rs` is untouched. Pure geometry/model lives in `tools.rs` (unit-tested without a display); GTK wiring lives in the other modules and is verified by `cargo build` + manual checks.

**Tech Stack:** Rust, GTK4 0.11 (feature `v4_14`), Cairo (`cairo-rs` 0.22 `png`), `gdk_pixbuf`, `ashpd` 0.13 (XDG screenshot portal, pure Rust), `async-channel` (already present).

## Global Constraints

- GTK features capped at `v4_14`; VTE at `v0_76`. NEVER enable `v4_16+`/`v0_78+`. (from `crates/tessera/Cargo.toml`)
- `cairo-rs` must stay `0.22` (matches the gtk4-reexported cairo).
- `ashpd` added **without** its `gtk4` feature (would pull a second gtk4 and break the build).
- No new **system** libraries (ashpd/zbus are pure Rust).
- All annotation geometry stored in **image space** (pixels of the source image), never widget space.
- Colors come only from `tools::PALETTE`; default color is blue (`PALETTE[1]`, the existing accent `#7aa2f7`).
- Numbered badges and the right-side label list are **removed**.
- Export: save PNG to `~/.cache/tessera/bridgeshot/shot-N.png` (unchanged scheme) **and** copy the rendered image to the clipboard; the status bar shows the saved path.

---

### Task 1: Add the `ashpd` dependency

**Files:**
- Modify: `crates/tessera/Cargo.toml` (dependencies table)

**Interfaces:**
- Produces: the `ashpd` crate available to later tasks.

- [ ] **Step 1: Add the dependency**

In `crates/tessera/Cargo.toml`, append to the `[dependencies]` section (after the `cairo-rs` line):

```toml
# XDG screenshot portal — capture any window/region on Wayland/COSMIC via the
# compositor's own picker. Pure Rust (zbus over D-Bus); no new system libs.
# The `gtk4` feature is intentionally OMITTED: it would pull a second gtk4
# version and break the build. We pass no WindowIdentifier (picker is
# compositor-driven), so we don't need it.
ashpd = "0.13"
```

- [ ] **Step 2: Build to fetch and compile the dependency**

Run: `cargo build -p tessera`
Expected: compiles successfully (downloads ashpd + zbus on first run).

- [ ] **Step 3: Verify no duplicate gtk4**

Run: `cargo tree -p tessera -d 2>/dev/null | grep -i '^gtk4' || echo "NO DUPLICATE gtk4"`
Expected: prints `NO DUPLICATE gtk4` (a duplicate would mean the ashpd gtk4 feature leaked in — if so, the build would already conflict).

- [ ] **Step 4: Commit**

```bash
git add crates/tessera/Cargo.toml Cargo.lock
git commit -m "build(bridgeshot): add ashpd for the screenshot portal"
```

---

### Task 2: `tools.rs` — annotation model + geometry (TDD)

Pure types and math, unit-tested without a display.

**Files:**
- Create: `crates/tessera/src/bridgeshot/tools.rs`
- Modify: `crates/tessera/src/bridgeshot.rs` (add `mod tools;` near the top, after the doc comment / `use` block — see step 3)

**Interfaces:**
- Produces:
  - `pub enum Tool { Box, Arrow, Text, Pen, Highlight }` (derives `Clone, Copy, PartialEq, Eq`)
  - `pub type Rgb = (f64, f64, f64);`
  - `pub const PALETTE: [Rgb; 6]`
  - `pub const DEFAULT_COLOR: Rgb` (== `PALETTE[1]`)
  - `pub enum Shape { Box{x,y,w,h:f64}, Arrow{x0,y0,x1,y1:f64}, Text{x,y:f64,content:String,size:f64}, Stroke{points:Vec<(f64,f64)>,highlight:bool} }`
  - `pub struct Annotation { pub shape: Shape, pub color: Rgb }`
  - `pub fn norm(x0:f64,y0:f64,x1:f64,y1:f64) -> (f64,f64,f64,f64)`
  - `pub fn arrow_head(x0:f64,y0:f64,x1:f64,y1:f64,head_len:f64,head_w:f64) -> [(f64,f64);3]`
  - `pub fn thumb_dims(w:i32,h:i32,target_w:i32) -> (i32,i32)`

- [ ] **Step 1: Write the failing tests**

Create `crates/tessera/src/bridgeshot/tools.rs` with ONLY the test module first:

```rust
//! BridgeShot annotation model + pure geometry helpers (unit-tested, no GTK).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_orders_corners() {
        assert_eq!(norm(5.0, 5.0, 1.0, 1.0), (1.0, 1.0, 4.0, 4.0));
        assert_eq!(norm(1.0, 1.0, 5.0, 3.0), (1.0, 1.0, 4.0, 2.0));
    }

    #[test]
    fn arrow_head_points_at_tip() {
        // Horizontal arrow (0,0)->(10,0), head 2 long, 2 wide.
        let [tip, left, right] = arrow_head(0.0, 0.0, 10.0, 0.0, 2.0, 2.0);
        assert_eq!(tip, (10.0, 0.0));
        // base center is 2 back from tip at (8,0); wings ±1 perpendicular.
        assert!((left.0 - 8.0).abs() < 1e-9 && (left.1 - 1.0).abs() < 1e-9);
        assert!((right.0 - 8.0).abs() < 1e-9 && (right.1 + 1.0).abs() < 1e-9);
    }

    #[test]
    fn arrow_head_zero_length_is_degenerate() {
        let pts = arrow_head(3.0, 3.0, 3.0, 3.0, 2.0, 2.0);
        assert_eq!(pts, [(3.0, 3.0), (3.0, 3.0), (3.0, 3.0)]);
    }

    #[test]
    fn thumb_dims_preserve_aspect_and_clamp() {
        assert_eq!(thumb_dims(1000, 500, 128), (128, 64));
        assert_eq!(thumb_dims(100, 50, 128), (100, 50)); // never upscale
        assert_eq!(thumb_dims(0, 10, 128), (1, 1)); // degenerate
    }
}
```

- [ ] **Step 2: Run tests to verify they fail (don't compile)**

Run: `cargo test -p tessera tools 2>&1 | tail -20`
Expected: FAIL — `cannot find function norm` / `arrow_head` / `thumb_dims` (not yet defined).

- [ ] **Step 3: Implement the module**

Prepend the implementation ABOVE the `#[cfg(test)]` block in `tools.rs`:

```rust
//! BridgeShot annotation model + pure geometry helpers (unit-tested, no GTK).

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Box,
    Arrow,
    Text,
    Pen,
    Highlight,
}

pub type Rgb = (f64, f64, f64);

/// Toolbar swatches.
pub const PALETTE: [Rgb; 6] = [
    (0.910, 0.302, 0.357), // red    #e84d5b
    (0.478, 0.635, 0.969), // blue   #7aa2f7  (existing accent)
    (0.878, 0.686, 0.408), // yellow #e0af68
    (0.549, 0.776, 0.451), // green  #8cc673
    (0.949, 0.949, 0.969), // white  #f2f2f7
    (0.102, 0.110, 0.149), // near-black #1a1c26
];

pub const DEFAULT_COLOR: Rgb = PALETTE[1];

pub enum Shape {
    Box { x: f64, y: f64, w: f64, h: f64 },
    Arrow { x0: f64, y0: f64, x1: f64, y1: f64 },
    Text { x: f64, y: f64, content: String, size: f64 },
    Stroke { points: Vec<(f64, f64)>, highlight: bool },
}

pub struct Annotation {
    pub shape: Shape,
    pub color: Rgb,
}

/// Order two drag corners into (x, y, w, h) with non-negative size.
pub fn norm(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64, f64, f64) {
    (x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs())
}

/// Triangle for an arrowhead at the (x1,y1) end: returns [tip, wing, wing].
pub fn arrow_head(
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    head_len: f64,
    head_w: f64,
) -> [(f64, f64); 3] {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-6 {
        return [(x1, y1), (x1, y1), (x1, y1)];
    }
    let (ux, uy) = (dx / len, dy / len);
    let (bx, by) = (x1 - ux * head_len, y1 - uy * head_len); // base center
    let (px, py) = (-uy, ux); // unit perpendicular
    let half = head_w / 2.0;
    [
        (x1, y1),
        (bx + px * half, by + py * half),
        (bx - px * half, by - py * half),
    ]
}

/// Thumbnail size: fit width to `target_w`, preserve aspect, never upscale.
pub fn thumb_dims(w: i32, h: i32, target_w: i32) -> (i32, i32) {
    if w <= 0 || h <= 0 {
        return (1, 1);
    }
    let tw = target_w.min(w).max(1);
    let scale = tw as f64 / w as f64;
    let th = ((h as f64 * scale).round() as i32).max(1);
    (tw, th)
}
```

Then declare the module: in `crates/tessera/src/bridgeshot.rs`, immediately after the final `use gtk4::{...};` import block (around line 22), add:

```rust
mod tools;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p tessera tools 2>&1 | tail -20`
Expected: PASS — 4 tests (`norm_orders_corners`, `arrow_head_points_at_tip`, `arrow_head_zero_length_is_degenerate`, `thumb_dims_preserve_aspect_and_clamp`).

Note: `bridgeshot.rs` will emit "unused" warnings for the new module until Task 8 wires it in — that's expected.

- [ ] **Step 5: Commit**

```bash
git add crates/tessera/src/bridgeshot/tools.rs crates/tessera/src/bridgeshot.rs
git commit -m "feat(bridgeshot): annotation model + geometry helpers (tools.rs)"
```

---

### Task 3: `state.rs` — documents + session state

**Files:**
- Create: `crates/tessera/src/bridgeshot/state.rs`
- Modify: `crates/tessera/src/bridgeshot.rs` (add `mod state;`)

**Interfaces:**
- Consumes: `tools::{Annotation, Tool, Rgb, DEFAULT_COLOR, thumb_dims}`
- Produces:
  - `pub struct Doc { pub pixbuf: Pixbuf, pub annos: Vec<Annotation>, pub thumb: Pixbuf }`
  - `pub enum Drag { Rect{x0,y0,x1,y1:f64}, Stroke{points:Vec<(f64,f64)>,highlight:bool} }`
  - `pub struct State { pub docs:Vec<Doc>, pub active:Option<usize>, pub tool:Tool, pub color:Rgb, pub drag:Option<Drag>, pub scale:f64, pub off_x:f64, pub off_y:f64, pub exports:u32 }`
  - `pub type Shot = Rc<RefCell<State>>;`
  - `impl State`: `pub fn new() -> Self`, `pub fn active_doc(&self) -> Option<&Doc>`, `pub fn active_doc_mut(&mut self) -> Option<&mut Doc>`, `pub fn push_anno(&mut self, a: Annotation)`, `pub fn undo(&mut self)`, `pub fn clear_active(&mut self)`
  - `pub fn add_doc(shot:&Shot, pixbuf:Pixbuf) -> usize` (builds thumbnail, appends, selects, returns index)
  - `pub fn to_image(s:&State, wx:f64, wy:f64) -> (f64,f64)` and `pub fn to_widget(s:&State, ix:f64, iy:f64) -> (f64,f64)`

- [ ] **Step 1: Implement the module**

Create `crates/tessera/src/bridgeshot/state.rs`:

```rust
//! BridgeShot session state: a list of captured documents (each with its own
//! annotations) plus the current tool/color and the active canvas transform.

#![allow(dead_code)] // wired up in the orchestrator task

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk_pixbuf::{InterpType, Pixbuf};

use super::tools::{thumb_dims, Annotation, Rgb, Tool, DEFAULT_COLOR};

const THUMB_W: i32 = 128;

/// One captured/opened image and its annotations.
pub struct Doc {
    pub pixbuf: Pixbuf,
    pub annos: Vec<Annotation>,
    pub thumb: Pixbuf,
}

/// An annotation being drawn (not yet committed), in image space.
pub enum Drag {
    Rect { x0: f64, y0: f64, x1: f64, y1: f64 },
    Stroke { points: Vec<(f64, f64)>, highlight: bool },
}

pub struct State {
    pub docs: Vec<Doc>,
    pub active: Option<usize>,
    pub tool: Tool,
    pub color: Rgb,
    pub drag: Option<Drag>,
    // Canvas transform for the active doc, recomputed every draw().
    pub scale: f64,
    pub off_x: f64,
    pub off_y: f64,
    pub exports: u32,
}

pub type Shot = Rc<RefCell<State>>;

impl State {
    pub fn new() -> Self {
        State {
            docs: Vec::new(),
            active: None,
            tool: Tool::Box,
            color: DEFAULT_COLOR,
            drag: None,
            scale: 1.0,
            off_x: 0.0,
            off_y: 0.0,
            exports: 0,
        }
    }

    pub fn active_doc(&self) -> Option<&Doc> {
        self.active.and_then(|i| self.docs.get(i))
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut Doc> {
        match self.active {
            Some(i) => self.docs.get_mut(i),
            None => None,
        }
    }

    pub fn push_anno(&mut self, a: Annotation) {
        if let Some(d) = self.active_doc_mut() {
            d.annos.push(a);
        }
    }

    pub fn undo(&mut self) {
        if let Some(d) = self.active_doc_mut() {
            d.annos.pop();
        }
    }

    pub fn clear_active(&mut self) {
        if let Some(d) = self.active_doc_mut() {
            d.annos.clear();
        }
    }
}

/// Append a new document (building its thumbnail), make it active, return index.
pub fn add_doc(shot: &Shot, pixbuf: Pixbuf) -> usize {
    let (tw, th) = thumb_dims(pixbuf.width(), pixbuf.height(), THUMB_W);
    let thumb = pixbuf
        .scale_simple(tw, th, InterpType::Bilinear)
        .unwrap_or_else(|| pixbuf.clone());
    let mut s = shot.borrow_mut();
    s.docs.push(Doc {
        pixbuf,
        annos: Vec::new(),
        thumb,
    });
    let idx = s.docs.len() - 1;
    s.active = Some(idx);
    s.drag = None;
    idx
}

/// Widget point -> image point using the active transform.
pub fn to_image(s: &State, wx: f64, wy: f64) -> (f64, f64) {
    let sc = if s.scale.abs() < 1e-6 { 1.0 } else { s.scale };
    ((wx - s.off_x) / sc, (wy - s.off_y) / sc)
}

/// Image point -> widget point using the active transform.
pub fn to_widget(s: &State, ix: f64, iy: f64) -> (f64, f64) {
    (s.off_x + ix * s.scale, s.off_y + iy * s.scale)
}
```

Then in `crates/tessera/src/bridgeshot.rs`, add after `mod tools;`:

```rust
mod state;
```

- [ ] **Step 2: Build**

Run: `cargo build -p tessera 2>&1 | tail -20`
Expected: compiles (warnings about unused new modules are fine).

- [ ] **Step 3: Commit**

```bash
git add crates/tessera/src/bridgeshot/state.rs crates/tessera/src/bridgeshot.rs
git commit -m "feat(bridgeshot): per-image documents + session state (state.rs)"
```

---

### Task 4: `capture.rs` — portal capture + window-snapshot fallback

**Files:**
- Create: `crates/tessera/src/bridgeshot/capture.rs`
- Modify: `crates/tessera/src/bridgeshot.rs` (add `mod capture;`)

**Interfaces:**
- Produces:
  - `pub fn capture_window_async<F: Fn(Option<Pixbuf>) + 'static>(window: &ApplicationWindow, done: F)` (moved from the old code, made public)
  - `pub fn capture_screen<F: Fn(Option<Pixbuf>) + 'static>(fallback: &ApplicationWindow, done: F)` — interactive portal screenshot; on any error falls back to `capture_window_async(fallback, done)`. `done` runs on the GTK main thread with the captured `Pixbuf` (or `None`).

- [ ] **Step 1: Implement the module**

Create `crates/tessera/src/bridgeshot/capture.rs`. The `capture_window_async`/`capture_paintable` bodies are copied verbatim from the current `bridgeshot.rs` (lines 448-506) and made `pub`/module-private; `capture_screen` is new.

```rust
//! Image capture for BridgeShot: the XDG screenshot portal (any window/region,
//! Wayland-safe) with the Tessera self-snapshot as a fallback.

#![allow(dead_code)] // wired up in the orchestrator task

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::ApplicationWindow;

/// Capture ANY window/region via the desktop portal. COSMIC shows its own
/// interactive picker; the chosen area comes back as a PNG we load. On any
/// failure (portal absent/denied/cancelled) we fall back to snapshotting
/// Tessera's own window so capture never dead-ends.
pub fn capture_screen<F: Fn(Option<Pixbuf>) + 'static>(fallback: &ApplicationWindow, done: F) {
    let fallback = fallback.clone();
    glib::spawn_future_local(async move {
        match request_portal_screenshot().await {
            Some(pb) => done(Some(pb)),
            None => capture_window_async(&fallback, done),
        }
    });
}

/// Returns the captured Pixbuf, or None if the portal failed/was cancelled.
async fn request_portal_screenshot() -> Option<Pixbuf> {
    use ashpd::desktop::screenshot::Screenshot;
    let response = Screenshot::request()
        .interactive(true)
        .modal(true)
        .send()
        .await
        .ok()?
        .response()
        .ok()?;
    // `uri()` Displays as a `file://` URI; glib decodes it to a path (handles
    // percent-encoding) without us depending on the url crate's API surface.
    let uri = response.uri().to_string();
    let (path, _host) = glib::filename_from_uri(&uri).ok()?;
    Pixbuf::from_file(&path).ok()
}

/// Snapshot the live Tessera window into a `Pixbuf`, asynchronously.
///
/// `WidgetPaintable` only records a render node on the widget's *next* snapshot
/// after it is attached — so a synchronous capture of an already-drawn, static
/// widget yields an empty node. Instead we attach the paintable, force a
/// redraw, and read back two frame-clock ticks later. A timeout backstop avoids
/// hanging if the frame clock is idle (e.g. the window is fully occluded).
pub fn capture_window_async<F: Fn(Option<Pixbuf>) + 'static>(window: &ApplicationWindow, done: F) {
    let target: gtk4::Widget = window
        .child()
        .unwrap_or_else(|| window.clone().upcast::<gtk4::Widget>());
    let renderer = match window.renderer() {
        Some(r) => r,
        None => {
            done(None);
            return;
        }
    };
    let paintable = gtk4::WidgetPaintable::new(Some(&target));
    target.queue_draw();

    let done = std::rc::Rc::new(std::cell::RefCell::new(Some(done)));
    let ticks = std::cell::Cell::new(0u32);
    {
        let done = done.clone();
        target.add_tick_callback(move |w, _clock| {
            let n = ticks.get() + 1;
            ticks.set(n);
            if n < 2 {
                return glib::ControlFlow::Continue;
            }
            let pb = capture_paintable(&paintable, &renderer, w);
            if let Some(cb) = done.borrow_mut().take() {
                cb(pb);
            }
            glib::ControlFlow::Break
        });
    }

    let done2 = done;
    glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
        if let Some(cb) = done2.borrow_mut().take() {
            cb(None);
        }
    });
}

/// Render the paintable's current node to a `Pixbuf` via the window renderer.
fn capture_paintable(
    paintable: &gtk4::WidgetPaintable,
    renderer: &gtk4::gsk::Renderer,
    w: &gtk4::Widget,
) -> Option<Pixbuf> {
    let (pw, ph) = (w.width(), w.height());
    if pw <= 0 || ph <= 0 {
        return None;
    }
    let snapshot = gtk4::Snapshot::new();
    paintable.snapshot(snapshot.upcast_ref::<gtk4::gdk::Snapshot>(), pw as f64, ph as f64);
    let node = snapshot.to_node()?;
    let texture = renderer.render_texture(&node, None);
    let tmp = std::env::temp_dir().join("tessera-bridgeshot-capture.png");
    texture.save_to_png(&tmp).ok()?;
    Pixbuf::from_file(&tmp).ok()
}
```

Then in `crates/tessera/src/bridgeshot.rs`, add after `mod state;`:

```rust
mod capture;
```

- [ ] **Step 2: Build (this is the real ashpd integration check)**

Run: `cargo build -p tessera 2>&1 | tail -30`
Expected: compiles. If `response.uri()` type lacks `to_string`/`Display`, or the builder signature differs, fix per docs.rs `ashpd::desktop::screenshot` before continuing.

- [ ] **Step 3: Commit**

```bash
git add crates/tessera/src/bridgeshot/capture.rs crates/tessera/src/bridgeshot.rs
git commit -m "feat(bridgeshot): screenshot-portal capture with window fallback (capture.rs)"
```

> **Risk note (no code change):** If at manual-test time the portal future never resolves under `glib::spawn_future_local`, switch to a worker thread: enable `ashpd = { version = "0.13", features = ["tokio"] }`, run the request inside `std::thread::spawn(move || tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(...))`, and deliver the resulting path to the GTK loop over an `async_channel::unbounded()` receiver awaited in `glib::spawn_future_local`. The public `capture_screen` signature stays the same.

---

### Task 5: `canvas.rs` — rendering + per-tool input + text overlay

**Files:**
- Create: `crates/tessera/src/bridgeshot/canvas.rs`
- Modify: `crates/tessera/src/bridgeshot.rs` (add `mod canvas;`)

**Interfaces:**
- Consumes: `state::{Shot, State, Drag, to_image, to_widget}`, `tools::{Tool, Shape, Annotation, arrow_head, norm}`
- Produces:
  - `pub fn paint_annotations(cr:&cairo::Context, annos:&[Annotation], scale:f64)` — draws committed annotations at the current cairo transform (shared with export).
  - `pub struct CanvasUi { pub area: DrawingArea, pub overlay: Overlay }`
  - `pub fn build(shot:&Shot) -> CanvasUi` — creates the DrawingArea (draw func + box/arrow/pen/highlight gesture) wrapped in an Overlay hosting the text-entry Fixed layer.

- [ ] **Step 1: Implement the module**

Create `crates/tessera/src/bridgeshot/canvas.rs`:

```rust
//! Canvas: renders the active document + annotations, and turns pointer input
//! into annotations according to the current tool. Text uses an Entry placed on
//! a Fixed overlay above the DrawingArea.

#![allow(dead_code)] // wired up in the orchestrator task

use std::f64::consts::TAU;

use gtk4::cairo;
use gtk4::gdk::prelude::GdkCairoContextExt; // cr.set_source_pixbuf
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    DrawingArea, Entry, EventControllerFocus, EventControllerKey, Fixed, GestureClick,
    GestureDrag, Overlay,
};

use super::state::{to_image, to_widget, Drag, Shot, State};
use super::tools::{arrow_head, norm, Annotation, Shape, Tool};

const BG: (f64, f64, f64) = (0.102, 0.106, 0.149); // #1a1b26
const PEN_W: f64 = 3.0; // image-space px
const HL_W: f64 = 16.0;
const TEXT_SIZE: f64 = 22.0; // image-space px
const ARROW_HEAD_LEN: f64 = 18.0;
const ARROW_HEAD_W: f64 = 14.0;

pub struct CanvasUi {
    pub area: DrawingArea,
    pub overlay: Overlay,
}

pub fn build(shot: &Shot) -> CanvasUi {
    let area = DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.add_css_class("bridgeshot-canvas");

    let fixed = Fixed::new();
    fixed.set_can_target(false); // transparent to input until a text entry exists

    let overlay = Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&fixed);

    // Draw function.
    {
        let shot = shot.clone();
        area.set_draw_func(move |_a, cr, w, h| draw(cr, w, h, &shot));
    }

    // Box / Arrow / Pen / Highlight via drag.
    install_drag(&area, shot);

    // Text via click (only acts when the Text tool is active).
    install_text_click(&area, &fixed, shot);

    CanvasUi { area, overlay }
}

fn draw(cr: &cairo::Context, w: i32, h: i32, shot: &Shot) {
    cr.set_source_rgb(BG.0, BG.1, BG.2);
    let _ = cr.paint();

    let mut s = shot.borrow_mut();
    let Some(doc_idx) = s.active else {
        draw_hint(cr, w, h);
        return;
    };
    let pb = s.docs[doc_idx].pixbuf.clone();

    let iw = pb.width() as f64;
    let ih = pb.height() as f64;
    let scale = (w as f64 / iw).min(h as f64 / ih).min(4.0);
    s.scale = scale;
    s.off_x = (w as f64 - iw * scale) / 2.0;
    s.off_y = (h as f64 - ih * scale) / 2.0;

    let _ = cr.save();
    cr.translate(s.off_x, s.off_y);
    cr.scale(scale, scale);
    cr.set_source_pixbuf(&pb, 0.0, 0.0);
    let _ = cr.paint();

    paint_annotations(cr, &s.docs[doc_idx].annos, scale);

    // In-progress preview in the current color.
    if let Some(drag) = &s.drag {
        let color = s.color;
        match drag {
            Drag::Rect { x0, y0, x1, y1 } => match s.tool {
                Tool::Arrow => paint_arrow(cr, *x0, *y0, *x1, *y1, color, scale),
                _ => {
                    let (x, y, bw, bh) = norm(*x0, *y0, *x1, *y1);
                    cr.set_source_rgb(color.0, color.1, color.2);
                    cr.set_line_width(2.0 / scale);
                    cr.set_dash(&[6.0 / scale, 4.0 / scale], 0.0);
                    cr.rectangle(x, y, bw, bh);
                    let _ = cr.stroke();
                    cr.set_dash(&[], 0.0);
                }
            },
            Drag::Stroke { points, highlight } => {
                paint_stroke(cr, points, *highlight, color, scale)
            }
        }
    }
    let _ = cr.restore();
}

fn draw_hint(cr: &cairo::Context, w: i32, h: i32) {
    cr.set_source_rgb(0.55, 0.6, 0.75);
    cr.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(15.0);
    let msg = "Capture, Open an image, or drop one here";
    let tw = cr.text_extents(msg).map(|e| e.width()).unwrap_or(0.0);
    cr.move_to((w as f64 - tw) / 2.0, h as f64 / 2.0);
    let _ = cr.show_text(msg);
}

/// Draw all committed annotations. `scale` keeps stroke widths constant on screen
/// and at export (export passes scale = 1.0).
pub fn paint_annotations(cr: &cairo::Context, annos: &[Annotation], scale: f64) {
    for a in annos {
        let c = a.color;
        match &a.shape {
            Shape::Box { x, y, w, h } => {
                cr.set_source_rgb(c.0, c.1, c.2);
                cr.set_line_width(3.0 / scale);
                cr.rectangle(*x, *y, *w, *h);
                let _ = cr.stroke();
            }
            Shape::Arrow { x0, y0, x1, y1 } => paint_arrow(cr, *x0, *y0, *x1, *y1, c, scale),
            Shape::Stroke { points, highlight } => paint_stroke(cr, points, *highlight, c, scale),
            Shape::Text { x, y, content, size } => {
                cr.set_source_rgb(c.0, c.1, c.2);
                cr.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                cr.set_font_size(*size);
                cr.move_to(*x, *y + *size); // x,y is top-left; baseline below
                let _ = cr.show_text(content);
            }
        }
    }
}

fn paint_arrow(cr: &cairo::Context, x0: f64, y0: f64, x1: f64, y1: f64, c: (f64, f64, f64), scale: f64) {
    cr.set_source_rgb(c.0, c.1, c.2);
    cr.set_line_width(3.0 / scale);
    cr.move_to(x0, y0);
    cr.line_to(x1, y1);
    let _ = cr.stroke();
    let [tip, l, r] = arrow_head(x0, y0, x1, y1, ARROW_HEAD_LEN, ARROW_HEAD_W);
    cr.move_to(tip.0, tip.1);
    cr.line_to(l.0, l.1);
    cr.line_to(r.0, r.1);
    cr.close_path();
    let _ = cr.fill();
}

fn paint_stroke(cr: &cairo::Context, points: &[(f64, f64)], highlight: bool, c: (f64, f64, f64), scale: f64) {
    if points.is_empty() {
        return;
    }
    if highlight {
        cr.set_source_rgba(c.0, c.1, c.2, 0.35);
        cr.set_line_width(HL_W / scale);
    } else {
        cr.set_source_rgb(c.0, c.1, c.2);
        cr.set_line_width(PEN_W / scale);
    }
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(points[0].0, points[0].1);
    for p in &points[1..] {
        cr.line_to(p.0, p.1);
    }
    let _ = cr.stroke();
}

fn install_drag(area: &DrawingArea, shot: &Shot) {
    let drag = GestureDrag::new();

    let (sb, cb) = (shot.clone(), area.clone());
    drag.connect_drag_begin(move |_g, x, y| {
        let mut s = sb.borrow_mut();
        if s.active.is_none() || s.tool == Tool::Text {
            return;
        }
        let (ix, iy) = to_image(&s, x, y);
        s.drag = Some(match s.tool {
            Tool::Pen => Drag::Stroke { points: vec![(ix, iy)], highlight: false },
            Tool::Highlight => Drag::Stroke { points: vec![(ix, iy)], highlight: true },
            _ => Drag::Rect { x0: ix, y0: iy, x1: ix, y1: iy },
        });
        drop(s);
        cb.queue_draw();
    });

    let (su, cu) = (shot.clone(), area.clone());
    drag.connect_drag_update(move |g, dx, dy| {
        let Some((sx, sy)) = g.start_point() else { return };
        let mut s = su.borrow_mut();
        let (ix, iy) = to_image(&s, sx + dx, sy + dy);
        match s.drag.as_mut() {
            Some(Drag::Rect { x1, y1, .. }) => {
                *x1 = ix;
                *y1 = iy;
            }
            Some(Drag::Stroke { points, .. }) => points.push((ix, iy)),
            None => return,
        }
        drop(s);
        cu.queue_draw();
    });

    let (se, ce) = (shot.clone(), area.clone());
    drag.connect_drag_end(move |_g, _dx, _dy| {
        let mut s = se.borrow_mut();
        let color = s.color;
        let tool = s.tool;
        if let Some(drag) = s.drag.take() {
            match drag {
                Drag::Rect { x0, y0, x1, y1 } => {
                    if tool == Tool::Arrow {
                        let dist = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
                        if dist > 4.0 {
                            s.push_anno(Annotation { shape: Shape::Arrow { x0, y0, x1, y1 }, color });
                        }
                    } else {
                        let (x, y, w, h) = norm(x0, y0, x1, y1);
                        if w > 4.0 && h > 4.0 {
                            s.push_anno(Annotation { shape: Shape::Box { x, y, w, h }, color });
                        }
                    }
                }
                Drag::Stroke { points, highlight } => {
                    if points.len() > 1 {
                        s.push_anno(Annotation { shape: Shape::Stroke { points, highlight }, color });
                    }
                }
            }
        }
        drop(s);
        ce.queue_draw();
    });

    area.add_controller(drag);
}

fn install_text_click(area: &DrawingArea, fixed: &Fixed, shot: &Shot) {
    let click = GestureClick::new();
    let (sb, fb, ab) = (shot.clone(), fixed.clone(), area.clone());
    click.connect_pressed(move |_g, _n, x, y| {
        let s = sb.borrow();
        if s.active.is_none() || s.tool != Tool::Text {
            return;
        }
        let (ix, iy) = to_image(&s, x, y);
        drop(s);
        spawn_text_entry(&sb, &fb, &ab, x, y, ix, iy);
    });
    area.add_controller(click);
}

/// Place an Entry at widget point (wx,wy); commit to a Text annotation at image
/// point (ix,iy) on Enter or focus-out, cancel on Escape.
fn spawn_text_entry(shot: &Shot, fixed: &Fixed, area: &DrawingArea, wx: f64, wy: f64, ix: f64, iy: f64) {
    let entry = Entry::new();
    entry.set_placeholder_text(Some("type, Enter to place"));
    entry.add_css_class("bridgeshot-text-entry");
    entry.set_width_request(160);
    fixed.set_can_target(true);
    fixed.put(&entry, wx, wy);
    entry.grab_focus();

    let committed = std::rc::Rc::new(std::cell::Cell::new(false));

    let commit = {
        let (shot, fixed, area, entry, committed) =
            (shot.clone(), fixed.clone(), area.clone(), entry.clone(), committed.clone());
        move |save: bool| {
            if committed.replace(true) {
                return;
            }
            let text = entry.text().to_string();
            if save && !text.trim().is_empty() {
                let mut s = shot.borrow_mut();
                let color = s.color;
                s.push_anno(Annotation {
                    shape: Shape::Text { x: ix, y: iy, content: text, size: TEXT_SIZE },
                    color,
                });
            }
            fixed.remove(&entry);
            fixed.set_can_target(false);
            area.queue_draw();
        }
    };

    {
        let commit = commit.clone();
        entry.connect_activate(move |_| commit(true));
    }
    {
        let focus = EventControllerFocus::new();
        let commit = commit.clone();
        focus.connect_leave(move |_| commit(true));
        entry.add_controller(focus);
    }
    {
        let key = EventControllerKey::new();
        let commit = commit.clone();
        key.connect_key_pressed(move |_c, keyval, _code, _mods| {
            if keyval == gtk4::gdk::Key::Escape {
                commit(false);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        entry.add_controller(key);
    }
}
```

Then in `crates/tessera/src/bridgeshot.rs`, add after `mod capture;`:

```rust
mod canvas;
```

- [ ] **Step 2: Build**

Run: `cargo build -p tessera 2>&1 | tail -30`
Expected: compiles (unused-warnings until Task 8).

- [ ] **Step 3: Commit**

```bash
git add crates/tessera/src/bridgeshot/canvas.rs crates/tessera/src/bridgeshot.rs
git commit -m "feat(bridgeshot): canvas rendering, tool input, text overlay (canvas.rs)"
```

---

### Task 6: `export.rs` — render to PNG + copy image to clipboard

**Files:**
- Create: `crates/tessera/src/bridgeshot/export.rs`
- Modify: `crates/tessera/src/bridgeshot.rs` (add `mod export;`)

**Interfaces:**
- Consumes: `state::Shot`, `canvas::paint_annotations`
- Produces:
  - `pub fn export_png(shot:&Shot) -> Result<(PathBuf, Pixbuf), String>` — renders the active doc + annotations to a PNG saved under the cache dir; returns the saved path and a `Pixbuf` of the result (for the clipboard). Errors if no active doc.

- [ ] **Step 1: Implement the module**

Create `crates/tessera/src/bridgeshot/export.rs`:

```rust
//! Render the active document (image + annotations) to a PNG and return a
//! Pixbuf of the result for the clipboard.

#![allow(dead_code)] // wired up in the orchestrator task

use std::path::PathBuf;

use gtk4::cairo;
use gtk4::gdk::prelude::GdkCairoContextExt;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;

use super::canvas::paint_annotations;
use super::state::Shot;

pub fn export_png(shot: &Shot) -> Result<(PathBuf, Pixbuf), String> {
    let s = shot.borrow();
    let doc = s.active_doc().ok_or("nothing to export")?;
    let pb = doc.pixbuf.clone();
    let (iw, ih) = (pb.width(), pb.height());

    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, iw, ih).map_err(|e| e.to_string())?;
    {
        let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
        cr.set_source_pixbuf(&pb, 0.0, 0.0);
        let _ = cr.paint();
        // scale = 1.0: stroke widths are authored in image space.
        paint_annotations(&cr, &doc.annos, 1.0);
    }
    drop(s);

    let dir = glib::user_cache_dir().join("tessera").join("bridgeshot");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let n = {
        let mut s = shot.borrow_mut();
        s.exports += 1;
        s.exports
    };
    let path = dir.join(format!("shot-{n}.png"));
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    surface.write_to_png(&mut file).map_err(|e| e.to_string())?;

    let out = Pixbuf::from_file(&path).map_err(|e| e.to_string())?;
    Ok((path, out))
}
```

Then in `crates/tessera/src/bridgeshot.rs`, add after `mod canvas;`:

```rust
mod export;
```

- [ ] **Step 2: Build**

Run: `cargo build -p tessera 2>&1 | tail -20`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add crates/tessera/src/bridgeshot/export.rs crates/tessera/src/bridgeshot.rs
git commit -m "feat(bridgeshot): export rendered PNG + clipboard image (export.rs)"
```

---

### Task 7: `gallery.rs` — toggleable thumbnail strip

**Files:**
- Create: `crates/tessera/src/bridgeshot/gallery.rs`
- Modify: `crates/tessera/src/bridgeshot.rs` (add `mod gallery;`)

**Interfaces:**
- Consumes: `state::Shot`
- Produces:
  - `pub struct Gallery { pub root: ScrolledWindow, list: GtkBox }`
  - `pub fn new() -> Gallery`
  - `pub fn add_thumb(&self, shot:&Shot, canvas:&DrawingArea, index:usize, thumb:&Pixbuf)` — appends a clickable thumbnail button that, on click, sets `state.active = index`, redraws the canvas, and updates the selection highlight; the new thumbnail is auto-selected.

- [ ] **Step 1: Implement the module**

Create `crates/tessera/src/bridgeshot/gallery.rs`:

```rust
//! Left-side thumbnail strip of this session's captures. Clicking a thumbnail
//! makes that document active on the canvas.

#![allow(dead_code)] // wired up in the orchestrator task

use gtk4::gdk::Texture;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, DrawingArea, Orientation, Picture, ScrolledWindow};

use super::state::Shot;

pub struct Gallery {
    pub root: ScrolledWindow,
    list: GtkBox,
}

pub fn new() -> Gallery {
    let list = GtkBox::new(Orientation::Vertical, 6);
    list.add_css_class("bridgeshot-gallery");
    list.set_margin_top(6);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);
    let root = ScrolledWindow::builder()
        .child(&list)
        .width_request(150)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .build();
    Gallery { root, list }
}

impl Gallery {
    pub fn add_thumb(&self, shot: &Shot, canvas: &DrawingArea, index: usize, thumb: &Pixbuf) {
        let texture = Texture::for_pixbuf(thumb);
        let pic = Picture::for_paintable(&texture);
        pic.set_can_shrink(true);
        let btn = Button::builder().child(&pic).build();
        btn.add_css_class("bridgeshot-thumb");

        let (sb, cb, list) = (shot.clone(), canvas.clone(), self.list.clone());
        btn.connect_clicked(move |b| {
            sb.borrow_mut().active = Some(index);
            select_only(&list, b);
            cb.queue_draw();
        });

        self.list.append(&btn);
        select_only(&self.list, &btn); // auto-select the newest
    }
}

/// Mark `btn` selected, clear the css class from its siblings.
fn select_only(list: &GtkBox, btn: &Button) {
    let mut child = list.first_child();
    while let Some(w) = child {
        w.remove_css_class("selected");
        child = w.next_sibling();
    }
    btn.add_css_class("selected");
}
```

Then in `crates/tessera/src/bridgeshot.rs`, add after `mod export;`:

```rust
mod gallery;
```

- [ ] **Step 2: Build**

Run: `cargo build -p tessera 2>&1 | tail -20`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add crates/tessera/src/bridgeshot/gallery.rs crates/tessera/src/bridgeshot.rs
git commit -m "feat(bridgeshot): session thumbnail gallery (gallery.rs)"
```

---

### Task 8: Rewrite `bridgeshot.rs` — orchestrator wiring it all together

This replaces the body of `bridgeshot.rs` (everything except the `mod` lines) with the new `launch`. It builds the header (gallery toggle, Capture, Open, Undo, Clear, Export), the toolbar row (tools + color swatches), the layout (gallery | canvas), window-local keys, and wires capture/open/drop/export through the new modules. The old `State`, `Anno`, `draw*`, `add_row`, `to_image`, `norm`, `export_png`, `capture_*`, and constants are deleted.

**Files:**
- Modify: `crates/tessera/src/bridgeshot.rs` (full rewrite below the doc comment)

**Interfaces:**
- Consumes: `tools`, `state`, `capture`, `canvas`, `export`, `gallery` (all above)
- Produces: `pub fn launch(main: &ApplicationWindow)` (signature unchanged — `app.rs:74` and `keys.rs:88` keep working).

- [ ] **Step 1: Replace the file contents**

Overwrite `crates/tessera/src/bridgeshot.rs` entirely with:

```rust
//! BridgeShot — capture any window/region (via the desktop screenshot portal),
//! annotate it with boxes, arrows, freehand pen, highlighter, and text in a
//! chosen color, switch between this session's captures via a toggleable left
//! thumbnail gallery, then export an annotated PNG (also copied to the
//! clipboard) to hand to an AI or paste into a chat.

mod canvas;
mod capture;
mod export;
mod gallery;
mod state;
mod tools;

use std::rc::Rc;

use gtk4::gdk::Texture;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::gdk::{Key, ModifierType};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, DrawingArea, DropTarget, EventControllerKey,
    HeaderBar, Label, Orientation, Paned, ToggleButton, Window,
};

use state::{add_doc, Shot, State};
use tools::{Tool, PALETTE};

/// Open the BridgeShot window. `main` is Tessera's window — used as the capture
/// fallback when the portal is unavailable.
pub fn launch(main: &ApplicationWindow) {
    let shot: Shot = Rc::new(std::cell::RefCell::new(State::new()));

    let win = Window::builder()
        .title("BridgeShot")
        .default_width(1180)
        .default_height(780)
        .build();
    win.add_css_class("bridgeshot-window");

    // ----- header -----
    let header = HeaderBar::new();
    let gallery_btn = ToggleButton::new();
    gallery_btn.set_icon_name("sidebar-show-symbolic");
    gallery_btn.set_active(true);
    gallery_btn.set_tooltip_text(Some("Toggle gallery (Alt+G)"));
    gallery_btn.add_css_class("flat");
    let capture_btn = Button::with_label("Capture");
    capture_btn.add_css_class("suggested-action");
    let open_btn = Button::with_label("Open…");
    let undo_btn = Button::with_label("Undo");
    let clear_btn = Button::with_label("Clear");
    let export_btn = Button::with_label("Export");
    export_btn.add_css_class("suggested-action");
    header.pack_start(&gallery_btn);
    header.pack_start(&capture_btn);
    header.pack_start(&open_btn);
    header.pack_end(&export_btn);
    header.pack_end(&clear_btn);
    header.pack_end(&undo_btn);
    win.set_titlebar(Some(&header));

    // ----- canvas + gallery -----
    let canvas_ui = canvas::build(&shot);
    let area = canvas_ui.area.clone();
    let gallery = gallery::new();

    let split = Paned::new(Orientation::Horizontal);
    split.set_start_child(Some(&gallery.root));
    split.set_end_child(Some(&canvas_ui.overlay));
    split.set_resize_start_child(false);
    split.set_shrink_start_child(false);
    split.set_resize_end_child(true);
    split.set_position(160);

    // ----- toolbar row (tools + colors) -----
    let toolbar = GtkBox::new(Orientation::Horizontal, 6);
    toolbar.add_css_class("bridgeshot-toolbar");
    let tools_def = [
        (Tool::Box, "Box", "B"),
        (Tool::Arrow, "Arrow", "A"),
        (Tool::Text, "Text", "T"),
        (Tool::Pen, "Pen", "P"),
        (Tool::Highlight, "Highlight", "H"),
    ];
    let tool_btns: Rc<Vec<(Tool, ToggleButton)>> = Rc::new(
        tools_def
            .iter()
            .map(|(tool, label, _)| {
                let b = ToggleButton::with_label(label);
                b.add_css_class("bridgeshot-tool");
                (*tool, b)
            })
            .collect(),
    );
    for (tool, btn) in tool_btns.iter() {
        let (tool, sb, tbs) = (*tool, shot.clone(), tool_btns.clone());
        btn.connect_clicked(move |b| {
            // Radio behaviour: keep this one active, clear siblings.
            if !b.is_active() {
                b.set_active(true);
                return;
            }
            sb.borrow_mut().tool = tool;
            for (_, other) in tbs.iter() {
                if other != b {
                    other.set_active(false);
                }
            }
        });
        toolbar.append(btn);
    }
    tool_btns[0].1.set_active(true); // Box default

    let sep = gtk4::Separator::new(Orientation::Vertical);
    toolbar.append(&sep);

    let swatches = GtkBox::new(Orientation::Horizontal, 4);
    swatches.add_css_class("bridgeshot-swatches");
    for (i, color) in PALETTE.iter().enumerate() {
        let sw = Button::new();
        sw.add_css_class("bridgeshot-swatch");
        sw.add_css_class(&format!("swatch-{i}"));
        sw.set_tooltip_text(Some("Annotation color"));
        let (c, sb) = (*color, shot.clone());
        sw.connect_clicked(move |b| {
            sb.borrow_mut().color = c;
            if let Some(p) = b.parent() {
                let mut ch = p.first_child();
                while let Some(w) = ch {
                    w.remove_css_class("selected");
                    ch = w.next_sibling();
                }
            }
            b.add_css_class("selected");
        });
        if i == 1 {
            sw.add_css_class("selected"); // default blue
        }
        swatches.append(&sw);
    }
    toolbar.append(&swatches);

    // ----- status bar -----
    let status = Label::new(Some(
        "Capture anything, Open an image, or drop one here — then pick a tool and draw.",
    ));
    status.set_xalign(0.0);
    status.set_selectable(true);
    status.set_wrap(true);
    status.add_css_class("bridgeshot-status");

    // ----- assemble -----
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&toolbar);
    let mid = GtkBox::new(Orientation::Vertical, 0);
    mid.set_vexpand(true);
    mid.append(&split);
    root.append(&mid);
    root.append(&status);
    win.set_child(Some(&root));

    let gallery = Rc::new(gallery);

    // Helper to load a Pixbuf as a new doc + thumbnail.
    let load = {
        let (shot, area, gallery) = (shot.clone(), area.clone(), gallery.clone());
        Rc::new(move |pb: Pixbuf| {
            let idx = add_doc(&shot, pb);
            let thumb = shot.borrow().docs[idx].thumb.clone();
            gallery.add_thumb(&shot, &area, idx, &thumb);
            area.queue_draw();
        })
    };

    // ----- gallery toggle -----
    {
        let gr = gallery.root.clone();
        gallery_btn.connect_toggled(move |b| gr.set_visible(b.is_active()));
    }

    // ----- capture (portal, window fallback) -----
    {
        let (main, win, status, load) =
            (main.clone(), win.clone(), status.clone(), load.clone());
        capture_btn.connect_clicked(move |_| {
            status.set_text("Choose what to capture…");
            win.set_visible(false);
            let (win, status, load) = (win.clone(), status.clone(), load.clone());
            // Let the window hide before the picker appears.
            glib::timeout_add_local_once(std::time::Duration::from_millis(120), {
                let main = main.clone();
                move || {
                    capture::capture_screen(&main, move |pb| {
                        match pb {
                            Some(pb) => {
                                load(pb);
                                status.set_text("Captured — pick a tool and draw, then Export.");
                            }
                            None => status.set_text("Capture cancelled or unavailable."),
                        }
                        win.present();
                    });
                }
            });
        });
    }

    // ----- open -----
    {
        let (win, status, load) = (win.clone(), status.clone(), load.clone());
        open_btn.connect_clicked(move |_| {
            let dialog = gtk4::FileDialog::builder().title("Open image").build();
            let (status, load) = (status.clone(), load.clone());
            dialog.open(Some(&win), gtk4::gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        if let Ok(pb) = Pixbuf::from_file(&path) {
                            load(pb);
                            status.set_text("Loaded image — pick a tool and draw.");
                        }
                    }
                }
            });
        });
    }

    // ----- drop -----
    {
        let dt = DropTarget::new(gtk4::gio::File::static_type(), gtk4::gdk::DragAction::COPY);
        let (status, load) = (status.clone(), load.clone());
        dt.connect_drop(move |_t, value, _x, _y| {
            if let Ok(file) = value.get::<gtk4::gio::File>() {
                if let Some(path) = file.path() {
                    if let Ok(pb) = Pixbuf::from_file(&path) {
                        load(pb);
                        status.set_text("Loaded image — pick a tool and draw.");
                        return true;
                    }
                }
            }
            false
        });
        area.add_controller(dt);
    }

    // ----- undo / clear -----
    {
        let (shot, area) = (shot.clone(), area.clone());
        undo_btn.connect_clicked(move |_| {
            shot.borrow_mut().undo();
            area.queue_draw();
        });
    }
    {
        let (shot, area) = (shot.clone(), area.clone());
        clear_btn.connect_clicked(move |_| {
            shot.borrow_mut().clear_active();
            area.queue_draw();
        });
    }

    // ----- export (save PNG + copy image to clipboard) -----
    {
        let (shot, status, win) = (shot.clone(), status.clone(), win.clone());
        export_btn.connect_clicked(move |_| match export::export_png(&shot) {
            Ok((path, pb)) => {
                let texture = Texture::for_pixbuf(&pb);
                win.clipboard().set_texture(&texture);
                status.set_text(&format!("Exported → {} (image copied to clipboard)", path.display()));
            }
            Err(e) => status.set_text(&format!("Export failed: {e}")),
        });
    }

    // ----- window-local keys -----
    install_keys(&win, &shot, &area, &gallery_btn, &tool_btns);

    // ----- auto-capture Tessera on open (self-snapshot, like before) -----
    {
        let (status, load, win) = (status.clone(), load.clone(), win.clone());
        capture::capture_window_async(main, move |pb| {
            if let Some(pb) = pb {
                load(pb);
                status.set_text("Captured Tessera — or Capture anything else.");
            }
            win.present();
        });
    }
}

fn install_keys(
    win: &Window,
    shot: &Shot,
    area: &DrawingArea,
    gallery_btn: &ToggleButton,
    tool_btns: &Rc<Vec<(Tool, ToggleButton)>>,
) {
    let controller = EventControllerKey::new();
    let (shot, area, gallery_btn, tool_btns) =
        (shot.clone(), area.clone(), gallery_btn.clone(), tool_btns.clone());
    controller.connect_key_pressed(move |_c, keyval, _code, mods| {
        // Ctrl+Z -> undo.
        if mods.contains(ModifierType::CONTROL_MASK) && keyval == Key::z {
            shot.borrow_mut().undo();
            area.queue_draw();
            return glib::Propagation::Stop;
        }
        // Alt+G -> toggle gallery.
        if mods.contains(ModifierType::ALT_MASK) && keyval == Key::g {
            gallery_btn.set_active(!gallery_btn.is_active());
            return glib::Propagation::Stop;
        }
        // Bare letters -> select tool (skipped automatically while an Entry has
        // focus, since the Entry consumes the keypress before it bubbles here).
        let pick = match keyval {
            Key::b => Some(0),
            Key::a => Some(1),
            Key::t => Some(2),
            Key::p => Some(3),
            Key::h => Some(4),
            _ => None,
        };
        if let Some(i) = pick {
            if !mods.contains(ModifierType::CONTROL_MASK) && !mods.contains(ModifierType::ALT_MASK) {
                tool_btns[i].1.set_active(true);
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
    win.add_controller(controller);
}
```

- [ ] **Step 2: Build**

Run: `cargo build -p tessera 2>&1 | tail -40`
Expected: compiles with NO unused-warnings from the bridgeshot modules now (they're all wired). Fix any signature mismatches against the interfaces declared in Tasks 2-7.

- [ ] **Step 3: Manual smoke test**

Run: `cargo run -p tessera`
Then:
1. Press **Alt+P** (or click the camera icon) → BridgeShot opens, auto-snapshot of Tessera shows on the canvas with a thumbnail in the left gallery.
2. Click **Capture** → BridgeShot hides, COSMIC's screenshot picker appears; pick a region/window → it loads as a new thumbnail and becomes active.
3. With **Box** selected, drag on the image → a colored rectangle. Switch color swatch → next box uses it.
4. **Arrow** (or press `A`): drag → arrow with a head. **Pen** (`P`): scribble. **Highlight** (`H`): translucent thick stroke. **Text** (`T`): click → type → Enter places it.
5. Click the first gallery thumbnail → canvas switches back, its annotations preserved.
6. **Undo** removes the last annotation; **Clear** empties the active image.
7. **Export** → status shows the saved path; paste into a chat → the image pastes.
8. **Alt+G** hides/shows the gallery.

Expected: all steps work. (Annotations being visible but unstyled toolbar is fine — CSS is Task 9.)

- [ ] **Step 4: Commit**

```bash
git add crates/tessera/src/bridgeshot.rs
git commit -m "feat(bridgeshot): v2 orchestrator — portal capture, multi-tool, gallery, clipboard export"
```

---

### Task 9: Theme/CSS for the toolbar, swatches, and gallery

**Files:**
- Modify: `crates/tessera/src/theme.rs` (the `css` format string, replacing the old `.bridgeshot-*` block at lines 60-66)

**Interfaces:**
- Consumes: existing `{bg} {fg} {accent} {surface} {border}` interpolation already in scope in `install_css`.

- [ ] **Step 1: Replace the bridgeshot CSS block**

In `crates/tessera/src/theme.rs`, replace the existing bridgeshot lines (the block starting `.bridgeshot-window` and ending with `.bridgeshot-status ... padding: 6px 10px; }}"`) with:

```rust
         .bridgeshot-window {{ background-color: {bg}; }}\n\
         .bridgeshot-canvas {{ background-color: {bg}; }}\n\
         .bridgeshot-toolbar {{ background-color: {surface}; padding: 5px 8px; \
                                border-bottom: 1px solid {border}; }}\n\
         .bridgeshot-toolbar button {{ min-height: 0; padding: 3px 10px; }}\n\
         .bridgeshot-tool:checked {{ background-color: alpha({accent}, 0.30); \
                                     box-shadow: inset 0 0 0 1px {accent}; }}\n\
         .bridgeshot-swatches {{ margin-left: 4px; }}\n\
         .bridgeshot-swatch {{ min-width: 20px; min-height: 20px; padding: 0; \
                               border-radius: 10px; margin: 0 1px; }}\n\
         .bridgeshot-swatch.selected {{ box-shadow: 0 0 0 2px {fg}; }}\n\
         .swatch-0 {{ background-image: none; background-color: #e84d5b; }}\n\
         .swatch-1 {{ background-image: none; background-color: #7aa2f7; }}\n\
         .swatch-2 {{ background-image: none; background-color: #e0af68; }}\n\
         .swatch-3 {{ background-image: none; background-color: #8cc673; }}\n\
         .swatch-4 {{ background-image: none; background-color: #f2f2f7; }}\n\
         .swatch-5 {{ background-image: none; background-color: #1a1c26; }}\n\
         .bridgeshot-gallery {{ background-color: {surface}; }}\n\
         .bridgeshot-thumb {{ padding: 2px; border-radius: 4px; }}\n\
         .bridgeshot-thumb.selected {{ box-shadow: inset 0 0 0 2px {accent}; }}\n\
         .bridgeshot-text-entry {{ background-color: {bg}; color: {fg}; \
                                   box-shadow: inset 0 0 0 1px {accent}; }}\n\
         .bridgeshot-status {{ color: alpha({fg}, 0.7); padding: 6px 10px; }}"
```

(Note: the `swatch-N` colors duplicate `PALETTE` so the swatch backgrounds are visible; keep them in sync if `PALETTE` changes. `background-image: none` overrides the theme's default button gradient so the flat color shows.)

- [ ] **Step 2: Build + visual check**

Run: `cargo run -p tessera` then Alt+P.
Expected: toolbar has a surface background; the active tool is highlighted; color swatches are round and filled, the selected one ringed; gallery thumbnails show a ring when active.

- [ ] **Step 3: Commit**

```bash
git add crates/tessera/src/theme.rs
git commit -m "style(bridgeshot): toolbar, color swatches, gallery thumbnails"
```

---

### Task 10: Final cleanup + full verification

**Files:**
- Modify: each `crates/tessera/src/bridgeshot/*.rs` (remove the temporary `#![allow(dead_code)]` lines now that everything is used)

- [ ] **Step 1: Remove temporary allow attributes**

Delete the `#![allow(dead_code)] // wired up in the orchestrator task` line from `state.rs`, `capture.rs`, `canvas.rs`, `export.rs`, and `gallery.rs`. (Leave any genuinely-unused helper only if the compiler complains; otherwise remove the helper.)

- [ ] **Step 2: Full build + tests + lint**

Run: `cargo test -p tessera 2>&1 | tail -20`
Expected: the 4 `tools` tests pass.

Run: `cargo build -p tessera 2>&1 | tail -30`
Expected: compiles with no warnings from the bridgeshot modules (no dead-code, no unused imports). Remove any leftover unused imports the compiler flags.

Run (if available): `cargo clippy -p tessera 2>&1 | tail -30`
Expected: no new warnings in `bridgeshot/*`.

- [ ] **Step 3: End-to-end manual pass**

Repeat Task 8 Step 3 once more on the final build, plus: confirm the portal-fallback path by reading the status text when you press **Escape**/cancel the COSMIC picker (should read "Capture cancelled or unavailable.", window returns).

- [ ] **Step 4: Commit**

```bash
git add crates/tessera/src/bridgeshot
git commit -m "chore(bridgeshot): drop temporary dead-code allows after wiring"
```

---

## Self-Review

**Spec coverage:**
- Capture anything (portal + region/window) → Task 4 (`capture_screen`) + Task 8 (Capture button). ✓
- Window-snapshot fallback → Task 4 (`capture_screen` falls back) + Task 8 (auto-capture on open). ✓
- Box / Arrow / Text / Pen / Highlight → Task 2 (`Shape`/`Tool`) + Task 5 (input + render). ✓
- Text typed onto the image → Task 5 (`spawn_text_entry`). ✓
- Color picker → Task 2 (`PALETTE`) + Task 8 (swatches) + Task 9 (CSS). ✓
- Left thumbnail gallery, session-only, click-to-switch, per-image annotations → Task 3 (`Doc`/`add_doc`) + Task 7 (`gallery`) + Task 8. ✓
- Toggle gallery like the sidebar (button + Alt+G) → Task 8 (`gallery_btn` + `install_keys`). ✓
- Export saves PNG + copies image to clipboard, status shows path → Task 6 + Task 8. ✓
- Numbered badges + right-side list removed → Task 8 (old code deleted), Task 9 (old CSS replaced). ✓
- Module split → Tasks 2-8. ✓
- `ashpd` without gtk4 feature, no dup gtk4 → Task 1. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full content. ✓

**Type consistency:** `Shot`, `State`, `Doc`, `Drag`, `Annotation`, `Shape`, `Tool`, `PALETTE`, `add_doc`, `to_image`, `to_widget`, `paint_annotations`, `export_png` (returns `(PathBuf, Pixbuf)`), `capture_screen`/`capture_window_async`, `gallery::new`/`Gallery::add_thumb`, `canvas::build`/`CanvasUi{area,overlay}` are used with identical signatures across producer and consumer tasks. ✓

**Deviations from spec (intentional, minor):**
- Module root stays `bridgeshot.rs` (not renamed to `bridgeshot/mod.rs`) — Rust resolves submodules either way; avoids touching `main.rs`.
- Added `state.rs` (separate from `tools.rs`) so `tools.rs` stays GTK-free and unit-testable; `Drag` lives in `state.rs`.
- The shared annotation renderer `paint_annotations` lives in `canvas.rs` (consumed by `export.rs`) rather than a separate `render.rs`.
