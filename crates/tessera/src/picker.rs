//! The launch screen: a centered row of number buttons. Clicking one (or
//! pressing Alt+digit, handled in `keys.rs`) builds the grid.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Orientation, Widget};
use std::rc::Rc;

const CHOICES: [usize; 7] = [1, 2, 3, 4, 6, 8, 9];

/// Build the picker widget. `on_pick` is called with the chosen pane count.
pub fn build(on_pick: impl Fn(usize) + 'static) -> Widget {
    let on_pick = Rc::new(on_pick);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.add_css_class("picker-root");

    let buttons = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .halign(Align::Center)
        .valign(Align::Center)
        .hexpand(true)
        .vexpand(true)
        .build();

    for n in CHOICES {
        let btn = Button::with_label(&n.to_string());
        btn.add_css_class("pick");
        btn.set_size_request(80, 80);
        let on_pick = on_pick.clone();
        btn.connect_clicked(move |_| on_pick(n));
        buttons.append(&btn);
    }

    root.append(&buttons);
    root.upcast()
}
