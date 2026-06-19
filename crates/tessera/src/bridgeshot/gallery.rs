//! Left-side thumbnail strip of this session's captures. Clicking a thumbnail
//! makes that document active on the canvas.

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
