//! Tabbed universal file viewer beside the terminals (the center `Paned`'s end
//! child). Each opened file becomes a tab whose content is chosen by file type:
//! text/code uses GtkSourceView (editable, `Ctrl+S` saves); images use GtkPicture
//! with a zoom toolbar, Ctrl+scroll zoom-to-cursor, and left-drag panning; PDFs
//! and office documents are rendered to page images on a worker thread (see
//! `preview`) and shown as a scrollable, zoomable, drag-pannable column;
//! audio/video play inline via GtkVideo; anything else gets an info card with
//! "Open externally". `Esc` or a tab's `×` closes it; closing the last tab hides
//! the panel.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk4::gdk::{Key, ModifierType};
use gtk4::glib::{self, Propagation};
use gtk4::prelude::*;
use gtk4::{
    gio, Align, Box as GtkBox, Button, ContentFit, EventControllerKey, EventControllerMotion,
    EventControllerScroll, EventControllerScrollFlags, EventSequenceState, GestureDrag, Label,
    Notebook, Orientation, Paned, Picture, PolicyType, PropagationPhase, ScrolledWindow, Video,
    Widget,
};
use sourceview5::prelude::*;
use sourceview5::{Buffer, LanguageManager, StyleSchemeManager, View};

use crate::preview;

/// What kind of viewer a file maps to (by extension).
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Image,
    Pdf,
    Office,
    Media,
    Other,
}

fn kind_of(path: &Path) -> Kind {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" | "tif" | "tiff"
        | "xpm" | "pnm" | "tga" | "icns" => Kind::Image,
        "pdf" => Kind::Pdf,
        // Office formats (rendered via soffice -> pdf -> images). CSV stays text.
        "doc" | "docx" | "odt" | "rtf" | "ppt" | "pptx" | "odp" | "xls" | "xlsx" | "ods" => {
            Kind::Office
        }
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
}

impl Editor {
    pub fn new(paned: &Paned) -> Editor {
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

        Editor {
            root: notebook,
            paned: paned.clone(),
            open,
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
                Kind::Image => (image_viewer(path), None, None),
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
        if self.root.parent().is_none() {
            self.paned.set_end_child(Some(&self.root));
        }
        self.root.set_visible(true);
        let w = self.paned.width();
        self.paned.set_position(if w > 200 { w / 2 } else { 700 });
    }
}

/// Editable source view with line numbers + syntax highlighting. Returns `None`
/// if the file isn't valid UTF-8 (binary) so the caller can show the info card.
fn text_viewer(path: &Path) -> Option<(Widget, Buffer)> {
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

/// Image viewer: fit-to-view by default, with a zoom toolbar and Ctrl+scroll to
/// zoom in/out (scrollable once zoomed past the viewport).
fn image_viewer(path: &Path) -> Widget {
    let pic = Picture::for_filename(path);
    pic.set_content_fit(ContentFit::Contain);
    pic.set_can_shrink(true);
    pic.set_halign(Align::Center);
    pic.set_valign(Align::Center);
    pic.set_vexpand(true);
    pic.set_hexpand(true);
    pic.add_css_class("image-view");
    let (iw, ih) = intrinsic_size(&pic, path);

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&pic)
        .build();

    let pic2 = pic.clone();
    wrap_zoomable(&scroller, move |z| match z {
        // Fit: let the picture shrink to the viewport (aspect preserved).
        None => {
            pic2.set_size_request(-1, -1);
            pic2.set_content_fit(ContentFit::Contain);
        }
        // Explicit zoom: render at intrinsic_size * z and let the window scroll.
        Some(z) => {
            pic2.set_content_fit(ContentFit::Fill);
            pic2.set_size_request((iw as f64 * z).round() as i32, (ih as f64 * z).round() as i32);
        }
    })
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

/// Natural pixel size of a loaded picture, falling back to a decoded texture.
fn intrinsic_size(pic: &Picture, path: &Path) -> (i32, i32) {
    pic.paintable()
        .filter(|p| p.intrinsic_width() > 0 && p.intrinsic_height() > 0)
        .map(|p| (p.intrinsic_width(), p.intrinsic_height()))
        .or_else(|| gtk4::gdk::Texture::from_filename(path).ok().map(|t| (t.width(), t.height())))
        .unwrap_or((1, 1))
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
                hadj.set_upper((ohu * ratio).max(ohp));
                hadj.set_value((oh + ax) * ratio - ax);
                vadj.set_upper((ovu * ratio).max(ovp));
                vadj.set_value((ov + ay) * ratio - ay);
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
    let status = Label::new(Some("Rendering…"));
    status.set_margin_top(40);
    status.add_css_class("doc-status");
    column.append(&status);

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

    let (tx, rx) = async_channel::unbounded::<preview::Msg>();
    preview::start_render(path.to_path_buf(), tx, cancel);

    let col = column.clone();
    let st = status.clone();
    let p = path.to_path_buf();
    let mode_rx = mode.clone();
    let sc_rx = scroller.clone();
    glib::spawn_future_local(async move {
        while let Ok(msg) = rx.recv().await {
            match msg {
                preview::Msg::Pages(n) => {
                    st.set_text(&format!("Rendering {n} page{}…", if n == 1 { "" } else { "s" }));
                }
                preview::Msg::Page(page) => {
                    let pic = Picture::for_filename(&page);
                    pic.set_can_shrink(true);
                    pic.set_content_fit(ContentFit::Contain);
                    pic.set_halign(Align::Center);
                    pic.add_css_class("doc-page");
                    size_doc_page(&pic, mode_rx.get(), sc_rx.width());
                    col.append(&pic);
                }
                preview::Msg::Done => st.set_visible(false),
                preview::Msg::Error(e) => {
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
        let _ = std::process::Command::new("xdg-open").arg(&p).spawn();
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
        cancel.store(true, Ordering::Relaxed);
    }
    open.borrow_mut().retain(|of| &of.child != child);
    if open.borrow().is_empty() {
        paned.set_end_child(None::<&Widget>);
    }
}
