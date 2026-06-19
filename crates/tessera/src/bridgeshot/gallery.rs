//! The persistent left-side screenshots panel in Tessera's main window. Shows a
//! Capture button plus a thumbnail per saved screenshot (loaded from the cache
//! dir on startup, so history survives restarts). Clicking a thumbnail re-opens
//! it in the annotation canvas.

use std::path::PathBuf;
use std::rc::Rc;

use gtk4::gdk::Texture;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, PolicyType, ScrolledWindow};

/// Directory where exported screenshots live.
pub fn shots_dir() -> PathBuf {
    glib::user_cache_dir().join("tessera").join("bridgeshot")
}

pub struct Panel {
    pub root: GtkBox,
    list: GtkBox,
    on_pick: Rc<dyn Fn(PathBuf)>,
}

/// Build the panel. `on_capture` runs when the Capture button is clicked;
/// `on_pick` runs when a thumbnail is clicked (with that shot's path).
pub fn build(on_capture: Rc<dyn Fn()>, on_pick: Rc<dyn Fn(PathBuf)>) -> Rc<Panel> {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("bridgeshot-panel");
    root.set_width_request(168);

    let header = GtkBox::new(Orientation::Horizontal, 6);
    header.add_css_class("bridgeshot-panel-header");
    let title = Label::new(Some("Screenshots"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("bridgeshot-panel-title");
    let capture = Button::from_icon_name("camera-photo-symbolic");
    capture.set_tooltip_text(Some("Capture a screenshot"));
    capture.add_css_class("flat");
    capture.connect_clicked(move |_| on_capture());
    header.append(&title);
    header.append(&capture);
    root.append(&header);

    let list = GtkBox::new(Orientation::Vertical, 6);
    list.add_css_class("bridgeshot-gallery");
    list.set_margin_top(6);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);
    let scroll = ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .build();
    root.append(&scroll);

    let panel = Rc::new(Panel {
        root,
        list,
        on_pick,
    });
    panel.load_existing();
    panel
}

impl Panel {
    /// Append a thumbnail for a saved screenshot at `path` (newest at the bottom).
    pub fn add_saved(&self, path: PathBuf) {
        let Ok(pb) = Pixbuf::from_file_at_scale(&path, 148, -1, true) else {
            return;
        };
        let texture = Texture::for_pixbuf(&pb);
        let pic = gtk4::Picture::for_paintable(&texture);
        pic.set_can_shrink(true);
        let btn = Button::builder().child(&pic).build();
        btn.add_css_class("bridgeshot-thumb");
        btn.set_tooltip_text(path.file_name().and_then(|n| n.to_str()));

        let on_pick = self.on_pick.clone();
        btn.connect_clicked(move |_| on_pick(path.clone()));
        self.list.append(&btn);
    }

    /// Load every saved shot-*.png from the cache dir, oldest first.
    fn load_existing(&self) {
        let dir = shots_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
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
        for (_, p) in shots {
            self.add_saved(p);
        }
    }
}
