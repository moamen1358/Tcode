//! The terminal grid — a *fixed*, equal-split layout: every pane gets the same
//! size, determined purely by the pane count (no draggable dividers). Panes are
//! arranged in the balanced shape from `tcode_core::grid::layout`, realized as
//! nested homogeneous `GtkBox`es (rows of panes stacked into columns). Adding a
//! pane (`+` / `Alt+n`) or a shell exiting rebuilds the tree.
//!
//! State lives behind `Rc<RefCell<GridInner>>` so widget callbacks can mutate it;
//! callbacks hold a `Weak` to avoid leaks across re-grid.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib;
use gtk4::pango::FontDescription;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, EventControllerFocus, Orientation};
use tcode_core::config::Config;
use tcode_core::grid::{coords, flat_index, layout, neighbor, Dir};
use vte4::prelude::*; // TerminalExt: connect_child_exited

use crate::pane::{OpenFn, Pane};

pub type EmptyFn = Rc<dyn Fn()>;

struct GridInner {
    container: GtkBox,
    panes: Vec<Pane>,
    /// The intermediate split boxes (rows + the column that stacks them), kept so
    /// zoom can toggle their visibility. Equal-split is automatic (homogeneous).
    splits: Vec<GtkBox>,
    focus: usize,
    zoomed: bool,
    cfg: Config,
    next_id: u64,
    /// Pending "reveal terminals" timer, reset on each resize event so the panes
    /// are revealed once the window stops resizing (see `Grid::freeze_for_resize`).
    resize_timer: Cell<Option<glib::SourceId>>,
    /// Whether a terminal held keyboard focus when the current freeze began,
    /// captured once at gesture start so the reveal restores focus only if the
    /// freeze actually took it (and doesn't yank focus out of the sidebar/editor).
    resize_focus: Cell<bool>,
    /// Opens a Ctrl+clicked terminal path in the editor/viewer panel.
    on_open: OpenFn,
    /// Called when the final pane exits. App-level state decides whether that closes
    /// the visible window or just removes a hidden session.
    on_empty: EmptyFn,
}

/// Lay `items` out as one equal-split (homogeneous) box along `orient`. Returns
/// the root widget and the created box (if any), so the caller can collect it.
/// One item needs no box; zero items yields an inert placeholder.
fn chain(orient: Orientation, items: &[gtk4::Widget]) -> (gtk4::Widget, Vec<GtkBox>) {
    if items.is_empty() {
        return (GtkBox::new(orient, 0).upcast(), Vec::new());
    }
    if items.len() == 1 {
        items[0].set_hexpand(true);
        items[0].set_vexpand(true);
        return (items[0].clone(), Vec::new());
    }
    let b = GtkBox::new(orient, 0);
    b.set_homogeneous(true); // every child the same size — fixed equal split
    b.set_hexpand(true);
    b.set_vexpand(true);
    for item in items {
        item.set_hexpand(true);
        item.set_vexpand(true);
        b.append(item);
    }
    (b.clone().upcast(), vec![b])
}

/// Build the full grid tree (rows of horizontal boxes, stacked in a column box).
fn build_tree(panes: &[Pane]) -> (gtk4::Widget, Vec<GtkBox>) {
    let widths = layout(panes.len());
    let mut all = Vec::new();
    let mut row_widgets = Vec::new();
    let mut idx = 0;
    for &w in &widths {
        let items: Vec<gtk4::Widget> = (0..w)
            .map(|_| {
                let r = panes[idx].root.clone().upcast::<gtk4::Widget>();
                idx += 1;
                r
            })
            .collect();
        let (row_w, mut row_boxes) = chain(Orientation::Horizontal, &items);
        all.append(&mut row_boxes);
        row_widgets.push(row_w);
    }
    let (root, mut col_boxes) = chain(Orientation::Vertical, &row_widgets);
    all.append(&mut col_boxes);
    (root, all)
}

