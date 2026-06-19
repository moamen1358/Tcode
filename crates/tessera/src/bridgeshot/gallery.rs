//! The persistent left-side screenshots panel in Tessera's main window: a strip
//! of the most recent saved screenshots (loaded from the cache dir on startup,
//! so history survives restarts). Clicking a thumbnail re-opens it in the
//! annotation canvas; capturing is triggered from the window titlebar.

use std::path::PathBuf;
use std::rc::Rc;

use gtk4::gdk::{ContentProvider, DragAction, Texture};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{gio, Box as GtkBox, Button, DragSource, Orientation, Separator};

/// Only the most recent N screenshots are kept on screen (no scrolling).
const MAX_SHOTS: usize = 3;

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
    // Only the most recent few screenshots are shown — sized to content, no scroll.
    root.append(&list);

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
        let file = gio::File::for_path(&path);
        drag.set_content(Some(&ContentProvider::for_value(&file.to_value())));
        {
            let texture = texture.clone();
            drag.connect_drag_begin(move |d, _| d.set_icon(Some(&texture), 0, 0));
        }
        btn.add_controller(drag);

        let on_pick = self.on_pick.clone();
        btn.connect_clicked(move |_| on_pick(path.clone()));
        // Newest at the top so a fresh capture is immediately visible.
        self.list.prepend(&btn);

        // Keep only the MAX_SHOTS most recent thumbnails: the prepend above put
        // the newest first, so drop anything past the limit (older captures stay
        // on disk, they're just no longer shown).
        let mut shown = 0;
        let mut child = self.list.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            shown += 1;
            if shown > MAX_SHOTS {
                self.list.remove(&c);
            }
            child = next;
        }
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

    /// Populate the strip with the MAX_SHOTS most recent screenshots, decoded
    /// after the window presents so startup isn't blocked.
    ///
    /// `scan_shots` is oldest-first, so we take the tail (the newest few) and add
    /// them oldest-first — each prepend in `add_saved` then leaves the newest on top.
    fn load_existing_deferred(self: &Rc<Self>) {
        let mut shots = Self::scan_shots();
        let start = shots.len().saturating_sub(MAX_SHOTS);
        let recent = shots.split_off(start);
        if recent.is_empty() {
            return;
        }
        let panel = self.clone();
        glib::idle_add_local_once(move || {
            for p in recent {
                panel.add_saved(p);
            }
        });
    }
}
