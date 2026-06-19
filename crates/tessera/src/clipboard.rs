//! Clipboard-history strip in the sidebar. While Tessera runs it records every
//! text copied to the system clipboard (from any app), newest on top; click an
//! entry to copy it again. In-memory only — nothing is written to disk.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk::Display;
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

    let list = GtkBox::new(Orientation::Vertical, 3);
    list.add_css_class("clip-list");
    list.set_margin_top(6);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);

    let hint = Label::new(Some("Copied text shows up here"));
    hint.set_xalign(0.0);
    hint.add_css_class("clip-empty");
    list.append(&hint);

    // Same height as the screenshots strip; scroll to reach older entries.
    let scroll = ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .height_request(290)
        .build();
    root.append(&scroll);

    let panel = Rc::new(Panel {
        root,
        list,
        last: Rc::new(RefCell::new(String::new())),
        hint: RefCell::new(Some(hint.upcast())),
    });
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

    fn add_entry(&self, text: String) {
        if text.trim().is_empty() || *self.last.borrow() == text {
            return; // empty, or a duplicate of the most recent capture
        }
        *self.last.borrow_mut() = text.clone();

        // Drop the empty-state placeholder once there's a real entry.
        if let Some(h) = self.hint.borrow_mut().take() {
            self.list.remove(&h);
        }

        let label = Label::new(Some(&preview(&text)));
        label.set_xalign(0.0);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_single_line_mode(true);
        let btn = Button::builder().child(&label).build();
        btn.add_css_class("clip-entry");
        btn.set_tooltip_text(Some(text.trim()));

        // Click re-copies the entry (updating `last` first so the resulting
        // clipboard-changed event is recognized as our own and skipped).
        {
            let (text, last) = (text.clone(), self.last.clone());
            btn.connect_clicked(move |_| {
                *last.borrow_mut() = text.clone();
                if let Some(d) = Display::default() {
                    d.clipboard().set_text(&text);
                }
            });
        }

        self.list.prepend(&btn); // newest on top

        // Trim to the cap (oldest fall off the bottom).
        let mut n = 0;
        let mut child = self.list.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            n += 1;
            if n > MAX_ENTRIES {
                self.list.remove(&c);
            }
            child = next;
        }
    }
}

/// One-line preview of copied text: runs of whitespace collapsed, length capped.
fn preview(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 140 {
        collapsed.chars().take(140).collect::<String>() + "…"
    } else {
        collapsed
    }
}
