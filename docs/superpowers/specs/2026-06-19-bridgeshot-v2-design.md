# BridgeShot v2 — Design

**Date:** 2026-06-19
**Component:** `crates/tessera/src/bridgeshot*`
**Status:** Approved design, pending spec review

## 1. Goal

Turn BridgeShot from a Tessera-window-only, numbered-box annotator into a general
screenshot tool, modelled on bridgemind.ai/products/bridgeshot:

- **Capture anything** on screen (any app, window, or a drag-selected region) — not
  just Tessera's own window.
- **Annotate** with boxes, arrows, freehand pen, highlighter, and **text typed
  directly onto the image**.
- **Pick a color** per annotation from a palette.
- A **left-side thumbnail gallery** of this session's captures, **toggleable** on/off
  exactly like the existing file sidebar (header button + keybinding), with each
  thumbnail switching the canvas to that image (annotations preserved per image).
- **Export** the annotated image: save a PNG and copy the **image itself** to the
  clipboard for pasting into chats.

## 2. Non-goals (YAGNI)

- No moving/resizing/selecting existing annotations. To change one, Undo and redraw.
- No editing committed text in place (Undo + retype).
- No per-annotation thickness UI (fixed widths per tool).
- No persistent cross-session history (gallery is session-only, in memory).
- No multi-format clipboard negotiation (clipboard gets the image; the saved path is
  shown in the status bar).

## 3. Capture (the central change)

Wayland/COSMIC blocks apps from grabbing other windows directly, so we use the
**desktop screenshot portal** `org.freedesktop.portal.Screenshot` via the **`ashpd`**
crate (`0.13`, pure Rust, no new system libraries — talks D-Bus over its own socket).

Request with `interactive(true)`: COSMIC shows **its own native screenshot picker**,
where the user chooses whole-screen / a specific window / a drag region. The portal
returns a `file://` URI to a PNG, which we load as a `Pixbuf` and append as a new
gallery doc.

