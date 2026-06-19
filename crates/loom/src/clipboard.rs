//! Clipboard-history strip in the sidebar: a CLIPBOARD header with a Clear
//! button (which asks for confirmation), then a scrollable stack of minimal
//! cards — each just the copied text in a rectangle (two-line preview) with pin
//! and delete (×) buttons. Click a card to copy it again; pin keeps an entry at
//! the top, exempt from Clear and the entry cap. Records every text copied to the
//! system clipboard (from any app) while Loom runs, newest on top, and saves
//! the history to disk so it survives restarts.
//!
//! The card widgets are a view over `entries` (the source of truth); any change
//! updates that list, rebuilds the cards, and re-saves the history file.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::gdk::Display;
use gtk4::pango::{EllipsizeMode, WrapMode};
use gtk4::prelude::*;
use gtk4::{
    gio, AlertDialog, Align, Box as GtkBox, Button, Label, Orientation, PolicyType, ScrolledWindow,
    Separator,
};

/// Cap on remembered (unpinned) clipboard entries.
const MAX_ENTRIES: usize = 60;
/// Entries larger than this are kept for the session but not written to disk, so
/// one huge copy can't bloat the history file.
const MAX_PERSIST_BYTES: usize = 64 * 1024;

/// One remembered clipboard entry. `id` is a stable handle a card's buttons use
/// to find their entry again after the list has been reordered.
struct Entry {
    id: u64,
    text: String,
    pinned: bool,
}

pub struct Panel {
    /// The section root (the host embeds this above the screenshots strip).
    pub root: GtkBox,
    list: GtkBox,
    /// Source of truth: pinned entries first, then unpinned newest-first.
    entries: RefCell<Vec<Entry>>,
    /// Most recently captured text — skips duplicate change events (including
    /// the one our own re-copy triggers).
    last: RefCell<String>,
    next_id: Cell<u64>,
    /// Whether to read/write the on-disk history (config `clipboard_persist`).
    /// When false, history is kept only for the running session.
    persist: bool,
}

/// Path of the on-disk history (user data dir, so it persists across restarts).
fn history_path() -> PathBuf {
    gtk4::glib::user_data_dir()
        .join("loom")
        .join("clipboard.history")
}