/// Hide every terminal for the duration of a resize, then reveal it once the
/// resize settles (~220ms after the last resize event). While a terminal is hidden
/// it gets no allocation, so VTE doesn't resize its PTY on each drag step — the
/// child process sees one SIGWINCH on reveal rather than a burst, so a TUI like
/// Claude Code (which reprints on every SIGWINCH) repaints once instead of stacking
/// copies. The `.pane` container (same background) holds the layout, so a frozen
/// pane reads as a solid block. Shared by window resizes (the surface hook) and
/// divider drags (the position-notify hook), the latter value-guarded so this
/// function's own hide/reveal can't feed back into a loop.
fn freeze_terminals(inner: &Rc<RefCell<GridInner>>) {
    // try_borrow: a resize can fire mid-borrow_mut (e.g. while rebuilding).
    let Ok(g) = inner.try_borrow() else {
        return;
    };
    let pending = g.resize_timer.take();
    if let Some(id) = pending {
        id.remove();
    } else {
        // Start of a resize gesture (no reveal pending): the panes are still
        // visible, so capture whether a terminal actually holds focus right now.
        // Mid-drag events (pending is Some) run after the panes are hidden, so they
        // must not overwrite this — focus has already left the hidden terminals.
        g.resize_focus.set(g.panes.iter().any(|p| p.has_focus()));
    }
    for p in &g.panes {
        p.set_resizing(true); // no-op if already hidden
    }
    let weak = Rc::downgrade(inner);
    let id = glib::timeout_add_local_once(Duration::from_millis(220), move || {
        // Weak: if the session is torn down mid-resize, let GridInner drop now rather
        // than pinning every pane (and its live PTY) alive until this one-shot fires.
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let Ok(g) = inner.try_borrow() else {
            return;
        };
        g.resize_timer.set(None);
        for p in &g.panes {
            p.set_resizing(false);
        }
        // Restore focus to the active pane ONLY if the freeze actually took it from
        // a terminal; otherwise leave focus where the user put it (sidebar/editor),
        // so resizing while typing in a side panel doesn't yank focus to a terminal.
        if g.resize_focus.get() {
            if let Some(p) = g.panes.get(g.focus) {
                p.grab_focus();
            }
        }
    });
    g.resize_timer.set(Some(id));
}

impl GridInner {
    fn refresh_active(&self) {
        for (i, p) in self.panes.iter().enumerate() {
            p.set_active(i == self.focus);
        }
        if let Some(p) = self.panes.get(self.focus) {
            p.grab_focus();
        }
    }

    fn rebuild(&mut self) {
        for p in &self.panes {
            p.detach_ring(); // avoid blank-GL-surface reparent of an overlay child
            p.root.unparent();
        }
        while let Some(child) = self.container.first_child() {
            self.container.remove(&child);
        }
        self.splits.clear();
        if self.panes.is_empty() {
            return;
        }
        if self.focus >= self.panes.len() {
            self.focus = self.panes.len() - 1;
        }

        // The full grid tree is always built; zoom just hides the non-focused panes
        // (a hidden homogeneous-box child collapses, so the focused pane fills the
        // space). This avoids re-parenting a pane, which paints the VTE terminal
        // blank on the GL renderer.
        let (tree, splits) = build_tree(&self.panes);
        tree.set_hexpand(true);
        tree.set_vexpand(true);
        self.container.append(&tree);
        self.splits = splits;

        for p in &self.panes {
            p.attach_ring();
        }
        self.apply_zoom();
        self.refresh_active();
    }

    /// Zoom by hiding every sibling along the focused pane's path to the root, so
    /// each ancestor box collapses around it (a homogeneous box splits only its
    /// *visible* children) and the focused pane fills the grid. Un-zoom restores
    /// everything. No re-parenting involved.
    fn apply_zoom(&self) {
        for p in &self.panes {
            p.root.set_visible(true);
        }
        for b in &self.splits {
            b.set_visible(true);
        }
        if !self.zoomed {
            return;
        }
        let Some(focused) = self.panes.get(self.focus) else {
            return;
        };
        let container: gtk4::Widget = self.container.clone().upcast();
        let mut current: gtk4::Widget = focused.root.clone().upcast();
        while let Some(parent) = current.parent() {
            if parent == container {
                break; // reached the grid root; don't touch anything above it
            }
            let Some(b) = parent.downcast_ref::<GtkBox>() else {
                break;
            };
            let mut child = b.first_child();
            while let Some(c) = child {
                let next = c.next_sibling();
                if c != current {
                    c.set_visible(false);
                }
                child = next;
            }
            current = parent;
        }
    }

    fn move_focus(&mut self, dir: Dir) {
        if self.zoomed || self.panes.is_empty() {
            return;
        }
        let widths = layout(self.panes.len());
        let (r, c) = coords(&widths, self.focus);
        let (nr, nc) = neighbor(&widths, r, c, dir);
        self.focus = flat_index(&widths, nr, nc).min(self.panes.len() - 1);
        self.refresh_active();
    }

