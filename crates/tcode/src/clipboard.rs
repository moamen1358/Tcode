//! Clipboard-history strip in the sidebar: a CLIPBOARD header with a Clear
//! button (which asks for confirmation), then a scrollable stack of minimal
//! cards — each just the copied text in a rectangle (two-line preview) with pin
//! and delete (×) buttons. Click a card to copy it again; pin keeps an entry at
//! the top, exempt from Clear and the entry cap. Records every text copied to the
//! system clipboard (from any app) while Tcode runs, newest on top, and saves
//! the history to disk so it survives restarts.
//!
//! The card widgets are a view over `entries` (the source of truth); any change
//! updates that list, rebuilds the cards, and re-saves the history file.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::gdk::{Display, Key, ModifierType};
use gtk4::glib::Propagation;
use gtk4::pango::{EllipsizeMode, WrapMode};
use gtk4::prelude::*;
use gtk4::{
    gio, AlertDialog, Align, Box as GtkBox, Button, EventControllerKey, Label, Orientation,
    PolicyType, ScrolledWindow, Separator,
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
        .join("tcode")
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
            make_private_dir(dir);
        }
        // Atomic + owner-only: history can hold secrets (e.g. a password copied from
        // a manager); a torn write must not corrupt or truncate it.
        if let Err(e) = tcode_core::fsutil::atomic_write(&path, &out, 0o600) {
            eprintln!("tcode: failed to write clipboard history: {e}");
        }
    }

    // --- Alt+V command palette (a search-filtered view over the same model) ---

    /// Texts of all entries in display order (pinned-first), for the palette filter.
    pub fn texts_in_order(&self) -> Vec<String> {
        self.entries.borrow().iter().map(|e| e.text.clone()).collect()
    }

    /// Text of the entry at display index `idx`, if any.
    pub fn entry_text(&self, idx: usize) -> Option<String> {
        self.entries.borrow().get(idx).map(|e| e.text.clone())
    }

    /// Whether the entry at display index `idx` is pinned.
    pub fn entry_is_pinned(&self, idx: usize) -> bool {
        self.entries.borrow().get(idx).map(|e| e.pinned).unwrap_or(false)
    }

    /// Copy `text` to the system clipboard (the palette's Ctrl-activate path).
    pub fn copy_text(&self, text: &str) {
        *self.last.borrow_mut() = text.to_string();
        if let Some(d) = Display::default() {
            d.clipboard().set_text(text);
        }
    }

    /// Build the Alt+V command palette: a search box over a scrollable, filtered
    /// list of history entries. Enter or click pastes the selected entry via
    /// `on_paste`; Ctrl+Enter copies it to the system clipboard instead. Acting
    /// (or Esc/click-away) closes the overlay `host`. It re-renders fresh, clears
    /// the query, and focuses the search box each time it's shown.
    pub fn palette(
        self: &Rc<Self>,
        on_paste: Rc<dyn Fn(&str)>,
        host: std::rc::Weak<crate::overlay::OverlayHost>,
    ) -> GtkBox {
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("clip-palette");
        root.set_size_request(660, -1);

        let search = gtk4::Entry::builder()
            .placeholder_text("Search clipboard…")
            .primary_icon_name("system-search-symbolic")
            .build();
        search.add_css_class("clip-search");
        root.append(&search);

        let list = GtkBox::new(Orientation::Vertical, 4);
        list.add_css_class("clip-pal-list");
        let scroll = ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .height_request(460)
            .build();
        root.append(&scroll);

        // Key-hints footer (discoverability).
        let footer = Label::new(Some("↵  paste        ⌃↵  copy        esc  close"));
        footer.set_xalign(0.0);
        footer.add_css_class("clip-pal-footer");
        root.append(&footer);

        // Selection index into the *currently shown* (filtered) rows.
        let sel = Rc::new(Cell::new(0usize));
        let shown: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));

        // (Re)render the filtered rows for the current query. `search` is held
        // weakly so the entry's own signal handlers (which capture `render`) can't
        // form a reference cycle that leaks the palette.
        let render: Rc<dyn Fn()> = {
            let (panel, list, search_w, sel, shown, on_paste, host) = (
                self.clone(),
                list.clone(),
                search.downgrade(),
                sel.clone(),
                shown.clone(),
                on_paste.clone(),
                host.clone(),
            );
            Rc::new(move || {
                let Some(search) = search_w.upgrade() else {
                    return;
                };
                while let Some(c) = list.first_child() {
                    list.remove(&c);
                }
                let texts = panel.texts_in_order();
                let idxs = tcode_core::clipboard::matching_indices(&texts, &search.text());
                if sel.get() >= idxs.len() {
                    sel.set(idxs.len().saturating_sub(1));
                }
                *shown.borrow_mut() = idxs.clone();
                if idxs.is_empty() {
                    let hint = Label::new(Some("No matching clips"));
                    hint.set_xalign(0.0);
                    hint.add_css_class("clip-empty");
                    list.append(&hint);
                    return;
                }
                for (row_i, &ei) in idxs.iter().enumerate() {
                    let text = texts[ei].clone();
                    let row = Button::new();
                    row.add_css_class("clip-pal-row");
                    if panel.entry_is_pinned(ei) {
                        row.add_css_class("pinned");
                    }
                    if row_i == sel.get() {
                        row.add_css_class("active");
                    }
                    let lbl = Label::new(Some(&preview(&text)));
                    lbl.set_xalign(0.0);
                    lbl.set_wrap(true);
                    lbl.set_wrap_mode(WrapMode::WordChar);
                    lbl.set_lines(2);
                    lbl.set_ellipsize(EllipsizeMode::End);
                    lbl.add_css_class("clip-text");
                    row.set_child(Some(&lbl));
                    let (op, hs, t) = (on_paste.clone(), host.clone(), text.clone());
                    row.connect_clicked(move |_| {
                        op(&t);
                        if let Some(h) = hs.upgrade() {
                            h.close();
                        }
                    });
                    list.append(&row);
                }
            })
        };

        // Refilter as the user types (reset the selection to the top).
        {
            let (render, sel) = (render.clone(), sel.clone());
            search.connect_changed(move |_| {
                sel.set(0);
                render();
            });
        }
        // Each time the palette is shown: clear the query, re-render, focus search.
        {
            let (render, sel) = (render.clone(), sel.clone());
            let search_w = search.downgrade();
            root.connect_map(move |_| {
                sel.set(0);
                if let Some(search) = search_w.upgrade() {
                    search.set_text("");
                    search.grab_focus();
                }
                render();
            });
        }
        // Keyboard: ↑/↓ move the selection; Enter pastes (Ctrl+Enter copies only).
        // Capture phase on the palette *root* (a strict ancestor of the focused
        // search box) so these keys are handled before GtkEntry's own activate
        // swallows Return — otherwise Enter does nothing.
        {
            let key = EventControllerKey::new();
            key.set_propagation_phase(gtk4::PropagationPhase::Capture);
            let (panel, sel, shown, render, on_paste, host) = (
                self.clone(),
                sel.clone(),
                shown.clone(),
                render.clone(),
                on_paste.clone(),
                host.clone(),
            );
            key.connect_key_pressed(move |_, kv, _, mods| {
                let n = shown.borrow().len();
                match kv {
                    Key::Down => {
                        if n > 0 {
                            sel.set((sel.get() + 1).min(n - 1));
                            render();
                        }
                        Propagation::Stop
                    }
                    Key::Up => {
                        sel.set(sel.get().saturating_sub(1));
                        render();
                        Propagation::Stop
                    }
                    Key::Return | Key::KP_Enter => {
                        let ei = shown.borrow().get(sel.get()).copied();
                        if let Some(ei) = ei {
                            if let Some(t) = panel.entry_text(ei) {
                                if mods.contains(ModifierType::CONTROL_MASK) {
                                    panel.copy_text(&t);
                                } else {
                                    on_paste(&t);
                                }
                            }
                        }
                        if let Some(h) = host.upgrade() {
                            h.close();
                        }
                        Propagation::Stop
                    }
                    _ => Propagation::Proceed,
                }
            });
            root.add_controller(key);
        }
        root
    }
}

fn make_private_dir(dir: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
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
