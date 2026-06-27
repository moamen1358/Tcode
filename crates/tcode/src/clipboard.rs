//! Clipboard history, surfaced through the compact **Alt+V palette** (`palette`):
//! a one-line list of text copied from any app while Tcode runs. Clicking a row
//! copies it back to the system clipboard, the pin keeps it at the top, and the
//! close icon removes it from history. Type to filter; ↑/↓ move the keyboard
//! selection, Enter copies, Ctrl+P toggles pinning, Ctrl+Delete removes, and Esc
//! closes the overlay. When
//! `clipboard_persist` is on, text, pin state, and capture time survive restarts.
//!
//! `entries` is the source of truth; the palette renders a filtered snapshot of it
//! and every change re-saves the history file. (`build`/`rebuild`/`build_card`
//! construct a legacy docked sidebar strip that is no longer parented — the
//! palette is the only surfaced UI.)

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtk4::gdk::{Display, Key, ModifierType};
use gtk4::glib::Propagation;
use gtk4::pango::{EllipsizeMode, WrapMode};
use gtk4::prelude::*;
use gtk4::{
    gio, AlertDialog, Align, Box as GtkBox, Button, EventControllerKey, Label, Orientation,
    PolicyType, ScrolledWindow, SearchEntry, Separator,
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
    captured_at: i64,
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
            // Re-copying something already in history shouldn't pile up duplicates.
            // A matching pinned entry already sits at the top, so leave it be; a
            // matching unpinned one is removed so the fresh copy floats back to the
            // top of the unpinned block with an updated capture time.
            if let Some(pos) = entries.iter().position(|e| e.text == text) {
                if entries[pos].pinned {
                    return;
                }
                entries.remove(pos);
            }
            let pinned = entries.iter().filter(|e| e.pinned).count();
            entries.insert(
                pinned,
                Entry {
                    id,
                    text,
                    pinned: false,
                    captured_at: unix_now(),
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
                // Unpinning returns the entry to its chronological spot among the
                // unpinned (newest-first), not the top — it isn't freshly copied.
                let pinned = entries.iter().filter(|e| e.pinned).count();
                let mut at = pinned;
                while at < entries.len() && entries[at].captured_at > entry.captured_at {
                    at += 1;
                }
                entries.insert(at, entry);
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
        // The history is surfaced through the Alt+V palette, not this strip, so the
        // strip widget is never parented into the window. Skip rebuilding its cards
        // while it's unshown — pure waste otherwise (callers still update `entries`,
        // which is what the palette reads). Robust if it's ever docked again.
        if self.root.parent().is_none() {
            return;
        }
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
            let Some((pinned, captured_at, len)) = parse_history_header(header, unix_now()) else {
                break;
            };
            let start = nl + 1;
            // Checked: a corrupt/hand-edited header with a huge length must not
            // overflow (debug panic) or wrap past the guard into an invalid slice.
            let Some(end) = start.checked_add(len).filter(|&e| e <= data.len()) else {
                break;
            };
            let Ok(text) = std::str::from_utf8(&data[start..end]) else {
                // Recoverable: the length prefix was valid, so we know exactly where
                // this record ends — skip just this bad-UTF-8 entry and resync at the
                // next record rather than dropping the whole tail.
                eprintln!("tcode: skipping malformed clipboard history record");
                i = end.saturating_add(1);
                continue;
            };
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            entries.push(Entry {
                id,
                text: text.to_string(),
                pinned,
                captured_at,
            });
            i = end.saturating_add(1); // skip the record's trailing newline
        }
        trim_unpinned(&mut entries);
    }

    /// Persist history as `{P|U} {unix_seconds} {byte_len}\n{text}\n`. The loader
    /// also accepts the older `{P|U} {byte_len}` header for backward compatibility.
    fn save(&self) {
        if !self.persist {
            return; // session-only history; never touch disk
        }
        let mut out: Vec<u8> = Vec::new();
        for e in self.entries.borrow().iter() {
            let bytes = e.text.as_bytes();
            // The size cap keeps one huge copy from bloating the file — but a pinned
            // entry is an explicit "keep this", so it persists regardless of size.
            if !e.pinned && bytes.len() > MAX_PERSIST_BYTES {
                continue;
            }
            let flag = if e.pinned { 'P' } else { 'U' };
            out.extend_from_slice(
                format!("{} {} {}\n", flag, e.captured_at, bytes.len()).as_bytes(),
            );
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

    // --- Alt+V clipboard palette (a compact view over the same model) ---

    /// `(id, text, pinned, capture time)` in display order (pinned-first). The
    /// palette renders this stable snapshot so keyboard actions always target the
    /// row the user can see, even if the system clipboard changes underneath it.
    fn snapshot(&self) -> Vec<(u64, String, bool, i64)> {
        self.entries
            .borrow()
            .iter()
            .map(|e| (e.id, e.text.clone(), e.pinned, e.captured_at))
            .collect()
    }

    /// Copy `text` to the system clipboard without recording it as a new entry.
    pub fn copy_text(&self, text: &str) {
        *self.last.borrow_mut() = text.to_string();
        if let Some(d) = Display::default() {
            d.clipboard().set_text(text);
        }
    }

    /// Build the Alt+V clipboard list. The visual surface intentionally contains
    /// rows only: click/Enter copies, pin moves a row to the top, the close icon
    /// deletes it, and Esc/Alt+V remain the invisible panel-close affordances.
    /// `_on_paste` and `_host` stay in the signature so session construction does
    /// not need a separate compatibility path for the former paste palette.
    pub fn palette(
        self: &Rc<Self>,
        _on_paste: Rc<dyn Fn(&str)>,
        _host: std::rc::Weak<crate::overlay::OverlayHost>,
    ) -> GtkBox {
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("clip-palette");
        root.set_size_request(760, -1);
        root.set_focusable(true);

        // Type-to-filter box (the README's "type to filter"); an empty query shows
        // every row. Focused on open so you can start filtering immediately.
        let search = SearchEntry::new();
        search.add_css_class("clip-pal-search");
        search.set_placeholder_text(Some("Search clipboard…"));
        root.append(&search);

        let list = GtkBox::new(Orientation::Vertical, 0);
        list.add_css_class("clip-pal-list");
        let scroll = ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .max_content_height(220)
            .propagate_natural_height(true)
            .build();
        root.append(&scroll);

        // Selection index into the currently shown rows.
        let sel = Rc::new(Cell::new(0usize));
        let shown: Rc<RefCell<Vec<(u64, String)>>> = Rc::new(RefCell::new(Vec::new()));
        let rows: Rc<RefCell<Vec<GtkBox>>> = Rc::new(RefCell::new(Vec::new()));
        let copied_id = Rc::new(Cell::new(None::<u64>));

        // Row callbacks need to redraw after copy/pin/delete. They hold only this
        // weak indirection, avoiding a list -> row -> render -> list cycle.
        let render_slot: RenderSlot = Rc::new(RefCell::new(None));
        let weak_render_slot = Rc::downgrade(&render_slot);

        let render: Rc<dyn Fn()> = {
            let (panel, search, list, sel, shown, rows, copied_id, weak_render_slot) = (
                self.clone(),
                search.clone(),
                list.clone(),
                sel.clone(),
                shown.clone(),
                rows.clone(),
                copied_id.clone(),
                weak_render_slot.clone(),
            );
            Rc::new(move || {
                while let Some(c) = list.first_child() {
                    list.remove(&c);
                }
                // Filter the history by the search box via the pure tcode-core matcher
                // (empty query → all rows). `shown` and the keyboard actions then work
                // off the filtered view, so Enter/pin/delete always hit a visible row.
                let all = panel.snapshot();
                let texts: Vec<String> = all.iter().map(|(_, t, _, _)| t.clone()).collect();
                let query = search.text();
                let snap: Vec<(u64, String, bool, i64)> =
                    tcode_core::clipboard::matching_indices(&texts, query.as_str())
                        .into_iter()
                        .map(|i| all[i].clone())
                        .collect();
                if sel.get() >= snap.len() {
                    sel.set(snap.len().saturating_sub(1));
                }
                *shown.borrow_mut() = snap
                    .iter()
                    .map(|(id, text, _, _)| (*id, text.clone()))
                    .collect();
                let mut new_rows: Vec<GtkBox> = Vec::with_capacity(snap.len());
                if snap.is_empty() {
                    // Tell "nothing copied yet" apart from "query matched nothing".
                    let hint = Label::new(Some(if all.is_empty() {
                        "Clipboard is empty"
                    } else {
                        "No matches"
                    }));
                    hint.set_xalign(0.0);
                    hint.add_css_class("clip-empty");
                    list.append(&hint);
                    *rows.borrow_mut() = new_rows;
                    return;
                }

                for (row_i, (id, text, pinned, captured_at)) in snap.iter().enumerate() {
                    let id = *id;
                    let text = text.clone();
                    let row = GtkBox::new(Orientation::Horizontal, 0);
                    row.add_css_class("clip-pal-row");
                    if *pinned {
                        row.add_css_class("pinned");
                    }
                    if row_i == sel.get() {
                        row.add_css_class("active");
                    }
                    let copy = Button::new();
                    copy.add_css_class("clip-pal-copy");
                    copy.set_hexpand(true);
                    copy.set_tooltip_text(Some(text.trim()));

                    let content = GtkBox::new(Orientation::Horizontal, 10);
                    let lbl = Label::new(Some(&preview(&text)));
                    lbl.set_xalign(0.0);
                    lbl.set_hexpand(true);
                    lbl.set_single_line_mode(true);
                    lbl.set_ellipsize(EllipsizeMode::End);
                    lbl.add_css_class("clip-pal-text");
                    content.append(&lbl);

                    let status = GtkBox::new(Orientation::Horizontal, 4);
                    status.add_css_class("clip-pal-status");
                    if copied_id.get() == Some(id) {
                        status.add_css_class("copied");
                        let check = gtk4::Image::from_icon_name("emblem-ok-symbolic");
                        check.set_pixel_size(12);
                        status.append(&check);
                        status.append(&Label::new(Some("Copied")));
                    } else {
                        status.append(&Label::new(Some(&format_capture_time(*captured_at))));
                    }
                    content.append(&status);
                    copy.set_child(Some(&content));
                    row.append(&copy);

                    let pin = Button::from_icon_name("view-pin-symbolic");
                    pin.add_css_class("clip-pal-pin");
                    pin.set_tooltip_text(Some(if *pinned { "Unpin" } else { "Pin to top" }));
                    row.append(&pin);

                    let delete = Button::from_icon_name("window-close-symbolic");
                    delete.add_css_class("clip-pal-delete");
                    delete.set_tooltip_text(Some("Remove from clipboard history"));
                    row.append(&delete);

                    {
                        let (panel, copied_id, weak_render_slot, sel) = (
                            panel.clone(),
                            copied_id.clone(),
                            weak_render_slot.clone(),
                            sel.clone(),
                        );
                        copy.connect_clicked(move |_| {
                            sel.set(row_i);
                            panel.copy_text(&text);
                            show_copied(&copied_id, id, &weak_render_slot);
                        });
                    }
                    {
                        let (panel, weak_render_slot, sel) =
                            (panel.clone(), weak_render_slot.clone(), sel.clone());
                        pin.connect_clicked(move |_| {
                            panel.toggle_pin(id);
                            sel.set(0);
                            call_render(&weak_render_slot);
                        });
                    }
                    {
                        let (panel, weak_render_slot) = (panel.clone(), weak_render_slot.clone());
                        delete.connect_clicked(move |_| {
                            panel.delete(id);
                            call_render(&weak_render_slot);
                        });
                    }

                    list.append(&row);
                    new_rows.push(row);
                }
                *rows.borrow_mut() = new_rows;
            })
        };
        *render_slot.borrow_mut() = Some(render);

        // Restyle only the old/new row for keyboard navigation and preserve scroll.
        let select: Rc<dyn Fn(usize)> = {
            let (rows, sel, scroll, list) =
                (rows.clone(), sel.clone(), scroll.clone(), list.clone());
            Rc::new(move |target: usize| {
                let (old_row, new_row, new) = {
                    let rows = rows.borrow();
                    if rows.is_empty() {
                        return;
                    }
                    let new = target.min(rows.len() - 1);
                    let old_row = rows.get(sel.get()).cloned();
                    (old_row, rows[new].clone(), new)
                };
                if let Some(old_row) = old_row {
                    old_row.remove_css_class("active");
                }
                new_row.add_css_class("active");
                sel.set(new);
                scroll_into_view(&scroll, &list, &new_row);
            })
        };

        // Every opening clears the filter, starts at the newest/pinned row, and
        // focuses the search box so you can type to filter right away.
        {
            let (render_slot, sel, scroll, search) = (
                render_slot.clone(),
                sel.clone(),
                scroll.clone(),
                search.clone(),
            );
            root.connect_map(move |_root| {
                sel.set(0);
                search.set_text("");
                if let Some(render) = render_slot.borrow().as_ref().cloned() {
                    render();
                }
                scroll.vadjustment().set_value(0.0);
                // OverlayHost::open() grabs focus onto the panel root the moment it
                // maps; an idle tick lets us reclaim it for the search box afterwards.
                let search = search.clone();
                gtk4::glib::idle_add_local_once(move || {
                    search.grab_focus();
                });
            });
        }

        // Re-filter as the query changes, snapping the selection back to the top.
        {
            let (weak_render_slot, sel) = (Rc::downgrade(&render_slot), sel.clone());
            search.connect_search_changed(move |_| {
                sel.set(0);
                call_render(&weak_render_slot);
            });
        }

        // Keyboard mirrors the visible row actions; the overlay host still owns Esc.
        {
            let key = EventControllerKey::new();
            key.set_propagation_phase(gtk4::PropagationPhase::Capture);
            let (panel, sel, shown, render_slot, select, copied_id) = (
                self.clone(),
                sel.clone(),
                shown.clone(),
                Rc::downgrade(&render_slot),
                select.clone(),
                copied_id.clone(),
            );
            key.connect_key_pressed(move |_, kv, _, state| {
                let ctrl = state.contains(ModifierType::CONTROL_MASK);
                match kv {
                    Key::Down => {
                        select(sel.get() + 1);
                        Propagation::Stop
                    }
                    Key::Up => {
                        select(sel.get().saturating_sub(1));
                        Propagation::Stop
                    }
                    Key::Return | Key::KP_Enter => {
                        let chosen = shown.borrow().get(sel.get()).cloned();
                        if let Some((id, text)) = chosen {
                            panel.copy_text(&text);
                            show_copied(&copied_id, id, &render_slot);
                        }
                        Propagation::Stop
                    }
                    // Ctrl-gated so bare p / Delete stay free to edit the search query.
                    Key::p if ctrl => {
                        let target = shown.borrow().get(sel.get()).map(|(id, _)| *id);
                        if let Some(id) = target {
                            panel.toggle_pin(id);
                            sel.set(0);
                            call_render(&render_slot);
                        }
                        Propagation::Stop
                    }
                    Key::Delete | Key::KP_Delete if ctrl => {
                        let target = shown.borrow().get(sel.get()).map(|(id, _)| *id);
                        if let Some(id) = target {
                            panel.delete(id);
                            call_render(&render_slot);
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

type RenderCallback = Rc<dyn Fn()>;
type RenderSlot = Rc<RefCell<Option<RenderCallback>>>;
type WeakRenderSlot = Weak<RefCell<Option<RenderCallback>>>;

fn call_render(slot: &WeakRenderSlot) {
    let render = slot
        .upgrade()
        .and_then(|slot| slot.borrow().as_ref().cloned());
    if let Some(render) = render {
        render();
    }
}

/// Show compact copy confirmation, then restore the row's capture time. A stale
/// timeout never clears feedback for a newer copy action.
fn show_copied(copied_id: &Rc<Cell<Option<u64>>>, id: u64, render: &WeakRenderSlot) {
    copied_id.set(Some(id));
    call_render(render);

    let copied_id = copied_id.clone();
    let render = render.clone();
    gtk4::glib::timeout_add_local_once(Duration::from_millis(1_800), move || {
        if copied_id.get() == Some(id) {
            copied_id.set(None);
            call_render(&render);
        }
    });
}

fn make_private_dir(dir: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
}

/// Scroll `scroll` the minimum needed to keep `row` (a child of `list`) fully
/// inside the viewport. The rows are already laid out by an earlier render, so
/// their bounds are valid here; before allocation `compute_bounds` is None and
/// this is a harmless no-op.
fn scroll_into_view(scroll: &ScrolledWindow, list: &GtkBox, row: &GtkBox) {
    let Some(bounds) = row.compute_bounds(list) else {
        return;
    };
    let vadj = scroll.vadjustment();
    let (y, h) = (bounds.y() as f64, bounds.height() as f64);
    let (top, page) = (vadj.value(), vadj.page_size());
    if y < top {
        vadj.set_value(y);
    } else if y + h > top + page {
        vadj.set_value((y + h - page).max(0.0));
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

/// One-line preview: collapse all whitespace and cap pathological copies before
/// Pango applies the final width-dependent ellipsis.
fn preview(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 200 {
        collapsed.chars().take(200).collect::<String>() + "…"
    } else {
        collapsed
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn format_capture_time(timestamp: i64) -> String {
    gtk4::glib::DateTime::from_unix_local(timestamp)
        .and_then(|date| date.format("%H:%M"))
        .map(|time| time.to_string())
        .unwrap_or_else(|_| "--:--".to_string())
}

/// Read both the current timestamped header and the legacy two-field header.
fn parse_history_header(header: &str, fallback_time: i64) -> Option<(bool, i64, usize)> {
    let mut parts = header.split_whitespace();
    let pinned = match parts.next()? {
        "P" => true,
        "U" => false,
        _ => return None,
    };
    let first = parts.next()?;
    let second = parts.next();
    if parts.next().is_some() {
        return None;
    }
    match second {
        Some(len) => Some((
            pinned,
            first.parse::<i64>().ok()?,
            len.parse::<usize>().ok()?,
        )),
        None => Some((pinned, fallback_time, first.parse::<usize>().ok()?)),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_history_header, preview};

    #[test]
    fn history_header_reads_timestamped_and_legacy_records() {
        assert_eq!(
            parse_history_header("P 1719480000 12", 7),
            Some((true, 1_719_480_000, 12))
        );
        assert_eq!(parse_history_header("U 12", 7), Some((false, 7, 12)));
        assert_eq!(parse_history_header("X 12", 7), None);
        assert_eq!(parse_history_header("P nope 12", 7), None);
    }

    #[test]
    fn preview_is_single_line_and_bounded() {
        assert_eq!(preview(" first\n second\tthird "), "first second third");
        let long = "x".repeat(250);
        let result = preview(&long);
        assert_eq!(result.chars().count(), 201);
        assert!(result.ends_with('…'));
    }
}
