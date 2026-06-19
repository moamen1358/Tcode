//! Clipboard-history strip in the sidebar: a CLIPBOARD header with a Clear
//! button, then a scrollable stack of minimal cards — each just the copied text
//! in a rectangle (two-line preview) with pin and delete (×) buttons. Click a
//! card to copy it again; pin keeps an entry at the top, exempt from Clear and
//! the entry cap. Records every text copied to the system clipboard (from any
//! app) while Tessera runs, newest on top. In-memory only — nothing on disk.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk::Display;
use gtk4::pango::{EllipsizeMode, WrapMode};
use gtk4::prelude::*;
use gtk4::{gio, Box as GtkBox, Button, Label, Orientation, PolicyType, ScrolledWindow, Separator};

/// Cap on remembered clipboard entries.
const MAX_ENTRIES: usize = 60;

pub struct Panel {
    /// The section root (the host embeds this above the screenshots strip).
    pub root: GtkBox,
    list: GtkBox,
    /// Most recently captured text — used to skip duplicate change events
    /// (including the one our own re-copy triggers).
    last: Rc<RefCell<String>>,
    /// Placeholder shown while the history is empty; removed on the first entry.
    hint: RefCell<Option<gtk4::Widget>>,
}

/// Build the clipboard-history section and start watching the system clipboard.
pub fn build() -> Rc<Panel> {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("shots-section");
    root.append(&Separator::new(Orientation::Horizontal));

    // Header: label + clear-all.
    let header = GtkBox::new(Orientation::Horizontal, 6);
    header.add_css_class("clip-header");
    let title = Label::new(Some("CLIPBOARD"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let clear = Button::from_icon_name("user-trash-symbolic");
    clear.add_css_class("clip-clear");
    clear.set_tooltip_text(Some("Clear clipboard history"));
    header.append(&title);
    header.append(&clear);
    root.append(&header);

    let list = GtkBox::new(Orientation::Vertical, 5);
    list.add_css_class("clip-list");
    list.set_margin_top(4);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);

    let hint = Label::new(Some("Copy something to start"));
    hint.set_xalign(0.0);
    hint.add_css_class("clip-empty");
    list.append(&hint);

    let scroll = ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .height_request(250)
        .build();
    root.append(&scroll);

    let panel = Rc::new(Panel {
        root,
        list,
        last: Rc::new(RefCell::new(String::new())),
        hint: RefCell::new(Some(hint.upcast())),
    });

    // Clear-all: empty the list and reset the dedupe guard so the next copy
    // (even of the same text) is recorded again.
    {
        let (list, last, panel_w) = (
            panel.list.clone(),
            panel.last.clone(),
            Rc::downgrade(&panel),
        );
        clear.connect_clicked(move |_| {
            // Remove every unpinned card; pinned ones stay.
            let mut child = list.first_child();
            while let Some(c) = child {
                let next = c.next_sibling();
                if !c.has_css_class("pinned") {
                    list.remove(&c);
                }
                child = next;
            }
            *last.borrow_mut() = String::new();
            // Only show the placeholder if nothing (not even a pin) is left.
            if list.first_child().is_none() {
                if let Some(p) = panel_w.upgrade() {
                    p.show_hint();
                }
            }
        });
    }

    panel.monitor();
    panel
}

impl Panel {
    /// Watch the system clipboard and record each new text copy. Holds only a
    /// weak ref so the long-lived clipboard signal can't pin this panel.
    fn monitor(self: &Rc<Self>) {
        let Some(display) = Display::default() else {
            return;
        };
        let weak = Rc::downgrade(self);
        display.clipboard().connect_changed(move |cb| {
            let Some(panel) = weak.upgrade() else {
                return;
            };
            cb.read_text_async(gio::Cancellable::NONE, move |res| {
                if let Ok(Some(text)) = res {
                    panel.add_entry(text.to_string());
                }
            });
        });
    }

    /// Re-show the empty placeholder (after a clear).
    fn show_hint(&self) {
        if self.hint.borrow().is_some() {
            return;
        }
        let hint = Label::new(Some("Copy something to start"));
        hint.set_xalign(0.0);
        hint.add_css_class("clip-empty");
        self.list.append(&hint);
        *self.hint.borrow_mut() = Some(hint.upcast());
    }

    fn add_entry(&self, text: String) {
        if text.trim().is_empty() || *self.last.borrow() == text {
            return; // empty, or a duplicate of the most recent capture
        }
        *self.last.borrow_mut() = text.clone();

        // Drop the empty-state placeholder once there's a real entry.
        if let Some(h) = self.hint.borrow_mut().take() {
            self.list.remove(&h);
        }

        // A minimal card: just the copied text in a rectangle.
        let card = GtkBox::new(Orientation::Horizontal, 0);
        card.add_css_class("clip-card");

        let copy = Button::new();
        copy.add_css_class("clip-copy");
        copy.set_hexpand(true);
        let prev = Label::new(Some(&preview(&text)));
        prev.set_xalign(0.0);
        prev.add_css_class("clip-text");
        prev.set_wrap(true);
        prev.set_wrap_mode(WrapMode::WordChar);
        prev.set_lines(2);
        prev.set_ellipsize(EllipsizeMode::End);
        copy.set_child(Some(&prev));
        copy.set_tooltip_text(Some(text.trim()));
        card.append(&copy);

        // Pin: keeps the entry at the top, exempt from Clear and the cap.
        let pin = Button::from_icon_name("view-pin-symbolic");
        pin.add_css_class("clip-pin");
        pin.set_valign(gtk4::Align::Start);
        pin.set_tooltip_text(Some("Pin"));
        card.append(&pin);

        // Delete (×).
        let del = Button::from_icon_name("window-close-symbolic");
        del.add_css_class("clip-del");
        del.set_valign(gtk4::Align::Start);
        del.set_tooltip_text(Some("Remove"));
        card.append(&del);

        // Click body → re-copy (update `last` first so the resulting change event
        // is recognized as our own and skipped).
        {
            let (text, last) = (text.clone(), self.last.clone());
            copy.connect_clicked(move |_| {
                *last.borrow_mut() = text.clone();
                if let Some(d) = Display::default() {
                    d.clipboard().set_text(&text);
                }
            });
        }
        // Pin toggle → move into / out of the pinned block at the top.
        {
            let (list_w, card_w) = (self.list.downgrade(), card.downgrade());
            pin.connect_clicked(move |_| {
                let (Some(list), Some(card)) = (list_w.upgrade(), card_w.upgrade()) else {
                    return;
                };
                let now_pinned = !card.has_css_class("pinned");
                if now_pinned {
                    card.add_css_class("pinned");
                } else {
                    card.remove_css_class("pinned");
                }
                // Pinned cards go to the very top; an unpinned card drops just
                // below the cards that are still pinned.
                list.remove(&card);
                let after = if now_pinned { None } else { last_pinned(&list) };
                list.insert_child_after(&card, after.as_ref());
            });
        }
        // Click × → remove just this card (weak refs avoid a card<->closure cycle).
        {
            let (list_w, card_w) = (self.list.downgrade(), card.downgrade());
            del.connect_clicked(move |_| {
                if let (Some(list), Some(card)) = (list_w.upgrade(), card_w.upgrade()) {
                    list.remove(&card);
                }
            });
        }

        // Insert below any pinned cards (a contiguous block at the top), so a
        // fresh copy is the newest *unpinned* entry.
        self.list
            .insert_child_after(&card, last_pinned(&self.list).as_ref());

        // Trim unpinned entries to the cap; pinned cards are exempt.
        let mut n = 0;
        let mut child = self.list.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            if !c.has_css_class("pinned") {
                n += 1;
                if n > MAX_ENTRIES {
                    self.list.remove(&c);
                }
            }
            child = next;
        }
    }
}

/// The last card of the pinned block (pinned cards form a contiguous prefix at
/// the top of the list), or None if nothing is pinned. New entries are inserted
/// after this, so pinned cards always stay above the rest.
fn last_pinned(list: &GtkBox) -> Option<gtk4::Widget> {
    let mut last = None;
    let mut child = list.first_child();
    while let Some(c) = child {
        if !c.has_css_class("pinned") {
            break;
        }
        child = c.next_sibling();
        last = Some(c);
    }
    last
}

/// Preview text for a card: whitespace runs (including newlines) collapsed to
/// single spaces so every card wraps to a uniform two lines, capped in length.
fn preview(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 200 {
        collapsed.chars().take(200).collect::<String>() + "…"
    } else {
        collapsed
    }
}
