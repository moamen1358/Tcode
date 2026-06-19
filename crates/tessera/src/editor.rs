//! Tabbed universal file viewer beside the terminals (the center `Paned`'s end
//! child). Each opened file becomes a tab whose content is chosen by file type:
//! text/code uses GtkSourceView (editable, `Ctrl+S` saves); images render on a
//! free canvas (a `DrawingArea` with a manual transform) — scroll to zoom toward
//! the cursor, left-drag to pan the image freely anywhere, double-click to reset
//! to Fit, plus a zoom toolbar; PDFs and office documents are rendered to page
//! images on a worker thread (see `preview`) and shown as a scrollable, zoomable,
//! drag-pannable column;
//! audio/video play inline via GtkVideo; anything else gets an info card with
//! "Open externally". `Esc` or a tab's `×` closes it; closing the last tab hides
//! the panel.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk4::gdk::prelude::GdkCairoContextExt; // cr.set_source_pixbuf
use gtk4::gdk::{Key, ModifierType};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib::{self, Propagation};
use gtk4::prelude::*;
use gtk4::{
    gio, Align, Box as GtkBox, Button, CenterBox, ContentFit, DrawingArea, EventControllerKey,
    EventControllerMotion, EventControllerScroll, EventControllerScrollFlags, EventSequenceState,
    GestureClick, GestureDrag, Label, Notebook, Orientation, Paned, Picture, PolicyType,
    PropagationPhase, ScrolledWindow, Video, Widget,
};
use sourceview5::prelude::*;
use sourceview5::{Buffer, LanguageManager, StyleSchemeManager, View};

use crate::preview;

/// What kind of viewer a file maps to (by extension).
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Csv,
    Image,
    Pdf,
    Office,
    Media,
    Other,
}

fn kind_of(path: &Path) -> Kind {
    // Office formats are owned by the preview pipeline (soffice -> pdf -> images);
    // reuse its single extension list so the two can't drift.
    if preview::is_office(path) {
        return Kind::Office;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" | "tif" | "tiff"
        | "xpm" | "pnm" | "tga" | "icns" => Kind::Image,
        "pdf" => Kind::Pdf,
        // Tabular text → rendered as a real table (falls back to text if it
        // doesn't parse as a grid).
        "csv" | "tsv" | "tab" => Kind::Csv,
        // Audio + video play inline via GtkVideo (GStreamer backend).
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "wmv" | "flv" | "m4v" | "mpg" | "mpeg" | "ogv"
        | "mp3" | "wav" | "flac" | "ogg" | "oga" | "m4a" | "aac" | "opus" | "wma" | "mid" => {
            Kind::Media
        }
        // Archives / binaries / fonts get the info card (no inline preview).
        "zip" | "tar" | "gz" | "xz" | "zst" | "bz2" | "7z" | "rar" | "exe" | "bin" | "so" | "o"
        | "a" | "dll" | "dylib" | "wasm" | "ttf" | "otf" | "woff" | "woff2" => Kind::Other,
        // Unknown/extensionless default to text, downgraded to the info card if
        // the bytes aren't valid UTF-8.
        _ => Kind::Text,
    }
}

struct OpenFile {
    path: PathBuf,
    /// `Some` only for editable text tabs (so `Ctrl+S` knows what it can save).
    buffer: Option<Buffer>,
    /// The tab's content widget (its identity in the notebook).
    child: Widget,
    /// Cancellation flag for an in-flight document render (Pdf/Office tabs).
    cancel: Option<Arc<AtomicBool>>,
}

type OpenFiles = Rc<RefCell<Vec<OpenFile>>>;

/// Apply an absolute zoom, optionally anchored at a viewport point (`None` =
/// centre). Used by the toolbar, Ctrl+scroll, and keyboard.
type ZoomFn = Rc<dyn Fn(f64, Option<(f64, f64)>)>;

pub struct Editor {
    pub root: Notebook,
    paned: Paned,
    open: OpenFiles,
    /// Backdrop painted behind images, from the theme `surface` color.
    backdrop: (f64, f64, f64),
}

