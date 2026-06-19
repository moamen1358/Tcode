//! The launch screen: resume a saved session or start a new one. Mirrors the
//! old pane-count picker pattern — returns a widget the app sets as the window
//! child, and calls back when the user chooses.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::pango::EllipsizeMode;
use gtk4::prelude::*;
use gtk4::{
    gio, Align, ApplicationWindow, Box as GtkBox, Button, FileDialog, Image, Label, Orientation,
    PolicyType, ScrolledWindow, Widget,
};

use tessera_core::session::Session;

/// Terminal-count choices offered when creating a new session.
const COUNTS: [usize; 7] = [1, 2, 3, 4, 6, 8, 9];

/// Build the session picker. `on_open` runs with the chosen saved session;
/// `on_new` runs when the user asks to create a new one.
pub fn build(
    sessions: Vec<Session>,
    on_open: impl Fn(Session) + 'static,
    on_new: impl Fn() + 'static,
) -> Widget {
    let on_open = Rc::new(on_open);

    let column = centered_column();

    column.append(&heading(
        "Tessera",
        if sessions.is_empty() {
            "No saved sessions yet — start a new one"
        } else {
            "Resume a session"
        },
    ));

    // Scrollable list of saved sessions so a long history doesn't overflow.
    let list = GtkBox::new(Orientation::Vertical, 8);
    for s in sessions {
        list.append(&session_card(s, &on_open));
    }
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .max_content_height(400)
        .propagate_natural_height(true)
        .child(&list)
        .build();
    column.append(&scroll);

    let new_btn = Button::with_label("＋  New Session");
    new_btn.add_css_class("session-new");
    new_btn.connect_clicked(move |_| on_new());
    column.append(&new_btn);

    wrap(column)
}

/// Build the "new session" screen: pick a folder, choose a terminal count, then
/// `on_create(folder, count)`. `on_cancel` goes back.
pub fn build_new(
    window: ApplicationWindow,
    on_create: impl Fn(PathBuf, usize) + 'static,
    on_cancel: impl Fn() + 'static,
) -> Widget {
    let chosen: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let count = Rc::new(Cell::new(1usize));

    let column = centered_column();
    column.append(&heading(
        "New Session",
        "Pick a folder and how many terminals",
    ));

    // Folder picker.
    column.append(&field_label("FOLDER"));
    let folder_btn = Button::new();
    folder_btn.add_css_class("session-folder-btn");
    let fb_box = GtkBox::new(Orientation::Horizontal, 8);
    let fi = Image::from_icon_name("folder-symbolic");
    fi.set_pixel_size(16);
    fi.add_css_class("session-card-icon");
    let folder_text = Label::new(Some("Choose a folder…"));
    folder_text.set_xalign(0.0);
    folder_text.set_hexpand(true);
    folder_text.set_ellipsize(EllipsizeMode::Middle);
    fb_box.append(&fi);
    fb_box.append(&folder_text);
    folder_btn.set_child(Some(&fb_box));
    column.append(&folder_btn);

    // Terminal count, as a segmented row.
    column.append(&field_label("TERMINALS"));
    let count_row = GtkBox::new(Orientation::Horizontal, 6);
    let count_btns: Rc<RefCell<Vec<Button>>> = Rc::new(RefCell::new(Vec::new()));
    for n in COUNTS {
        let b = Button::with_label(&n.to_string());
        b.add_css_class("session-count");
        b.set_hexpand(true);
        if n == 1 {
            b.add_css_class("selected");
        }
        let count_c = count.clone();
        let btns = count_btns.clone();
        b.connect_clicked(move |btn| {
            count_c.set(n);
            for cb in btns.borrow().iter() {
                cb.remove_css_class("selected");
            }
            btn.add_css_class("selected");
        });
        count_btns.borrow_mut().push(b.clone());
        count_row.append(&b);
    }
    column.append(&count_row);

    // Actions.
    let create_btn = Button::with_label("Create Session");
    create_btn.add_css_class("session-new");
    create_btn.set_sensitive(false);
    {
        let chosen_c = chosen.clone();
        let count_c = count.clone();
        create_btn.connect_clicked(move |_| {
            if let Some(folder) = chosen_c.borrow().clone() {
                on_create(folder, count_c.get());
            }
        });
    }
    column.append(&create_btn);

    let back_btn = Button::with_label("← Back");
    back_btn.add_css_class("session-back");
    back_btn.connect_clicked(move |_| on_cancel());
    column.append(&back_btn);

    // Folder button opens the native chooser; on success show the path + enable Create.
    {
        let chosen_c = chosen.clone();
        let create_c = create_btn.clone();
        folder_btn.connect_clicked(move |_| {
            let dialog = FileDialog::builder()
                .title("Choose a folder for the new session")
                .modal(true)
                .build();
            let chosen_cc = chosen_c.clone();
            let create_cc = create_c.clone();
            let text = folder_text.clone();
            dialog.select_folder(Some(&window), gio::Cancellable::NONE, move |res| {
                if let Ok(folder) = res {
                    if let Some(path) = folder.path() {
                        text.set_label(&path.display().to_string());
                        *chosen_cc.borrow_mut() = Some(path);
                        create_cc.set_sensitive(true);
                    }
                }
            });
        });
    }

    wrap(column)
}

