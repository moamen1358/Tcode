//! The persistent left-side screenshots panel in Tessera's main window: a
//! scrollable strip of every saved screenshot (loaded from the cache dir on
//! startup, so history survives restarts), showing ~3 at a time. Clicking a
//! thumbnail re-opens it in the annotation canvas; capturing is from the titlebar.

use std::path::PathBuf;
use std::rc::Rc;

use gtk4::gdk::{DragAction, Texture};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, DragSource, Orientation, Separator};

/// Directory where exported screenshots live.
pub fn shots_dir() -> PathBuf {
    glib::user_cache_dir().join("tessera").join("bridgeshot")
}

pub struct Panel {
    pub root: GtkBox,
    list: GtkBox,
    on_pick: Rc<dyn Fn(PathBuf)>,
}

/// Build the panel. `on_pick` runs when a thumbnail is clicked (with that
/// shot's path). Capturing is started from the window titlebar, not here.
pub fn build(on_pick: Rc<dyn Fn(PathBuf)>) -> Rc<Panel> {
    // A compact strip of recent screenshots at the bottom of the file sidebar,
    // set off from the file tree above by a divider.
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("shots-section");
    root.append(&Separator::new(Orientation::Horizontal));

    let list = GtkBox::new(Orientation::Vertical, 6);
    list.add_css_class("bridgeshot-gallery");
    list.set_margin_top(6);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);
    // Show ~3 thumbnails at a time; scroll the strip to reach the rest.
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .height_request(290)
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
    /// Append a thumbnail for a saved screenshot at `path` (newest at the bottom).
    pub fn add_saved(&self, path: PathBuf) {
        // Display through a GtkImage at a fixed pixel size. Unlike GtkPicture,
        // GtkImage does NOT do height-for-width, so every thumbnail is a bounded,
        // uniform square regardless of the capture's aspect — portrait shots no
        // longer balloon and squeeze out the file tree above. Decode larger than
        // shown so the downscale stays crisp.
        let Ok(pb) = Pixbuf::from_file_at_scale(&path, 320, 320, true) else {
            return;
        };
        let texture = Texture::for_pixbuf(&pb);
        let img = gtk4::Image::from_paintable(Some(&texture));
        img.set_pixel_size(84);
        let btn = Button::builder().child(&img).build();
        btn.add_css_class("bridgeshot-thumb");
        btn.set_tooltip_text(path.file_name().and_then(|n| n.to_str()));

        // Drag a thumbnail into any terminal (or other drop target) — it provides
        // the screenshot file, so the terminal inserts its path.
        let drag = DragSource::new();
        drag.set_actions(DragAction::COPY);
        drag.set_content(Some(&crate::dnd::file_drag_provider(&path)));
        {
            let texture = texture.clone();
            drag.connect_drag_begin(move |d, _| d.set_icon(Some(&texture), 0, 0));
        }
        btn.add_controller(drag);

        let on_pick = self.on_pick.clone();
        btn.connect_clicked(move |_| on_pick(path.clone()));
        // Newest at the top so a fresh capture is immediately visible; older
        // captures remain below and are reachable by scrolling the strip.
        self.list.prepend(&btn);
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
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|x| x.to_str()) == Some("png")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("shot-"))
            })
            .map(|p| {
                let t = std::fs::metadata(&p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                (t, p)
            })
            .collect();
        shots.sort_by_key(|(t, _)| *t);
        shots.into_iter().map(|(_, p)| p).collect()
    }

    /// Populate the strip with every saved screenshot, decoding a few per idle
    /// tick so a large history doesn't block startup. `scan_shots` is oldest-first
    /// and `add_saved` prepends, so the newest ends up on top.
    fn load_existing_deferred(self: &Rc<Self>) {
        let shots = Self::scan_shots();
        if shots.is_empty() {
            return;
        }
        let panel = self.clone();
        let mut iter = shots.into_iter();
        glib::idle_add_local(move || {
            for _ in 0..4 {
                match iter.next() {
                    Some(p) => panel.add_saved(p),
                    None => return glib::ControlFlow::Break,
                }
            }
            glib::ControlFlow::Continue
        });
    }
}