impl Editor {
    pub fn new(paned: &Paned, surface: &str) -> Editor {
        let notebook = Notebook::new();
        notebook.set_scrollable(true);
        notebook.add_css_class("editor");

        let open: OpenFiles = Rc::new(RefCell::new(Vec::new()));

        // Ctrl+S saves the current text tab; Esc closes the current tab.
        {
            let nb = notebook.clone();
            let open_c = open.clone();
            let paned_c = paned.clone();
            let kc = EventControllerKey::new();
            kc.set_propagation_phase(PropagationPhase::Capture);
            kc.connect_key_pressed(move |_c, key, _code, mods| {
                if mods.contains(ModifierType::CONTROL_MASK) && key == Key::s {
                    save_current(&nb, &open_c);
                    Propagation::Stop
                } else if key == Key::Escape {
                    if let Some(cur) = nb.current_page() {
                        let child = open_c
                            .borrow()
                            .iter()
                            .find(|of| nb.page_num(&of.child) == Some(cur))
                            .map(|of| of.child.clone());
                        if let Some(child) = child {
                            close_tab(&nb, &open_c, &paned_c, &child);
                        }
                    }
                    Propagation::Stop
                } else {
                    Propagation::Proceed
                }
            });
            notebook.add_controller(kc);
        }

        let c = crate::theme::rgba(surface);
        Editor {
            root: notebook,
            paned: paned.clone(),
            open,
            backdrop: (c.red() as f64, c.green() as f64, c.blue() as f64),
        }
    }

    /// Open `path` in a tab (focusing it if already open) and reveal the panel.
    pub fn open(&self, path: &Path) {
        if let Some(child) = self
            .open
            .borrow()
            .iter()
            .find(|of| of.path == path)
            .map(|of| of.child.clone())
        {
            if let Some(p) = self.root.page_num(&child) {
                self.root.set_current_page(Some(p));
            }
            self.reveal();
            return;
        }

        let (child, buffer, cancel): (Widget, Option<Buffer>, Option<Arc<AtomicBool>>) =
            match kind_of(path) {
                Kind::Text => match text_viewer(path) {
                    Some((w, b)) => (w, Some(b), None),
                    None => (fallback_viewer(path), None, None),
                },
                // Table view; if it doesn't parse as a grid, fall back to text.
                Kind::Csv => match crate::csvview::csv_viewer(path) {
                    Some(w) => (w, None, None),
                    None => match text_viewer(path) {
                        Some((w, b)) => (w, Some(b), None),
                        None => (fallback_viewer(path), None, None),
                    },
                },
                Kind::Image => (image_viewer(path, self.backdrop), None, None),
                Kind::Pdf | Kind::Office => {
                    let cancel = Arc::new(AtomicBool::new(false));
                    (build_pages(path, cancel.clone()), None, Some(cancel))
                }
                Kind::Media => (media_viewer(path), None, None),
                Kind::Other => (fallback_viewer(path), None, None),
            };

        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let tab = GtkBox::new(Orientation::Horizontal, 6);
        tab.add_css_class("editor-tab");
        let label = Label::new(Some(&name));
        let close = Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        tab.append(&label);
        tab.append(&close);

        let page = self.root.append_page(&child, Some(&tab));
        self.root.set_current_page(Some(page));
        self.open.borrow_mut().push(OpenFile {
            path: path.to_path_buf(),
            buffer,
            child: child.clone(),
            cancel,
        });

        {
            let nb = self.root.clone();
            let open_c = self.open.clone();
            let paned_c = self.paned.clone();
            let child = child.clone();
            close.connect_clicked(move |_| close_tab(&nb, &open_c, &paned_c, &child));
        }

        self.reveal();
        child.grab_focus();
    }

    fn reveal(&self) {
        // Set the split position only on the very first reveal; otherwise opening a
        // second file would snap the divider back to 50%, discarding the width the
        // user dragged.
        let first = self.root.parent().is_none();
        if first {
            self.paned.set_end_child(Some(&self.root));
        }
        self.root.set_visible(true);
        if first {
            let w = self.paned.width();
            self.paned.set_position(if w > 200 { w / 2 } else { 700 });
        }
    }
}

