//! The screenshots tray in Tcode's main window: a vertical, scrollable strip of
//! every screenshot in the user's `Pictures/Screenshots` — Tcode's own annotated
//! captures AND screenshots taken by any other tool — loaded on startup and kept
//! live by a directory watcher, floated against the right edge of the work area on
//! Alt+P. Newest on top. Click a thumbnail to re-open it in the annotation canvas,
//! or drag it onto any terminal to insert its path. Capture is from the titlebar.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::gdk::prelude::GdkCairoContextExt; // cr.set_source_pixbuf
use gtk4::gdk::{DragAction, Texture};
use gtk4::gdk_pixbuf::{InterpType, Pixbuf};
use gtk4::prelude::*;
use gtk4::{gio, glib, Box as GtkBox, DragSource, DrawingArea, GestureClick, Orientation};

/// The screenshots folder this tray shows and saves into: the user's
/// `Pictures/Screenshots` (where COSMIC/GNOME and other tools save), so captures from
/// any tool appear here and Tcode's annotated saves land alongside them. Falls back to
/// a private cache dir only if the Pictures location can't be resolved.
pub fn shots_dir() -> PathBuf {
    if let Some(pics) = glib::user_special_dir(glib::UserDirectory::Pictures) {
        return pics.join("Screenshots");
    }
    glib::user_cache_dir().join("tcode").join("frame")
}

/// True for a file the tray can thumbnail (any raster image, by extension) — so it
/// shows screenshots from any tool, not just Tcode's own `shot-N.png` exports.
fn is_image_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif")
    )
}

/// Cap on thumbnails kept in the strip. Bounds the decoded textures retained in
/// RAM — both the startup history load and a long session of new captures; older
/// shots stay on disk in the folder, just out of the strip.
const MAX_THUMBS: usize = 60;

pub struct Panel {
    pub root: GtkBox,
    list: GtkBox,
    on_pick: Rc<dyn Fn(PathBuf)>,
    /// File names already shown, so the directory watcher and the capture flow can't
    /// add the same screenshot twice.
    seen: RefCell<HashSet<OsString>>,
    /// Live watch on the screenshots folder; held here so it stays armed for the
    /// panel's lifetime (a dropped `FileMonitor` stops delivering events).
    monitor: RefCell<Option<gio::FileMonitor>>,
}

/// Build the panel. `on_pick` runs when a thumbnail is clicked (with that
/// shot's path). Capturing is started from the window titlebar, not here.
pub fn build(on_pick: Rc<dyn Fn(PathBuf)>) -> Rc<Panel> {
    // The screenshots tray: the far-right column, summoned with Alt+P. Newest on top;
    // fills the column height and scrolls vertically through all shots.
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("shot-tray");
    // Fixed width inside the work-area Box: never expand to steal terminal space.
    root.set_hexpand(false);

    // Spacing is 0 here ON PURPOSE: the gap between thumbnails comes from CSS margins
    // on `.frame-thumb` (4.5px, exactly like `.pane`) plus the gallery's CSS padding
    // (4.5px, exactly like `.grid-root`) — mirroring the terminal grid's mechanism so
    // the gaps render IDENTICALLY to the inter-terminal gaps at any UI zoom. A code-set
    // GtkBox spacing scaled differently from the panes' CSS gaps, so they didn't match.
    let list = GtkBox::new(Orientation::Vertical, 0);
    list.add_css_class("frame-gallery");
    list.set_hexpand(true);
    // A column of thumbnails (84px + spacing); scroll vertically for the rest.
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .width_request(120)
        .build();
    root.append(&scroll);

    let panel = Rc::new(Panel {
        root,
        list,
        on_pick,
        seen: RefCell::new(HashSet::new()),
        monitor: RefCell::new(None),
    });
    panel.load_existing_deferred();
    panel.start_watch();
    panel
}