/// One clickable card: folder icon, name, path, and terminal/file badges.
fn session_card<F: Fn(Session) + 'static>(s: Session, on_open: &Rc<F>) -> Button {
    let row = GtkBox::new(Orientation::Horizontal, 12);

    let icon = Image::from_icon_name("folder-symbolic");
    icon.set_pixel_size(24);
    icon.set_valign(Align::Center);
    icon.add_css_class("session-card-icon");
    row.append(&icon);

    let body = GtkBox::new(Orientation::Vertical, 3);
    body.set_hexpand(true);

    let name = Label::new(Some(&s.name));
    name.set_xalign(0.0);
    name.add_css_class("session-name");
    body.append(&name);

    let path = Label::new(Some(&s.root.display().to_string()));
    path.set_xalign(0.0);
    path.set_ellipsize(EllipsizeMode::Middle);
    path.add_css_class("session-meta");
    body.append(&path);

    let badges = GtkBox::new(Orientation::Horizontal, 6);
    badges.append(&badge(
        "utilities-terminal-symbolic",
        &format!("{} terminal{}", s.panes, plural(s.panes)),
    ));
    badges.append(&badge(
        "text-x-generic-symbolic",
        &format!("{} file{}", s.files.len(), plural(s.files.len())),
    ));
    body.append(&badges);

    row.append(&body);

    let btn = Button::builder().child(&row).build();
    btn.add_css_class("session-card");
    let on_open = on_open.clone();
    btn.connect_clicked(move |_| on_open(s.clone()));
    btn
}

/// A small icon + text pill.
fn badge(icon_name: &str, text: &str) -> GtkBox {
    let b = GtkBox::new(Orientation::Horizontal, 4);
    b.add_css_class("session-badge");
    let i = Image::from_icon_name(icon_name);
    i.set_pixel_size(12);
    let l = Label::new(Some(text));
    b.append(&i);
    b.append(&l);
    b
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The fixed-width centered column shared by both screens.
fn centered_column() -> GtkBox {
    let column = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .halign(Align::Center)
        .valign(Align::Center)
        .vexpand(true)
        .build();
    column.set_width_request(460);
    column
}

/// Title + subtitle block.
fn heading(title: &str, subtitle: &str) -> GtkBox {
    let b = GtkBox::new(Orientation::Vertical, 2);
    b.set_margin_bottom(8);
    let t = Label::new(Some(title));
    t.add_css_class("session-title");
    let s = Label::new(Some(subtitle));
    s.add_css_class("session-subtitle");
    b.append(&t);
    b.append(&s);
    b
}

fn field_label(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.set_xalign(0.0);
    l.add_css_class("session-field-label");
    l
}

/// Wrap a column in the full-screen picker background.
fn wrap(column: GtkBox) -> Widget {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.add_css_class("picker-root");
    root.append(&column);
    root.upcast()
}