/// Editable source view with line numbers + syntax highlighting. Returns `None`
/// if the file isn't valid UTF-8 (binary) so the caller can show the info card.
fn text_viewer(path: &Path) -> Option<(Widget, Buffer)> {
    // Guard against opening a multi-GB file synchronously (would freeze the UI /
    // exhaust memory); over the cap we show the info card instead.
    const MAX_TEXT_BYTES: u64 = 64 * 1024 * 1024;
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_TEXT_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None; // NUL byte -> binary
    }
    let content = String::from_utf8(bytes).ok()?;

    let buffer = Buffer::new(None);
    if let Some(lang) = LanguageManager::default().guess_language(path.to_str(), None) {
        buffer.set_language(Some(&lang));
    }
    if let Some(scheme) = StyleSchemeManager::default().scheme("Adwaita-dark") {
        buffer.set_style_scheme(Some(&scheme));
    }
    buffer.set_text(&content);

    let view = View::with_buffer(&buffer);
    view.set_show_line_numbers(true);
    view.set_highlight_current_line(true);
    view.set_left_margin(14);
    view.add_css_class("editor-view");
    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&view)
        .build();
    Some((scroller.upcast(), buffer))
}

/// Lower/upper bounds for the free image canvas zoom (1.0 = 100 %).
const MIN_ZOOM: f64 = 0.05;
const MAX_ZOOM: f64 = 20.0;

/// The image canvas transform: `zoom` plus a free top-left offset in widget px.
/// `fit` means "recompute zoom+offset to fit-and-centre on every draw" (so it
/// tracks window resizing) until the user zooms or pans.
#[derive(Clone, Copy)]
struct ImgView {
    zoom: f64,
    off_x: f64,
    off_y: f64,
    fit: bool,
}

/// An in-flight zoom animation: ease `zoom` toward `target` while pinning the
/// image pixel (`ix`,`iy`) under the fixed widget anchor (`ax`,`ay`).
#[derive(Clone, Copy)]
struct Anim {
    target: f64,
    ax: f64,
    ay: f64,
    ix: f64,
    iy: f64,
}

/// Per-image-viewer animation handles, grouped to keep closures tidy.
#[derive(Clone)]
struct Zoomer {
    state: Rc<Cell<ImgView>>,
    anim: Rc<Cell<Option<Anim>>>,
    running: Rc<Cell<bool>>,
    area: DrawingArea,
}