impl Panel {
    /// Build a thumbnail widget (drag + click wired) for the shot at `path`, or
    /// `None` if it can't be decoded. Shared by live captures and the startup load.
    fn make_thumb(&self, path: &Path) -> Option<gtk4::Widget> {
        // Centre-crop every capture to ONE fixed tile shape, then paint it into a
        // fixed-height DrawingArea. A DrawingArea is EXACTLY the size we set (a fixed
        // content height, width filling the column) — unlike GtkPicture, whose natural
        // height is the texture's, so in a vertical box it balloons and stacks dead
        // letterbox space above and below every image (the doubled gap between tiles).
        // The draw covers the tile, so it always fills edge-to-edge with no dead bars.
        // The click still opens the full, uncropped screenshot. Decode bounded to 480px
        // so a 4K capture isn't fully decoded per thumbnail.
        const TW: i32 = 240;
        const TH: i32 = 150; // 16:10 source crop
        const THUMB_H: i32 = 70; // fixed on-screen tile height (px)
        let src = Pixbuf::from_file_at_scale(path, 480, 480, true).ok()?;
        let (sw, sh) = (src.width(), src.height());
        if sw < 1 || sh < 1 {
            return None;
        }
        let target = f64::from(TW) / f64::from(TH);
        let (cw, ch) = if f64::from(sw) / f64::from(sh) > target {
            (((f64::from(sh)) * target).round() as i32, sh) // too wide → trim the sides
        } else {
            (sw, ((f64::from(sw)) / target).round() as i32) // too tall → trim top/bottom
        };
        let cw = cw.clamp(1, sw);
        let ch = ch.clamp(1, sh);
        let cropped = src.new_subpixbuf((sw - cw) / 2, (sh - ch) / 2, cw, ch);
        let scaled = cropped.scale_simple(TW, TH, InterpType::Bilinear)?;
        let texture = Texture::for_pixbuf(&scaled);

        // A fixed-height DrawingArea: exactly THUMB_H tall, width filling the column,
        // so the tray is a uniform stack with ONE 9px gap between tiles (no ballooning).
        let area = DrawingArea::new();
        area.set_content_height(THUMB_H);
        area.set_hexpand(true);
        area.add_css_class("frame-thumb");
        area.set_tooltip_text(path.file_name().and_then(|n| n.to_str()));
        // Tag the widget with its file name so trim_to_cap can prune the matching
        // `seen` entry when this tile is dropped (otherwise `seen` grows unbounded).
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            area.set_widget_name(name);
        }
        // A pointer cursor hints the thumbnail is interactive (click or drag).
        area.set_cursor_from_name(Some("pointer"));
        {
            let pb = scaled.clone();
            area.set_draw_func(move |_a, cr, w, h| {
                let (pw, ph) = (f64::from(pb.width()), f64::from(pb.height()));
                if pw <= 0.0 || ph <= 0.0 {
                    return;
                }
                let (wf, hf) = (f64::from(w), f64::from(h));
                let s = (wf / pw).max(hf / ph); // cover: fill the tile, crop overflow
                cr.rectangle(0.0, 0.0, wf, hf);
                cr.clip();
                cr.translate((wf - pw * s) / 2.0, (hf - ph * s) / 2.0);
                cr.scale(s, s);
                cr.set_source_pixbuf(&pb, 0.0, 0.0);
                let _ = cr.paint();
            });
        }

        // Drag a thumbnail onto any terminal (or other drop target) — it provides the
        // screenshot file, so the terminal inserts its path.
        let drag = DragSource::new();
        drag.set_actions(DragAction::COPY);
        drag.set_content(Some(&crate::dnd::file_drag_provider(path)));
        {
            let texture = texture.clone();
            drag.connect_drag_begin(move |d, _| d.set_icon(Some(&texture), 0, 0));
        }
        area.add_controller(drag);