    fn remove_by_id(&mut self, id: u64) {
        if let Some(pos) = self.panes.iter().position(|p| p.id == id) {
            self.panes[pos].root.unparent();
            self.panes.remove(pos);
            if self.focus > pos {
                self.focus -= 1;
            }
            // Clamp in case focus pointed at/past the removed pane (now out of range).
            self.focus = self.focus.min(self.panes.len().saturating_sub(1));
        }
        if self.panes.is_empty() {
            // We're inside the grid's borrow_mut here (called from child_exited), so
            // defer app-level cleanup until the borrow is gone.
            let on_empty = self.on_empty.clone();
            glib::idle_add_local_once(move || on_empty());
            return;
        }
        self.zoomed = false;
        self.rebuild();
    }
}

#[derive(Clone)]
pub struct Grid {
    pub root: GtkBox,
    inner: Rc<RefCell<GridInner>>,
}

impl Grid {
    pub fn new(n: usize, cfg: &Config, on_open: OpenFn, on_empty: EmptyFn) -> Grid {
        let container = GtkBox::new(Orientation::Vertical, 0);
        container.add_css_class("grid-root");

        let inner = Rc::new(RefCell::new(GridInner {
            container: container.clone(),
            panes: Vec::new(),
            splits: Vec::new(),
            focus: 0,
            zoomed: false,
            cfg: cfg.clone(),
            next_id: 0,
            resize_timer: Cell::new(None),
            resize_focus: Cell::new(false),
            on_open,
            on_empty,
        }));

        let grid = Grid {
            root: container,
            inner,
        };
        let n = n.clamp(1, 16);
        // Build `n` plain terminal panes. Each runs the user's login shell; an
        // optional `startup_command` from config is fed into every pane once it's
        // spawned at final size (see Pane::spawn_params).
        for _ in 0..n {
            let pane = grid.make_pane();
            grid.inner.borrow_mut().panes.push(pane);
        }
        grid.inner.borrow_mut().rebuild();
        grid
    }

    fn make_pane(&self) -> Pane {
        let (id, cfg, on_open) = {
            let mut g = self.inner.borrow_mut();
            let id = g.next_id;
            g.next_id += 1;
            (id, g.cfg.clone(), g.on_open.clone())
        };
        let pane = Pane::new(&cfg, id, on_open);

        let controller = EventControllerFocus::new();
        let weak = Rc::downgrade(&self.inner);
        controller.connect_enter(move |_| {
            if let Some(inner) = weak.upgrade() {
                if let Ok(mut g) = inner.try_borrow_mut() {
                    if let Some(pos) = g.panes.iter().position(|p| p.id == id) {
                        g.focus = pos;
                        for (i, p) in g.panes.iter().enumerate() {
                            p.set_active(i == pos);
                        }
                    }
                }
            }
        });
        // Drop the focus ring when the terminal loses focus (e.g. clicking the
        // sidebar or opening a file in the editor) so it isn't left highlighted.
        let weak_leave = Rc::downgrade(&self.inner);
        controller.connect_leave(move |_| {
            if let Some(inner) = weak_leave.upgrade() {
                if let Ok(g) = inner.try_borrow() {
                    if let Some(p) = g.panes.iter().find(|p| p.id == id) {
                        p.set_active(false);
                    }
                }
            }
        });
        pane.terminal.add_controller(controller);

        let weak2 = Rc::downgrade(&self.inner);
        pane.terminal.connect_child_exited(move |_t, _status| {
            let Some(inner) = weak2.upgrade() else {
                return;
            };
            if let Ok(mut g) = inner.try_borrow_mut() {
                g.remove_by_id(id);
                return;
            }
            // Inner was already borrowed (rare re-entrancy) — retry on idle so the
            // exited pane is never silently left in the grid with a dead PTY.
            let weak = weak2.clone();
            glib::idle_add_local_once(move || {
                if let Some(inner) = weak.upgrade() {
                    if let Ok(mut g) = inner.try_borrow_mut() {
                        g.remove_by_id(id);
                    }
                }
            });
        });

        pane
    }

    pub fn add_pane(&self) {
        // Bound the pane count like Grid::new's clamp(1, 16), so holding Alt+n
        // (key-repeat) can't spawn unbounded shells/PTYs.
        if self.inner.borrow().panes.len() >= 16 {
            return;
        }
        let pane = self.make_pane();
        let id = pane.id;
        {
            let mut g = self.inner.borrow_mut();
            g.panes.push(pane);
            g.focus = g.panes.len() - 1;
            g.zoomed = false;
            g.rebuild();
        }
        // Spawn the new pane's shell once it's been allocated at its final size
        // (the rest of the grid is already live and sized).
        self.spawn_when_sized(id);
    }