impl Zoomer {
    /// Request a smooth zoom by `factor` toward widget point (`px`,`py`). Chains
    /// onto any running animation's target so rapid scrolls accumulate smoothly.
    fn zoom_to(&self, px: f64, py: f64, factor: f64) {
        let v = self.state.get();
        // Guard against a non-finite scroll delta (some kinetic/trackpad backends)
        // turning the zoom factor into NaN, which would poison the transform.
        if v.zoom <= 0.0 || !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let base = self.anim.get().map(|a| a.target).unwrap_or(v.zoom);
        let target = (base * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let ix = (px - v.off_x) / v.zoom;
        let iy = (py - v.off_y) / v.zoom;
        self.anim.set(Some(Anim {
            target,
            ax: px,
            ay: py,
            ix,
            iy,
        }));
        self.start();
    }

    /// Zoom toward the canvas centre (toolbar ± buttons).
    fn zoom_center(&self, factor: f64) {
        // max(1) so a not-yet-allocated canvas zooms toward a sane point, not (0,0).
        let (w, h) = (
            self.area.width().max(1) as f64,
            self.area.height().max(1) as f64,
        );
        self.zoom_to(w / 2.0, h / 2.0, factor);
    }

    /// Cancel any running zoom animation (e.g. when a pan or Fit takes over).
    fn cancel(&self) {
        self.anim.set(None);
    }

    /// Ensure the per-frame easing tick is running (one at a time).
    fn start(&self) {
        if self.running.replace(true) {
            return;
        }
        // Capture only the Rc state, NOT the DrawingArea — capturing `self` (which
        // holds `area`) would form an area→tick→area cycle, and a detached
        // widget's frame clock stops ticking, so closing a tab mid-animation would
        // leak the canvas forever. The area arrives as the `a` parameter instead.
        let (state, anim, running) = (self.state.clone(), self.anim.clone(), self.running.clone());
        self.area.add_tick_callback(move |a, _clock| {
            let Some(an) = anim.get() else {
                running.set(false);
                return glib::ControlFlow::Break;
            };
            let mut v = state.get();
            // Frame-rate-stable enough easing toward the target (~50 ms settle).
            let next = v.zoom + (an.target - v.zoom) * 0.30;
            let done = (an.target - next).abs() <= an.target * 0.002;
            v.zoom = if done { an.target } else { next };
            v.off_x = an.ax - an.ix * v.zoom;
            v.off_y = an.ay - an.iy * v.zoom;
            v.fit = false;
            state.set(v);
            a.queue_draw();
            if done {
                anim.set(None);
                running.set(false);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
}

/// Image viewer: a free canvas (no scroll clamping). The image floats — scroll
/// zooms toward the cursor, left-drag pans it anywhere, double-click resets to
/// Fit. A toolbar mirrors the zoom controls. Fit is recomputed on resize.
fn image_viewer(path: &Path, backdrop: (f64, f64, f64)) -> Widget {
    let pixbuf = Pixbuf::from_file(path).ok();
    let (iw, ih) = pixbuf
        .as_ref()
        .map(|p| (p.width().max(1) as f64, p.height().max(1) as f64))
        .unwrap_or((1.0, 1.0));

    let area = DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_focusable(true);
    area.add_css_class("image-view");

    let state = Rc::new(Cell::new(ImgView {
        zoom: 1.0,
        off_x: 0.0,
        off_y: 0.0,
        fit: true,
    }));
    let ptr = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    let zoomer = Zoomer {
        state: state.clone(),
        anim: Rc::new(Cell::new(None)),
        running: Rc::new(Cell::new(false)),
        area: area.clone(),
    };

    let pct = Label::new(Some("100%"));
    pct.add_css_class("viewer-zoom-pct");

    // Draw: fill the backdrop, then paint the image under the current transform.
    // In Fit mode the transform is (re)derived here from the live widget size, so
    // resizing the panel keeps the image fitted and centred for free.
    {
        let (state, pct, pb) = (state.clone(), pct.clone(), pixbuf.clone());
        area.set_draw_func(move |_a, cr, w, h| {
            cr.set_source_rgb(backdrop.0, backdrop.1, backdrop.2); // theme surface
            let _ = cr.paint();
            let Some(pb) = pb.as_ref() else {
                cr.set_source_rgb(0.55, 0.6, 0.75);
                cr.set_font_size(15.0);
                let msg = "Could not load image";
                let tw = cr.text_extents(msg).map(|e| e.width()).unwrap_or(0.0);
                cr.move_to((w as f64 - tw) / 2.0, h as f64 / 2.0);
                let _ = cr.show_text(msg);
                return;
            };
            let mut v = state.get();
            if v.fit {
                let z = (w as f64 / iw).min(h as f64 / ih);
                v.zoom = if z.is_finite() && z > 0.0 { z } else { 1.0 };
                v.off_x = (w as f64 - iw * v.zoom) / 2.0;
                v.off_y = (h as f64 - ih * v.zoom) / 2.0;
                state.set(v); // write back so pan/zoom continue from here
            }
            pct.set_text(&format!("{:.0}%", v.zoom * 100.0));

            let _ = cr.save();
            cr.translate(v.off_x, v.off_y);
            cr.scale(v.zoom, v.zoom);
            cr.set_source_pixbuf(pb, 0.0, 0.0);
            let _ = cr.paint();
            let _ = cr.restore();
        });
    }

    // Track the pointer (widget coords) so scroll can zoom toward it.
    let motion = EventControllerMotion::new();
    {
        let ptr = ptr.clone();
        motion.connect_motion(move |_, x, y| ptr.set((x, y)));
    }
    area.add_controller(motion);

    // Scroll (no modifier needed) zooms smoothly toward the pointer. The factor
    // tracks the delta magnitude, so a trackpad gives fine-grained, fluid zoom.
    let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
    {
        let (zoomer, ptr) = (zoomer.clone(), ptr.clone());
        scroll.connect_scroll(move |_c, _dx, dy| {
            let (px, py) = ptr.get();
            zoomer.zoom_to(px, py, 1.15_f64.powf(-dy));
            Propagation::Stop
        });
    }
    area.add_controller(scroll);

    // Left-drag pans the image freely (no clamping — it can float off any edge).
    let drag = GestureDrag::new();
    drag.set_button(gtk4::gdk::BUTTON_PRIMARY);
    let start = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    {
        let (state, start, area_c, zoomer) =
            (state.clone(), start.clone(), area.clone(), zoomer.clone());
        drag.connect_drag_begin(move |g, _x, _y| {
            g.set_state(EventSequenceState::Claimed);
            zoomer.cancel(); // a pan overrides any in-flight zoom animation
            let v = state.get();
            start.set((v.off_x, v.off_y));
            area_c.set_cursor_from_name(Some("grabbing"));
        });
    }
    {
        let (state, start, area_c) = (state.clone(), start.clone(), area.clone());
        drag.connect_drag_update(move |_g, ox, oy| {
            let (x0, y0) = start.get();
            let mut v = state.get();
            v.off_x = x0 + ox;
            v.off_y = y0 + oy;
            v.fit = false;
            state.set(v);
            area_c.queue_draw();
        });
    }
    {
        let area_c = area.clone();
        drag.connect_drag_end(move |_g, _ox, _oy| area_c.set_cursor_from_name(Some("grab")));
    }
    area.add_controller(drag);
    area.set_cursor_from_name(Some("grab"));

    // Double-click resets to Fit.
    let dbl = GestureClick::new();
    {
        let (state, area_c, zoomer) = (state.clone(), area.clone(), zoomer.clone());
        dbl.connect_pressed(move |_g, n, _x, _y| {
            if n == 2 {
                zoomer.cancel();
                let mut v = state.get();
                v.fit = true;
                state.set(v);
                area_c.queue_draw();
            }
        });
    }
    area.add_controller(dbl);

    // Toolbar: −, live %, +, Fit — flat text glyphs, matching the reference style.
    // The bar fills the full width (one uniform dark strip); the cluster centres.
    let cluster = GtkBox::new(Orientation::Horizontal, 2);

    let minus = Button::with_label("\u{2212}"); // − (minus sign)
    minus.add_css_class("flat");
    minus.add_css_class("viewer-zoom-btn");
    minus.set_tooltip_text(Some("Zoom out"));
    {
        let zoomer = zoomer.clone();
        minus.connect_clicked(move |_| zoomer.zoom_center(1.0 / 1.25));
    }

    let plus = Button::with_label("+");
    plus.add_css_class("flat");
    plus.add_css_class("viewer-zoom-btn");
    plus.set_tooltip_text(Some("Zoom in"));
    {
        let zoomer = zoomer.clone();
        plus.connect_clicked(move |_| zoomer.zoom_center(1.25));
    }

    let fit_btn = Button::from_icon_name("view-fullscreen-symbolic");
    fit_btn.add_css_class("flat");
    fit_btn.set_tooltip_text(Some("Fit to window"));
    {
        let (state, area_c, zoomer) = (state.clone(), area.clone(), zoomer.clone());
        fit_btn.connect_clicked(move |_| {
            zoomer.cancel();
            let mut v = state.get();
            v.fit = true;
            state.set(v);
            area_c.queue_draw();
        });
    }

    cluster.append(&minus);
    cluster.append(&pct);
    cluster.append(&plus);
    cluster.append(&fit_btn);

    let bar = CenterBox::new();
    bar.add_css_class("viewer-toolbar");
    bar.set_center_widget(Some(&cluster));

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&bar);
    root.append(&area);
    root.upcast()
}

/// Inline audio/video player (GtkVideo over the GStreamer backend) with built-in
/// play / seek / volume controls. "Open externally" stays available in case the
/// system can't decode the codec.
fn media_viewer(path: &Path) -> Widget {
    let video = Video::new();
    video.set_file(Some(&gio::File::for_path(path)));
    video.set_autoplay(false);
    video.set_loop(false);
    video.set_vexpand(true);
    video.set_hexpand(true);
    video.add_css_class("media-view");

    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let bar = GtkBox::new(Orientation::Horizontal, 8);
    bar.add_css_class("viewer-toolbar");
    let title = Label::new(Some(&name));
    title.set_hexpand(true);
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    bar.append(&title);
    bar.append(&open_externally(path));

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&bar);
    root.append(&video);
    root.upcast()
}

/// Wrap `content` with a zoom toolbar (−, live %, +, Fit) and full mouse control:
/// left-drag pans (hand tool), Ctrl+scroll zooms toward the pointer.
/// `apply(Some(z))` requests an absolute zoom (1.0 = 100 %); `apply(None)` fits.
fn wrap_zoomable(content: &ScrolledWindow, apply: impl Fn(Option<f64>) + 'static) -> Widget {
    content.set_vexpand(true);
    content.set_hexpand(true);

    let apply = Rc::new(apply);
    let zoom = Rc::new(Cell::new(1.0_f64));
    let fit = Rc::new(Cell::new(true));
    let ptr = Rc::new(Cell::new((0.0_f64, 0.0_f64)));

    let bar = GtkBox::new(Orientation::Horizontal, 2);
    bar.add_css_class("viewer-toolbar");
    bar.set_halign(Align::Center);
    let pct = Label::new(Some("Fit"));
    pct.add_css_class("viewer-zoom-pct");

    // Apply an absolute zoom, keeping the point under `anchor` (viewport coords,
    // None = viewport centre) fixed. Anchoring is skipped on the first zoom out
    // of Fit (there is no prior scale to scale from). We set the adjustment's
    // upper ourselves from the known scale ratio so the new value isn't clamped
    // to the stale bounds before the layout pass runs.
    let zoom_to: ZoomFn = {
        let (apply, zoom, fit, pct, content) = (
            apply.clone(),
            zoom.clone(),
            fit.clone(),
            pct.clone(),
            content.clone(),
        );
        Rc::new(move |z: f64, anchor: Option<(f64, f64)>| {
            let z = z.clamp(0.1, 8.0);
            let old_z = zoom.get();
            let was_fit = fit.get();
            zoom.set(z);
            fit.set(false);
            pct.set_text(&format!("{:.0}%", z * 100.0));

            let hadj = content.hadjustment();
            let vadj = content.vadjustment();
            let (oh, ohu, ohp) = (hadj.value(), hadj.upper(), hadj.page_size());
            let (ov, ovu, ovp) = (vadj.value(), vadj.upper(), vadj.page_size());

            apply(Some(z));

            if !was_fit && old_z > 0.0 {
                let ratio = z / old_z;
                let (ax, ay) = anchor.unwrap_or((ohp / 2.0, ovp / 2.0));
                let hu = (ohu * ratio).max(ohp);
                hadj.set_upper(hu);
                hadj.set_value(((oh + ax) * ratio - ax).clamp(0.0, (hu - ohp).max(0.0)));
                let vu = (ovu * ratio).max(ovp);
                vadj.set_upper(vu);
                vadj.set_value(((ov + ay) * ratio - ay).clamp(0.0, (vu - ovp).max(0.0)));
            }
        })
    };

    let minus = Button::from_icon_name("zoom-out-symbolic");
    minus.add_css_class("flat");
    minus.set_tooltip_text(Some("Zoom out"));
    {
        let (zoom_to, zoom) = (zoom_to.clone(), zoom.clone());
        minus.connect_clicked(move |_| zoom_to(zoom.get() / 1.25, None));
    }

    let plus = Button::from_icon_name("zoom-in-symbolic");
    plus.add_css_class("flat");
    plus.set_tooltip_text(Some("Zoom in"));
    {
        let (zoom_to, zoom) = (zoom_to.clone(), zoom.clone());
        plus.connect_clicked(move |_| zoom_to(zoom.get() * 1.25, None));
    }

    let fit_btn = Button::from_icon_name("zoom-fit-best-symbolic");
    fit_btn.add_css_class("flat");
    fit_btn.set_tooltip_text(Some("Fit to window"));
    {
        let (apply, zoom, fit, pct) = (apply.clone(), zoom.clone(), fit.clone(), pct.clone());
        fit_btn.connect_clicked(move |_| {
            zoom.set(1.0);
            fit.set(true);
            pct.set_text("Fit");
            apply(None);
        });
    }

    bar.append(&minus);
    bar.append(&pct);
    bar.append(&plus);
    bar.append(&fit_btn);

    // Track the pointer (viewport coords) so Ctrl+scroll can zoom toward it.
    let motion = EventControllerMotion::new();
    {
        let ptr = ptr.clone();
        motion.connect_motion(move |_, x, y| ptr.set((x, y)));
    }
    content.add_controller(motion);

    // Ctrl+scroll zooms toward the pointer (intercept before the window scrolls).
    let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
    scroll.set_propagation_phase(PropagationPhase::Capture);
    {
        let (zoom_to, zoom, ptr) = (zoom_to.clone(), zoom.clone(), ptr.clone());
        scroll.connect_scroll(move |c, _dx, dy| {
            if c.current_event_state().contains(ModifierType::CONTROL_MASK) {
                let factor = if dy < 0.0 { 1.1 } else { 1.0 / 1.1 };
                zoom_to(zoom.get() * factor, Some(ptr.get()));
                Propagation::Stop
            } else {
                Propagation::Proceed
            }
        });
    }
    content.add_controller(scroll);

    // Left-drag pans the content (hand tool), with a grab / grabbing cursor.
    let drag = GestureDrag::new();
    drag.set_button(gtk4::gdk::BUTTON_PRIMARY);
    let start = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    {
        let (content, start) = (content.clone(), start.clone());
        drag.connect_drag_begin(move |g, _x, _y| {
            g.set_state(EventSequenceState::Claimed);
            start.set((content.hadjustment().value(), content.vadjustment().value()));
            content.set_cursor_from_name(Some("grabbing"));
        });
    }
    {
        let (content, start) = (content.clone(), start.clone());
        drag.connect_drag_update(move |_g, ox, oy| {
            let (h0, v0) = start.get();
            content.hadjustment().set_value(h0 - ox);
            content.vadjustment().set_value(v0 - oy);
        });
    }
    {
        let content = content.clone();
        drag.connect_drag_end(move |_g, _ox, _oy| content.set_cursor_from_name(Some("grab")));
    }
    content.add_controller(drag);
    content.set_cursor_from_name(Some("grab"));

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&bar);
    root.append(content);
    root.upcast()
}

/// A scrollable column of rendered pages for a PDF or office document. The actual
/// rendering happens on a worker thread; pages stream in as they're produced.
fn build_pages(path: &Path, cancel: Arc<AtomicBool>) -> Widget {
    let column = GtkBox::new(Orientation::Vertical, 14);
    column.set_halign(Align::Fill);
    column.set_margin_top(12);
    column.set_margin_bottom(12);
    column.set_margin_start(10);
    column.set_margin_end(10);
    column.add_css_class("doc-view");
    // A spinner + label so an office conversion (soffice can take seconds before
    // the first page) reads as working, not frozen.
    let status_box = GtkBox::new(Orientation::Horizontal, 10);
    status_box.set_halign(Align::Center);
    status_box.set_margin_top(40);
    let spinner = gtk4::Spinner::new();
    spinner.set_size_request(20, 20);
    spinner.start();
    let status = Label::new(Some("Rendering…"));
    status.add_css_class("doc-status");
    status_box.append(&spinner);
    status_box.append(&status);
    column.append(&status_box);

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&column)
        .build();

    // Page sizing: None = fit each page to the panel width; Some(z) = panel * z.
    // Streaming pages adopt the current mode as they arrive.
    let mode: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));

    // Bounded so a slow UI applies backpressure to the worker instead of letting
    // page messages queue without limit; the worker bails if the receiver is gone.
    let (tx, rx) = async_channel::bounded::<preview::Msg>(32);
    preview::start_render(path.to_path_buf(), tx, cancel);

    let col = column.clone();
    let st = status.clone();
    let sb = status_box.clone();
    let sp = spinner.clone();
    let p = path.to_path_buf();
    let mode_rx = mode.clone();
    let sc_rx = scroller.clone();
    glib::spawn_future_local(async move {
        while let Ok(msg) = rx.recv().await {
            match msg {
                preview::Msg::Pages(n) => {
                    st.set_text(&format!(
                        "Rendering {n} page{}…",
                        if n == 1 { "" } else { "s" }
                    ));
                }
                preview::Msg::Page(page) => {
                    if !page.exists() {
                        eprintln!("tessera: preview page vanished: {}", page.display());
                        continue;
                    }
                    let pic = Picture::for_filename(&page);
                    pic.set_can_shrink(true);
                    pic.set_content_fit(ContentFit::Contain);
                    pic.set_halign(Align::Center);
                    pic.add_css_class("doc-page");
                    size_doc_page(&pic, mode_rx.get(), sc_rx.width());
                    col.append(&pic);
                }
                preview::Msg::Done => {
                    sp.stop();
                    sb.set_visible(false);
                }
                preview::Msg::Error(e) => {
                    sp.stop();
                    st.set_text(&format!("Could not render preview:\n{e}"));
                    st.add_css_class("doc-error");
                    col.append(&open_externally(&p));
                }
            }
        }
    });

    let col2 = column.clone();
    // Weak: the scroller owns the scroll controller that holds this closure, so a
    // strong ref here would form a cycle and leak the page subtree on tab close.
    let sc_weak = scroller.downgrade();
    wrap_zoomable(&scroller, move |z| {
        mode.set(z);
        let panel = sc_weak.upgrade().map(|s| s.width()).unwrap_or(800);
        let mut child = col2.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            if let Ok(pic) = c.downcast::<Picture>() {
                size_doc_page(&pic, z, panel);
            }
            child = next;
        }
    })
}

