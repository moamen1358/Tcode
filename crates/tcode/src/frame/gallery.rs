//! The screenshots tray in Tcode's main window: a vertical, scrollable strip of
//! every saved screenshot (loaded from the cache dir on startup, so history
//! survives restarts), floated against the right edge of the work area on Alt+P.
//! Newest on top. Click a thumbnail to re-open it in the annotation canvas, or
//! drag it onto any terminal to insert its path. Capturing is from the titlebar.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::gdk::{DragAction, Texture};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, DragSource, GestureClick, Orientation};

/// Directory where exported screenshots live.
pub fn shots_dir() -> PathBuf {
    glib::user_cache_dir().join("tcode").join("frame")
}

/// True for a canonical `shot-<digits>.png` filename (our own exports).
fn is_shot_name(name: &str) -> bool {
    name.strip_prefix("shot-")
        .and_then(|s| s.strip_suffix(".png"))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Cap on thumbnails kept in the strip. Bounds the decoded textures retained in
/// RAM — both the startup history load and a long session of new captures; older
/// shots stay on disk in the cache dir, just out of the strip.
const MAX_THUMBS: usize = 60;

pub struct Panel {
    pub root: GtkBox,
    list: GtkBox,
    on_pick: Rc<dyn Fn(PathBuf)>,
}

/// Build the panel. `on_pick` runs when a thumbnail is clicked (with that
/// shot's path). Capturing is started from the window titlebar, not here.
pub fn build(on_pick: Rc<dyn Fn(PathBuf)>) -> Rc<Panel> {
    // The screenshots tray: the far-right column, summoned with Alt+P. Newest on top;
    // fills the column height and scrolls vertically through all shots.
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("shot-tray");

    let list = GtkBox::new(Orientation::Vertical, 6);
    list.add_css_class("frame-gallery");
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
    });
    panel.load_existing_deferred();
    panel
}

impl Panel {
    /// Build a thumbnail widget (drag + click wired) for the shot at `path`, or
    /// `None` if it can't be decoded. Shared by live captures and the startup load.
    fn make_thumb(&self, path: &Path) -> Option<gtk4::Image> {
        // Display through a GtkImage at a fixed pixel size. Unlike GtkPicture,
        // GtkImage does NOT do height-for-width, so every thumbnail is a bounded,
        // uniform square regardless of the capture's aspect — portrait shots no
        // longer balloon and squeeze out the file tree above. Decode larger than
        // shown so the downscale stays crisp.
        let pb = Pixbuf::from_file_at_scale(path, 320, 320, true).ok()?;
        let texture = Texture::for_pixbuf(&pb);
        let img = gtk4::Image::from_paintable(Some(&texture));
        img.set_pixel_size(84);
        img.add_css_class("frame-thumb");
        img.set_tooltip_text(path.file_name().and_then(|n| n.to_str()));
        // A pointer cursor hints the thumbnail is interactive (click or drag).
        img.set_cursor_from_name(Some("pointer"));

        // Drag a thumbnail onto any terminal (or other drop target) — it provides
        // the screenshot file, so the terminal inserts its path. A plain Image (not
        // a Button) is used on purpose: a GtkButton's own click gesture claims the
        // press and the drag never starts, so dragging a thumbnail did nothing.
        let drag = DragSource::new();
        drag.set_actions(DragAction::COPY);
        drag.set_content(Some(&crate::dnd::file_drag_provider(path)));
        {
            let texture = texture.clone();
            drag.connect_drag_begin(move |d, _| d.set_icon(Some(&texture), 0, 0));
        }
        img.add_controller(drag);

        // A plain click (press+release without a drag) re-opens the shot to annotate.
        // GestureClick fires on release and is cancelled if the DragSource claims the
        // sequence first, so click and drag stay mutually exclusive.
        let click = GestureClick::new();
        {
            let on_pick = self.on_pick.clone();
            let path = path.to_path_buf();
            click.connect_released(move |_g, _n, _x, _y| on_pick(path.clone()));
        }
        img.add_controller(click);
        Some(img)
    }

    /// Bound the live tray: drop the oldest (bottom) thumbnails past MAX_THUMBS so
    /// a long session of captures can't grow the retained textures unbounded.
    fn trim_to_cap(&self) {
        while self.list.observe_children().n_items() as usize > MAX_THUMBS {
            match self.list.last_child() {
                Some(oldest) => self.list.remove(&oldest),
                None => break,
            }
        }
    }

    /// Add a thumbnail for a freshly captured screenshot at `path` (newest on top).
    pub fn add_saved(&self, path: PathBuf) {
        let Some(img) = self.make_thumb(&path) else {
            return;
        };
        // Newest on top so a fresh capture is immediately visible; older captures
        // remain below and are reachable by scrolling the tray.
        self.list.prepend(&img);
        self.trim_to_cap();
    }

    /// Add a historical thumbnail at the BOTTOM (used by the startup load, which
    /// feeds newest-first). Appending — rather than prepending — keeps the column
    /// newest-on-top while leaving the very top free for a live capture taken
    /// mid-load: it prepends and so stays above all the back-filled history.
    fn add_existing(&self, path: &Path) {
        let Some(img) = self.make_thumb(path) else {
            return;
        };
        self.list.append(&img);
        self.trim_to_cap();
    }

    /// Scan the cache dir for saved shot-*.png paths, sorted oldest first.
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
                // Regular files only: skip symlinks/dirs so a planted shot-*.png
                // symlink can't be decoded or dragged out as an arbitrary path.
                // (DirEntry::file_type doesn't traverse the symlink itself.)
                if !e.file_type().ok()?.is_file() {
                    return None;
                }
                let p = e.path();
                // Strict shot-<digits>.png, not just a "shot-" prefix.
                if !is_shot_name(p.file_name()?.to_str()?) {
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
    /// appends (via `add_existing`), so the column stays newest-on-top AND a live
    /// capture taken mid-load — which prepends — keeps the very top.
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
                    Some(p) => panel.add_existing(&p),
                    None => return glib::ControlFlow::Break,
                }
            }
            glib::ControlFlow::Continue
        });
    }
}