/// Build the clipboard-history section and start watching the system clipboard.
/// `persist` mirrors the config flag: when false, nothing is read from or written
/// to disk (history lives only for this session).
pub fn build(persist: bool) -> Rc<Panel> {
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
        entries: RefCell::new(Vec::new()),
        last: RefCell::new(String::new()),
        next_id: Cell::new(0),
        persist,
    });

    // Clear-all asks first, so the history isn't wiped by a stray click.
    {
        let weak = Rc::downgrade(&panel);
        clear.connect_clicked(move |btn| {
            let Some(panel) = weak.upgrade() else {
                return;
            };
            // Nothing unpinned to clear → do nothing (and don't nag).
            if !panel.entries.borrow().iter().any(|e| !e.pinned) {
                return;
            }
            let dialog = AlertDialog::builder()
                .modal(true)
                .message("Clear clipboard history?")
                .detail("Removes all unpinned entries. Pinned items are kept.")
                .build();
            dialog.set_buttons(&["Cancel", "Clear"]);
            dialog.set_cancel_button(0);
            dialog.set_default_button(0);
            let window = btn.root().and_downcast::<gtk4::Window>();
            let weak = weak.clone();
            dialog.choose(window.as_ref(), gio::Cancellable::NONE, move |res| {
                if let Ok(1) = res {
                    if let Some(panel) = weak.upgrade() {
                        panel.clear();
                    }
                }
            });
        });
    }

    if persist {
        panel.load_from_disk();
    }
    panel.rebuild();
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

    /// Record a freshly copied text as the newest unpinned entry.
    fn add_entry(self: &Rc<Self>, text: String) {
        if text.trim().is_empty() || *self.last.borrow() == text {
            return; // empty, or a duplicate of the most recent capture
        }
        *self.last.borrow_mut() = text.clone();

        let id = self.next_id.get();
        self.next_id.set(id + 1);
        {
            let mut entries = self.entries.borrow_mut();
            let pinned = entries.iter().filter(|e| e.pinned).count();
            entries.insert(
                pinned,
                Entry {
                    id,
                    text,
                    pinned: false,
                },
            );
            trim_unpinned(&mut entries);
        }
        self.rebuild();
        self.save();
    }

    /// Re-copy an entry to the system clipboard.
    fn recopy(&self, id: u64) {
        let text = self
            .entries
            .borrow()
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.text.clone());
        if let Some(text) = text {
            *self.last.borrow_mut() = text.clone();
            if let Some(d) = Display::default() {
                d.clipboard().set_text(&text);
            }
        }
    }

    /// Remove a single entry.
    fn delete(self: &Rc<Self>, id: u64) {
        self.entries.borrow_mut().retain(|e| e.id != id);
        self.rebuild();
        self.save();
    }

    /// Pin → move to the very top; unpin → drop just below the pinned block.
    fn toggle_pin(self: &Rc<Self>, id: u64) {
        {
            let mut entries = self.entries.borrow_mut();
            let Some(pos) = entries.iter().position(|e| e.id == id) else {
                return;
            };
            let mut entry = entries.remove(pos);
            entry.pinned = !entry.pinned;
            if entry.pinned {
                entries.insert(0, entry);
            } else {
                let pinned = entries.iter().filter(|e| e.pinned).count();
                entries.insert(pinned, entry);
            }
        }
        self.rebuild();
        self.save();
    }

    /// Drop every unpinned entry (called after the user confirms).
    fn clear(self: &Rc<Self>) {
        self.entries.borrow_mut().retain(|e| e.pinned);
        *self.last.borrow_mut() = String::new();
        self.rebuild();
        self.save();
    }

    /// Rebuild the card widgets from `entries` (or the empty-state hint).
    fn rebuild(self: &Rc<Self>) {
        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let entries = self.entries.borrow();
        if entries.is_empty() {
            let hint = Label::new(Some("Copy something to start"));
            hint.set_xalign(0.0);
            hint.add_css_class("clip-empty");
            self.list.append(&hint);
            return;
        }
        for e in entries.iter() {
            let card = self.build_card(e.id, &e.text, e.pinned);
            self.list.append(&card);
        }
    }

    /// Build one card (text + pin + delete) wired to its entry by `id`.
    fn build_card(self: &Rc<Self>, id: u64, text: &str, pinned: bool) -> GtkBox {
        let card = GtkBox::new(Orientation::Horizontal, 0);
        card.add_css_class("clip-card");
        if pinned {
            card.add_css_class("pinned");
        }

        let copy = Button::new();
        copy.add_css_class("clip-copy");
        copy.set_hexpand(true);
        let prev = Label::new(Some(&preview(text)));
        prev.set_xalign(0.0);
        prev.add_css_class("clip-text");
        prev.set_wrap(true);
        prev.set_wrap_mode(WrapMode::WordChar);
        prev.set_lines(2);
        prev.set_ellipsize(EllipsizeMode::End);
        copy.set_child(Some(&prev));
        copy.set_tooltip_text(Some(text.trim()));
        card.append(&copy);

        let pin = Button::from_icon_name("view-pin-symbolic");
        pin.add_css_class("clip-pin");
        pin.set_valign(Align::Start);
        pin.set_tooltip_text(Some(if pinned { "Unpin" } else { "Pin" }));
        card.append(&pin);

        let del = Button::from_icon_name("window-close-symbolic");
        del.add_css_class("clip-del");
        del.set_valign(Align::Start);
        del.set_tooltip_text(Some("Remove"));
        card.append(&del);

        let weak = Rc::downgrade(self);
        copy.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.recopy(id);
            }
        });
        let weak = Rc::downgrade(self);
        pin.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.toggle_pin(id);
            }
        });
        let weak = Rc::downgrade(self);
        del.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.delete(id);
            }
        });

        card
    }

    /// Load saved history (length-prefixed records, see `save`).
    fn load_from_disk(&self) {
        let Ok(data) = std::fs::read(history_path()) else {
            return;
        };
        let mut entries = self.entries.borrow_mut();
        let mut i = 0;
        while i < data.len() {
            let Some(nl) = data[i..].iter().position(|&b| b == b'\n').map(|p| i + p) else {
                break;
            };
            let Ok(header) = std::str::from_utf8(&data[i..nl]) else {
                break;
            };
            let mut parts = header.splitn(2, ' ');
            let pinned = parts.next() == Some("P");
            let Some(len) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
                break;
            };
            let start = nl + 1;
            // Checked: a corrupt/hand-edited header with a huge length must not
            // overflow (debug panic) or wrap past the guard into an invalid slice.
            let Some(end) = start.checked_add(len).filter(|&e| e <= data.len()) else {
                break;
            };
            let Ok(text) = std::str::from_utf8(&data[start..end]) else {
                break;
            };
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            entries.push(Entry {
                id,
                text: text.to_string(),
                pinned,
            });
            i = end.saturating_add(1); // skip the record's trailing newline
        }
        trim_unpinned(&mut entries);
    }

    /// Persist the history: one record per entry as `{P|U} {byte_len}\n{text}\n`,
    /// which round-trips arbitrary text (including newlines) without escaping.
    fn save(&self) {
        if !self.persist {
            return; // session-only history; never touch disk
        }
        let mut out: Vec<u8> = Vec::new();
        for e in self.entries.borrow().iter() {
            let bytes = e.text.as_bytes();
            if bytes.len() > MAX_PERSIST_BYTES {
                continue;
            }
            let flag = if e.pinned { 'P' } else { 'U' };
            out.extend_from_slice(format!("{} {}\n", flag, bytes.len()).as_bytes());
            out.extend_from_slice(bytes);
            out.push(b'\n');
        }
        let path = history_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&path, &out).is_ok() {
            // History can hold secrets (e.g. a password copied from a manager);
            // restrict it to the owner.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

/// Keep all pinned entries plus the newest `MAX_ENTRIES` unpinned ones. Relies on
/// the invariant that unpinned entries are stored newest-first.
fn trim_unpinned(entries: &mut Vec<Entry>) {
    let mut kept = 0;
    entries.retain(|e| {
        if e.pinned {
            return true;
        }
        kept += 1;
        kept <= MAX_ENTRIES
    });
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