/// Size one document page: `None` fits the page to the panel width; `Some(z)`
/// renders it at `panel_width * z` (overflowing → scrollable).
fn size_doc_page(pic: &Picture, z: Option<f64>, panel: i32) {
    match z {
        None => {
            pic.set_hexpand(true);
            pic.set_size_request(-1, -1);
        }
        Some(z) => {
            pic.set_hexpand(false);
            let w = ((panel.max(200) as f64) * z).round() as i32;
            pic.set_size_request(w.max(120), -1);
        }
    }
}

/// Info card for files we can't preview inline (video, audio, archives, …).
fn fallback_viewer(path: &Path) -> Widget {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let size = std::fs::metadata(path)
        .map(|m| human_size(m.len()))
        .unwrap_or_else(|_| "?".into());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| "FILE".into());

    let card = GtkBox::new(Orientation::Vertical, 10);
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.add_css_class("fallback-card");

    let title = Label::new(Some(&name));
    title.add_css_class("fallback-title");
    let meta = Label::new(Some(&format!("{ext} · {size}")));
    meta.add_css_class("fallback-meta");
    let no_preview = Label::new(Some("No inline preview for this file type"));
    no_preview.add_css_class("fallback-meta");

    card.append(&title);
    card.append(&meta);
    card.append(&no_preview);
    card.append(&open_externally(path));
    card.upcast()
}