**Flow:**
1. `Capture` clicked → hide the BridgeShot window (so it isn't in the shot).
2. `glib::spawn_future_local` runs the async portal request:
   ```rust
   let uri = Screenshot::request()
       .interactive(true)
       .modal(true)
       .send().await?
       .response()?
       .uri().to_owned();
   ```
   zbus drives its own I/O reactor; the future is awaited on the GTK main context —
   the standard ashpd + GTK4 pattern, so no extra worker thread is required.
3. Convert URI → path, `Pixbuf::from_file`, append doc, select it, re-show window.
4. **Fallback:** if the portal request errors (portal absent), fall back to the
   existing Tessera self-snapshot (`capture_window_async`) so capture never dead-ends.

`ashpd` is added **without** its `gtk4` feature (that feature may pin a different
gtk4 version and cause a duplicate-crate conflict). The parent window identifier is
omitted (`WindowIdentifier::default()`); the interactive picker is compositor-driven,
so parenting is unnecessary.

**Kept:** `Open…` (FileDialog) and drag-and-drop an image onto the canvas — both
append a doc the same way.

## 4. Annotation data model (`bridgeshot/tools.rs`)

Replaces the single `Anno` struct.

```rust
#[derive(Clone, Copy, PartialEq)]
pub enum Tool { Box, Arrow, Text, Pen, Highlight }

pub type Rgb = (f64, f64, f64);

/// Toolbar palette, drawn as color swatches.
pub const PALETTE: &[Rgb] = &[
    (0.91, 0.30, 0.36), // red
    (0.48, 0.64, 0.97), // blue   (existing ACCENT)
    (0.88, 0.69, 0.41), // yellow (existing YELLOW)
    (0.55, 0.78, 0.45), // green
    (0.95, 0.95, 0.97), // white
    (0.10, 0.11, 0.15), // near-black
];

pub enum Shape {
    Box   { x: f64, y: f64, w: f64, h: f64 },
    Arrow { x0: f64, y0: f64, x1: f64, y1: f64 },
    Text  { x: f64, y: f64, content: String, size: f64 },
    Stroke{ points: Vec<(f64, f64)>, highlight: bool }, // pen=false, highlighter=true
}

pub struct Annotation { pub shape: Shape, pub color: Rgb }
```

All geometry is stored in **image space** (so it survives canvas resize and exports at
full resolution). Numbered badges and the right-side label list are **removed**.

## 5. State & per-image documents

```rust
pub struct Doc {
    pub pixbuf: Pixbuf,            // full-resolution image
    pub annos:  Vec<Annotation>,  // this image's annotations
    pub thumb:  Pixbuf,           // ~130px-wide thumbnail for the gallery
}

pub struct State {
    pub docs:   Vec<Doc>,
    pub active: Option<usize>,     // index into docs
    pub tool:   Tool,              // current tool
    pub color:  Rgb,               // current color
    pub drag:   Option<Drag>,      // in-progress annotation (image space)
    pub scale:  f64,
    pub off_x:  f64,
    pub off_y:  f64,
    pub exports: u32,
}

/// In-progress input, before commit.
pub enum Drag {
    Rect   { x0: f64, y0: f64, x1: f64, y1: f64 }, // box & arrow
    Stroke { points: Vec<(f64, f64)>, highlight: bool },
}
```

`active` indexes the doc shown on the canvas. Switching gallery thumbnails just changes
`active` and queues a redraw.

## 6. Tools & interaction (`bridgeshot/canvas.rs`)

A `DrawingArea` renders the active doc's image + its annotations + any in-progress drag
preview. A `GestureDrag` handles pointer input, dispatched by the active `Tool`:

- **Box / Arrow:** drag begin/update set `Drag::Rect`; drag end commits a `Box` or
  `Arrow` annotation in the current color (min size threshold as today). Arrow renders
  as a line plus a filled triangular head at `(x1,y1)`.
- **Pen / Highlight:** drag accumulates points into `Drag::Stroke`; commit pushes a
  `Stroke`. Highlighter uses a thick width (~14px image-space) and ~35% alpha; pen uses
  ~3px, full alpha.
- **Text:** the canvas is wrapped in a `gtk4::Overlay` whose overlay child is a
  `gtk4::Fixed`. On a click with the Text tool, a `gtk4::Entry` is `fixed.put()` at the
  click point and focused. **Enter** or focus-out commits its text as a `Text`
  annotation (image-space position, current color) and removes the Entry; **Escape**
  cancels.

Line widths divide by `scale` on the canvas so on-screen strokes track the export.

**Undo** pops the active doc's last annotation. **Clear** empties the active doc's
annotations. Both queue a redraw.

## 7. Layout, gallery & toggle (`bridgeshot/mod.rs`, `bridgeshot/gallery.rs`)

```
HeaderBar:   [▣ Gallery]  Capture  Open…                    Undo  Clear  [Export]
Toolbar row: [Box][Arrow][Text][Pen][Highlight]      ●red ●blue ●yellow ●green ●… 
┌────────────┬──────────────────────────────────────────────┐
│  Gallery   │                                              │
│  thumbs    │   Canvas (Overlay: DrawingArea + Fixed)       │
│  (vertical,│   image + annotations + text-entry layer      │
│  click to  │                                              │
│  switch)   │                                              │
└────────────┴──────────────────────────────────────────────┘
Status bar
```

- **Toolbar row:** a `GtkBox` directly under the titlebar holding the tool toggle-button
  group (one active at a time) and the color swatches. Selecting a swatch sets
  `state.color`; the active tool and color are visually indicated.
- **Gallery** (`bridgeshot/gallery.rs`): a vertical scrollable strip of thumbnail
  buttons, one per doc, newest at the top (or bottom — implementer's call, top is
  natural). Click → set `active`, redraw, highlight the selected thumbnail. New
  captures/opens append a thumbnail and auto-select it.
- **Toggle:** a `ToggleButton` (icon `sidebar-show-symbolic`) packed at the **start** of
  the header, mirroring the main window's sidebar button (`app.rs:52-57,89-97`). It
  shows/hides the gallery's `ScrolledWindow` via `set_visible(btn.is_active())`,
  defaulting to visible.

## 8. Color picker

Preset swatches from `PALETTE` in the toolbar row (no custom RGB dialog in v1). The
current color applies to the next annotation drawn. Default color: blue (existing
accent).

## 9. Export (`bridgeshot/export.rs`)

Render the **active doc** (image + its annotations) to a full-resolution
`cairo::ImageSurface`, then:

1. Save to `~/.cache/tessera/bridgeshot/shot-N.png` (unchanged location/scheme).
2. **Copy the image to the clipboard** as a `gdk::Texture`
   (`gdk::Texture::for_pixbuf` of the rendered result → `clipboard.set_texture`), so it
   pastes straight into chats.
3. Status bar shows the saved path (no longer the clipboard's text payload).

The annotation-drawing code is shared between live canvas rendering and export by
factoring a `draw_annotations(cr, &[Annotation], scale)` helper used by both.

## 10. Module structure

The current 545-line `bridgeshot.rs` is split into a folder (it has outgrown one file):

| File | Responsibility |
|------|----------------|
| `bridgeshot/mod.rs` | `launch()`, window/header/toolbar assembly, state, wiring |
| `bridgeshot/tools.rs` | `Tool`, `Shape`, `Annotation`, `Drag`, `PALETTE`, `Rgb` |
| `bridgeshot/canvas.rs` | `DrawingArea` render + per-tool gesture handling + text overlay |
| `bridgeshot/gallery.rs` | thumbnail strip build/update/select |
| `bridgeshot/capture.rs` | portal capture (`ashpd`) + existing window self-snapshot fallback |
| `bridgeshot/export.rs` | render-to-PNG + clipboard texture |

`main.rs` keeps `mod bridgeshot;` (now resolving to the folder). `app.rs` and
`keys.rs` call sites for launching BridgeShot are unchanged.

## 11. Dependencies

Add to `crates/tessera/Cargo.toml`:
```toml
# XDG screenshot portal (capture any window/region on Wayland/COSMIC).
# Pure Rust (zbus over D-Bus); no new system libs. gtk4 feature intentionally
# omitted to avoid pulling a second gtk4 version.
ashpd = "0.13"
```
No new system packages. `zbus` + an async-io runtime arrive transitively.

## 12. Keybindings (BridgeShot window-local)

A `gtk4::EventControllerKey` on the BridgeShot window:

| Key | Action |
|-----|--------|
| `Alt+G` | toggle gallery |
| `B / A / T / P / H` | select Box / Arrow / Text / Pen / Highlight tool |
| `Ctrl+Z` | undo last annotation |
| `Escape` | cancel an in-progress text entry |

(Tool letters are skipped while a text Entry has focus.)

## 13. Risks & mitigations

- **ashpd ↔ glib executor:** awaiting the portal future under `glib::spawn_future_local`
  is the documented ashpd+GTK approach; zbus runs its own reactor. If it misbehaves,
  fall back to running the request on a `std::thread` and delivering the path via the
  already-present `async-channel`.
- **Portal absent / denied:** caught and routed to the Tessera self-snapshot fallback;
  status bar reports it.
- **Duplicate gtk4 via ashpd:** avoided by not enabling ashpd's `gtk4` feature.
- **Session memory:** many full-resolution captures accumulate in `docs`; acceptable for
  a session tool. Documented, not optimized.

## 14. Verification

- `cargo build -p tessera` succeeds with `ashpd` added (no duplicate gtk4).
- Manual: Capture → COSMIC picker → region/window/screen lands on canvas as a new
  gallery doc; each tool draws in the selected color; text types onto the image; gallery
  toggles via button and `Alt+G`; switching thumbnails preserves per-image annotations;
  Export writes the PNG and the image pastes from the clipboard.