    /// Spawn the shell in every pane that hasn't started one yet. Called once the
    /// freshly-built grid has reached its final layout, so each prompt prints at
    /// the right size. Idempotent (already-spawned panes are skipped).
    pub fn spawn_pending(&self) {
        for p in &self.inner.borrow().panes {
            p.spawn();
        }
    }

    /// Spawn pane `id`'s shell once its terminal has settled at a real size. Used
    /// when adding a pane to a live grid; the build path uses `spawn_pending` after
    /// the whole layout settles.
    fn spawn_when_sized(&self, id: u64) {
        let weak = Rc::downgrade(&self.inner);
        let mut last = -1i32;
        let mut stable = 0u8;
        let mut ticks = 0u32;
        glib::timeout_add_local(Duration::from_millis(16), move || {
            ticks += 1;
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let Ok(g) = inner.try_borrow() else {
                return glib::ControlFlow::Continue;
            };
            let Some(p) = g.panes.iter().find(|p| p.id == id) else {
                return glib::ControlFlow::Break; // pane gone (e.g. removed)
            };
            let w = p.terminal.width();
            stable = if w > 1 && w == last { stable + 1 } else { 0 };
            last = w;
            if stable >= 3 || ticks > 120 {
                p.spawn();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }

    /// Whether two `Grid` handles refer to the same underlying grid. Used by the
    /// session-restore poll to detect that the page it was sizing has since been
    /// torn down and rebuilt, so it must not spawn shells into the stale grid.
    pub fn same(&self, other: &Grid) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// Number of live terminal panes (used to persist the session layout).
    pub fn pane_count(&self) -> usize {
        self.inner.borrow().panes.len()
    }

    /// Apply the base font (point size) and the UI zoom to every terminal.
    /// Non-destructive (no scrollback touch), so it's safe to re-apply when
    /// revealing a live session. Open-time garble is avoided by spawning the
    /// shells only once each pane has its final size (see `spawn_pending`).
    pub fn apply_font(&self, font: &str, size: u32, scale: f64) {
        let desc = FontDescription::from_string(&format!("{font} {size}"));
        for p in &self.inner.borrow().panes {
            p.terminal.set_font(Some(&desc));
            p.terminal.set_font_scale(scale);
        }
    }

    pub fn move_focus(&self, dir: Dir) {
        self.inner.borrow_mut().move_focus(dir);
    }

    pub fn toggle_zoom(&self) {
        let mut g = self.inner.borrow_mut();
        if g.panes.is_empty() {
            return;
        }
        g.zoomed = !g.zoomed;
        g.apply_zoom();
        if let Some(p) = g.panes.get(g.focus) {
            p.grab_focus();
        }
    }

    /// Hide the terminals while a resize is in flight, revealing them once it
    /// settles. See [`freeze_terminals`]. Used for window resizes (the surface
    /// hook in app.rs); divider drags drive [`freeze_terminals`] directly.
    pub fn freeze_for_resize(&self) {
        freeze_terminals(&self.inner);
    }

    pub fn feed_focused(&self, text: &str) {
        let g = self.inner.borrow();
        if let Some(p) = g.panes.get(g.focus) {
            p.feed_text(text);
        }
    }

    pub fn grab_focused(&self) {
        let g = self.inner.borrow();
        if let Some(p) = g.panes.get(g.focus) {
            p.grab_focus();
        }
    }

    /// Copy the focused terminal's selection to the clipboard.
    pub fn copy_focused(&self) {
        let g = self.inner.borrow();
        if let Some(p) = g.panes.get(g.focus) {
            p.copy();
        }
    }

    /// Paste the clipboard into the focused terminal.
    pub fn paste_focused(&self) {
        let g = self.inner.borrow();
        if let Some(p) = g.panes.get(g.focus) {
            p.paste();
        }
    }

    /// No-op: the grid is a fixed equal split (homogeneous boxes), so there are no
    /// divider positions to set. Kept because the session-restore poll calls it.
    pub fn relayout_positions(&self) {}

    /// Grid container size in pixels — used by the session-restore poll to know
    /// when the terminal area has reached its final size before spawning shells.
    pub fn container_size(&self) -> (i32, i32) {
        let g = self.inner.borrow();
        (g.container.width(), g.container.height())
    }

    /// Empty: a fixed equal-split grid has no divider positions to persist.
    pub fn split_ratios(&self) -> Vec<f64> {
        Vec::new()
    }

    /// No-op: nothing to restore for a fixed equal-split grid. (Sessions saved by
    /// older, resizable versions may still carry ratios; they're simply ignored.)
    pub fn apply_split_ratios(&self, _ratios: &[f64]) {}
}