fn open_externally(path: &Path) -> Button {
    let btn = Button::with_label("Open externally");
    btn.add_css_class("fallback-open");
    let p = path.to_path_buf();
    btn.connect_clicked(move |_| {
        if let Err(e) = std::process::Command::new("xdg-open").arg(&p).spawn() {
            eprintln!("tessera: 'Open externally' failed (xdg-open): {e}");
        }
    });
    btn
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn save_current(nb: &Notebook, open: &OpenFiles) {
    let Some(cur) = nb.current_page() else { return };
    let files = open.borrow();
    if let Some(of) = files.iter().find(|of| nb.page_num(&of.child) == Some(cur)) {
        if let Some(b) = &of.buffer {
            let text = b.text(&b.start_iter(), &b.end_iter(), false);
            if let Err(e) = std::fs::write(&of.path, text.as_str()) {
                eprintln!("tessera: save failed for {}: {e}", of.path.display());
            }
        }
    }
}

fn close_tab(nb: &Notebook, open: &OpenFiles, paned: &Paned, child: &Widget) {
    if let Some(p) = nb.page_num(child) {
        nb.remove_page(Some(p));
    }
    // Cancel an in-flight render for this tab, if any.
    if let Some(cancel) = open
        .borrow()
        .iter()
        .find(|of| &of.child == child)
        .and_then(|of| of.cancel.clone())
    {
        cancel.store(true, Ordering::Release);
    }
    open.borrow_mut().retain(|of| &of.child != child);
    if open.borrow().is_empty() {
        paned.set_end_child(None::<&Widget>);
    }
}
