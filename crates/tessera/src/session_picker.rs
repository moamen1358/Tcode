//! The launch screen: resume a saved session or start a new one. Mirrors the
//! old pane-count picker pattern — returns a widget the app sets as the window
//! child, and calls back when the user chooses.

use gtk4::pango::EllipsizeMode;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation, PolicyType, ScrolledWindow, Widget};
use std::rc::Rc;

use tessera_core::session::Session;

/// Build the session picker. `on_open` runs with the chosen saved session;
/// `on_new` runs when the user asks to create a new one.
pub fn build(
    sessions: Vec<Session>,
    on_open: impl Fn(Session) + 'static,
    on_new: impl Fn() + 'static,
) -> Widget {
    let on_open = Rc::new(on_open);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.add_css_class("picker-root");

    let column = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .halign(Align::Center)
        .valign(Align::Center)
        .vexpand(true)
        .build();
    column.set_width_request(440);

    let title = Label::new(Some("Tessera"));
    title.add_css_class("session-title");
    column.append(&title);

    let subtitle = Label::new(Some(if sessions.is_empty() {
        "No saved sessions yet — start a new one"
    } else {
        "Resume a session"
    }));
    subtitle.add_css_class("session-subtitle");
    column.append(&subtitle);

    // Scrollable list of saved sessions so a long history doesn't overflow.
    let list = GtkBox::new(Orientation::Vertical, 6);
    for s in sessions {
        list.append(&session_card(s, &on_open));
    }
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .max_content_height(380)
        .propagate_natural_height(true)
        .child(&list)
        .build();
    column.append(&scroll);

    let new_btn = Button::with_label("＋  New Session");
    new_btn.add_css_class("session-new");
    new_btn.connect_clicked(move |_| on_new());
    column.append(&new_btn);

    root.append(&column);
    root.upcast()
}

/// One clickable card: name on top, then "root · N terminals · M files".
fn session_card<F: Fn(Session) + 'static>(s: Session, on_open: &Rc<F>) -> Button {
    let body = GtkBox::new(Orientation::Vertical, 2);

    let name = Label::new(Some(&s.name));
    name.set_xalign(0.0);
    name.add_css_class("session-name");
    body.append(&name);

    let meta = Label::new(Some(&format!(
        "{}   ·   {} terminal{}   ·   {} file{}",
        s.root.display(),
        s.panes,
        if s.panes == 1 { "" } else { "s" },
        s.files.len(),
        if s.files.len() == 1 { "" } else { "s" },
    )));
    meta.set_xalign(0.0);
    meta.set_ellipsize(EllipsizeMode::Middle);
    meta.add_css_class("session-meta");
    body.append(&meta);

    let btn = Button::builder().child(&body).build();
    btn.add_css_class("session-card");
    let on_open = on_open.clone();
    btn.connect_clicked(move |_| on_open(s.clone()));
    btn
}