        // A plain click (press+release without a drag) re-opens the shot to annotate.
        // GestureClick fires on release and is cancelled if the DragSource claims the
        // sequence first, so click and drag stay mutually exclusive.
        let click = GestureClick::new();
        {
            let on_pick = self.on_pick.clone();
            let path = path.to_path_buf();
            click.connect_released(move |_g, _n, _x, _y| on_pick(path.clone()));
        }
        area.add_controller(click);
        Some(area.upcast())
    }

    /// Bound the live tray: drop the oldest (bottom) thumbnails past MAX_THUMBS so
    /// a long session of captures can't grow the retained textures unbounded.
    fn trim_to_cap(&self) {
        while self.list.observe_children().n_items() as usize > MAX_THUMBS {
            match self.list.last_child() {
                Some(oldest) => {
                    // Drop this tile's file name from `seen` too, so the set tracks only
                    // what's shown. widget_name carries the name (set in make_thumb); an
                    // untagged widget reports its type name, which is harmlessly absent.
                    let name = oldest.widget_name();
                    self.seen.borrow_mut().remove(OsStr::new(name.as_str()));
                    self.list.remove(&oldest);
                }
                None => break,
            }
        }
    }

    /// Add a thumbnail for `path` unless it's already shown. `prepend` puts it on top
    /// (live captures + new external screenshots); otherwise it's appended (the startup
    /// back-fill, which feeds newest-first). Dedup is keyed by file name, so the watcher
    /// and the capture flow can't add the same screenshot twice.
    fn add_path(&self, path: &Path, prepend: bool) {
        let Some(name) = path.file_name().map(|n| n.to_os_string()) else {
            return;
        };
        if self.seen.borrow().contains(&name) {
            return;
        }
        let Some(img) = self.make_thumb(path) else {
            return; // unreadable (e.g. still being written) — a later watch event retries
        };
        if prepend {
            // Newest on top so a fresh capture is immediately visible; older captures
            // remain below and are reachable by scrolling the tray.
            self.list.prepend(&img);
        } else {
            // Appended below the back-fill so the column stays newest-on-top while the
            // very top is left free for a live capture taken mid-load (which prepends).
            self.list.append(&img);
        }
        self.seen.borrow_mut().insert(name);
        self.trim_to_cap();
    }

    /// Add a freshly captured/exported (or externally added) screenshot, newest on top.
    pub fn add_saved(&self, path: PathBuf) {
        self.add_path(&path, true);
    }

    /// Watch the screenshots folder so a capture from ANY tool — or Tcode's own export —
    /// appears in the tray live, without a restart. The monitor is stored on the panel
    /// so it stays armed for the panel's lifetime.
    fn start_watch(self: &Rc<Self>) {
        let dir = gio::File::for_path(shots_dir());
        let Ok(monitor) =
            dir.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        else {
            return;
        };
        let weak = Rc::downgrade(self);
        monitor.connect_changed(move |_m, file, _other, event| {
            use gio::FileMonitorEvent as E;
            // Created/ChangesDoneHint cover writes (a file may be partial on Created and
            // complete on ChangesDoneHint); MovedIn covers tools that write-then-rename.
            // add_path's seen-set keeps the two events for one file from doubling it.
            if !matches!(event, E::Created | E::ChangesDoneHint | E::MovedIn) {
                return;
            }
            let (Some(panel), Some(path)) = (weak.upgrade(), file.path()) else {
                return;
            };
            if is_image_file(&path) && path.is_file() {
                panel.add_path(&path, true); // newest on top
            }
        });
        *self.monitor.borrow_mut() = Some(monitor);
    }

    /// Scan the screenshots folder for image files, sorted oldest first.
    ///
    /// This is the cheap part of loading history: a directory scan plus a sort
    /// by mtime. It deliberately does NOT decode any thumbnails — that work is
    /// done later, incrementally, by `load_existing_deferred`.
    fn scan_shots() -> Vec<PathBuf> {
        let dir = shots_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut shots: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                // Regular files only: skip symlinks/dirs so a planted image
                // symlink can't be decoded or dragged out as an arbitrary path.
                // (DirEntry::file_type doesn't traverse the symlink itself.)
                if !e.file_type().ok()?.is_file() {
                    return None;
                }
                let p = e.path();
                // Any raster image, so screenshots from any tool show up — not just
                // Tcode's own shot-N.png exports.
                if !is_image_file(&p) {
                    return None;
                }
                let t = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                Some((t, p))
            })
            .collect();
        shots.sort_by_key(|(t, _)| *t);
        shots.into_iter().map(|(_, p)| p).collect()
    }

    /// Populate the strip with every saved screenshot, decoding a few per idle
    /// tick so a large history doesn't block startup. Loads newest-first and
    /// appends (via `add_path(.., false)`), so the column stays newest-on-top AND a
    /// live capture taken mid-load — which prepends — keeps the very top.
    fn load_existing_deferred(self: &Rc<Self>) {
        let mut shots = Self::scan_shots();
        // Only the newest MAX_THUMBS are loaded — decoding and retaining every shot
        // ever taken would grow RAM without bound on each startup. scan_shots is
        // oldest-first, so drop the oldest excess from the front.
        if shots.len() > MAX_THUMBS {
            shots.drain(0..shots.len() - MAX_THUMBS);
        }
        if shots.is_empty() {
            return;
        }
        let panel = self.clone();
        // Newest-first (scan_shots is oldest-first) so each append lands below the
        // previous: the newest ends on top, older shots back-fill downward — and a
        // concurrent live capture's prepend is never pushed below this history.
        let mut iter = shots.into_iter().rev();
        glib::idle_add_local(move || {
            for _ in 0..4 {
                match iter.next() {
                    Some(p) => panel.add_path(&p, false),
                    None => return glib::ControlFlow::Break,
                }
            }
            glib::ControlFlow::Continue
        });
    }
}
