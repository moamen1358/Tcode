//! The terminal grid — a *resizable* layout built from standard `GtkPaned`
//! splits (drag any border to resize). Panes are arranged in the balanced shape
//! from `loom_core::grid::layout`, realized as nested Paned chains. Adding a
//! pane (`+` / `Alt+n`) or a shell exiting rebuilds the split tree.
//!
//! State lives behind `Rc<RefCell<GridInner>>` so widget callbacks can mutate it;
//! callbacks hold a `Weak` to avoid leaks across re-grid.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk4::glib;
use gtk4::pango::FontDescription;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box as GtkBox, EventControllerFocus, Orientation, Paned};
use loom_core::config::Config;
use loom_core::grid::{coords, flat_index, layout, neighbor, Dir};
use vte4::prelude::*; // TerminalExt: connect_child_exited

use crate::pane::{OpenFn, Pane};

/// A Paned, whether it splits horizontally, and the divisor for an equal split.
type PanedInfo = (Paned, bool, usize);

struct GridInner {
    container: GtkBox,
    panes: Vec<Pane>,
    paneds: Vec<PanedInfo>,
    focus: usize,
    zoomed: bool,
    cfg: Config,
    window: ApplicationWindow,
    next_id: u64,
    self_weak: Weak<RefCell<GridInner>>,
    /// While a height (vertical) resize or a zoom is in flight, panes drop their
    /// scrollback so VTE can't flood the growing pane with pulled-up history.
    resize_timer: Rc<Cell<Option<glib::SourceId>>>,
    resizing: Rc<Cell<bool>>,
    /// Opens a Ctrl+clicked terminal path in the editor/viewer panel.
    on_open: OpenFn,
}

/// Build a right-nested Paned chain over `items`. Returns the root widget and the
/// created Paneds tagged with orientation + the equal-split divisor.
fn chain(
    orient: Orientation,
    items: &[gtk4::Widget],
    divisor: usize,
) -> (gtk4::Widget, Vec<PanedInfo>) {
    if items.len() == 1 {
        return (items[0].clone(), Vec::new());
    }
    let is_h = orient == Orientation::Horizontal;
    let mut paneds = Vec::new();
    let mut acc = items[items.len() - 1].clone();
    for item in items[..items.len() - 1].iter().rev() {
        let p = Paned::new(orient);
        // Thin handle (a line, not a wide gap) between terminals.
        p.set_wide_handle(false);
        p.set_resize_start_child(true);
        p.set_resize_end_child(true);
        p.set_shrink_start_child(false);
        p.set_shrink_end_child(false);
        p.set_start_child(Some(item));
        p.set_end_child(Some(&acc));
        paneds.push((p.clone(), is_h, divisor));
        acc = p.upcast();
    }
    (acc, paneds)
}

/// Build the full split tree (rows of horizontal chains, stacked vertically).
fn build_tree(panes: &[Pane]) -> (gtk4::Widget, Vec<PanedInfo>) {
    let widths = layout(panes.len());
    let rows = widths.len();
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
        let (row_w, mut row_paneds) = chain(Orientation::Horizontal, &items, w);
        all.append(&mut row_paneds);
        row_widgets.push(row_w);
    }
    let (root, mut v_paneds) = chain(Orientation::Vertical, &row_widgets, rows);
    all.append(&mut v_paneds);
    (root, all)
}

/// Drop every terminal's scrollback for the duration of a resize drag — so VTE
/// can't flood/garble the buffer while re-wrapping (and a SIGWINCH-aware prompt
/// can't stack reprints) — then restore it ~60ms after the drag settles. Shared
/// by the grid's own dividers and external ones (the editor / sidebar dividers,
/// which resize the terminals too but aren't part of the grid's paned tree).
fn suppress_reflow(
    weak: &Weak<RefCell<GridInner>>,
    timer: &Rc<Cell<Option<glib::SourceId>>>,
    resizing: &Rc<Cell<bool>>,
) {
    if let Some(id) = timer.take() {
        id.remove();
    }
    if !resizing.replace(true) {
        if let Some(inner) = weak.upgrade() {
            // try_borrow: a programmatic position change can fire this while the
            // grid is mid-borrow_mut (e.g. rebuild).
            if let Ok(g) = inner.try_borrow() {
                for p in &g.panes {
                    p.set_resizing(true);
                }
            }
        }
    }
    let weak2 = weak.clone();
    let timer2 = timer.clone();
    let resizing2 = resizing.clone();
    let id = glib::timeout_add_local_once(Duration::from_millis(60), move || {
        timer2.set(None);
        resizing2.set(false);
        if let Some(inner) = weak2.upgrade() {
            if let Ok(g) = inner.try_borrow() {
                for p in &g.panes {
                    p.set_resizing(false);
                }
            }
        }
    });
    timer.set(Some(id));
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

    /// Set each Paned to an equal split for its chain (only once sized).
    fn set_positions(&self) {
        let w = self.container.width();
        let h = self.container.height();
        for (paned, is_h, div) in &self.paneds {
            let dim = if *is_h { w } else { h };
            if dim > 1 && *div > 1 {
                paned.set_position((dim / *div as i32).max(1));
            }
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
        self.paneds.clear();
        if self.panes.is_empty() {
            return;
        }
        if self.focus >= self.panes.len() {
            self.focus = self.panes.len() - 1;
        }

        // The full grid tree is always built; zoom just hides the non-focused
        // panes (a hidden GtkPaned child collapses, so the focused pane fills the
        // space). This avoids re-parenting a pane, which paints the VTE terminal
        // blank on the GL renderer.
        {
            let (tree, paneds) = build_tree(&self.panes);
            tree.set_hexpand(true);
            tree.set_vexpand(true);
            self.container.append(&tree);
            self.paneds = paneds;
            self.set_positions();

            // Resizing terminals live has two artifacts: a height grow makes VTE
            // pull scrollback up to fill the pane (floods it with old output), and
            // a width change makes VTE re-wrap the whole scrollback buffer — during
            // a fast drag those partial re-wraps render the prompt garbled over
            // itself. Drop scrollback to 0 for the duration of any drag and restore
            // it shortly after it settles, so neither happens.
            for (paned, _, _) in &self.paneds {
                let weak = self.self_weak.clone();
                let timer = self.resize_timer.clone();
                let resizing = self.resizing.clone();
                paned.connect_position_notify(move |_| {
                    suppress_reflow(&weak, &timer, &resizing);
                });
            }
        }
        for p in &self.panes {
            p.attach_ring();
        }
        self.apply_zoom();
        self.refresh_active();
    }

    /// Zoom by hiding the sibling at every level along the focused pane's path to
    /// the root, so each ancestor Paned collapses around it and the focused pane
    /// fills the grid. Un-zoom restores everything. No re-parenting involved.
    fn apply_zoom(&self) {
        for p in &self.panes {
            p.root.set_visible(true);
        }
        for (paned, _, _) in &self.paneds {
            paned.set_visible(true);
        }
        if !self.zoomed {
            return;
        }
        let Some(focused) = self.panes.get(self.focus) else {
            return;
        };
        let mut current: gtk4::Widget = focused.root.clone().upcast();
        while let Some(parent) = current.parent() {
            let Some(paned) = parent.downcast_ref::<Paned>() else {
                break; // reached the container (a GtkBox), not a Paned
            };
            if let Some(start) = paned.start_child() {
                if start != current {
                    start.set_visible(false);
                }
            }
            if let Some(end) = paned.end_child() {
                if end != current {
                    end.set_visible(false);
                }
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
            // We're inside the grid's borrow_mut here (called from child_exited).
            // close-request -> save_current -> pane_count() would re-borrow the
            // same GridInner and panic, so defer the close until the borrow is gone.
            let win = self.window.clone();
            glib::idle_add_local_once(move || win.close());
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
    pub fn new(n: usize, cfg: &Config, window: &ApplicationWindow, on_open: OpenFn) -> Grid {
        let container = GtkBox::new(Orientation::Vertical, 0);
        container.add_css_class("grid-root");

        let inner = Rc::new(RefCell::new(GridInner {
            container: container.clone(),
            panes: Vec::new(),
            paneds: Vec::new(),
            focus: 0,
            zoomed: false,
            cfg: cfg.clone(),
            window: window.clone(),
            next_id: 0,
            self_weak: Weak::new(),
            resize_timer: Rc::new(Cell::new(None)),
            resizing: Rc::new(Cell::new(false)),
            on_open,
        }));
        inner.borrow_mut().self_weak = Rc::downgrade(&inner);

        let grid = Grid {
            root: container,
            inner,
        };
        for _ in 0..n.clamp(1, 16) {
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

    /// Suppress terminal reflow for a resize driven from outside the grid — the
    /// editor or sidebar divider, which resizes the terminals (incl. a zoomed one)
    /// but isn't one of the grid's own paneds. Call on each of their position
    /// changes; mirrors the grid's internal divider handling.
    pub fn on_external_resize(&self) {
        let g = self.inner.borrow();
        suppress_reflow(&g.self_weak, &g.resize_timer, &g.resizing);
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
        {
            let mut g = self.inner.borrow_mut();
            if g.panes.is_empty() {
                return;
            }
            // Zoom grows the focused pane in both dimensions; drop scrollback so
            // VTE doesn't flood it with pulled-up history (restored once settled).
            for p in &g.panes {
                p.set_resizing(true);
            }
            g.zoomed = !g.zoomed;
            g.apply_zoom();
            if let Some(p) = g.panes.get(g.focus) {
                p.grab_focus();
            }
        }
        let inner = self.inner.clone();
        glib::timeout_add_local_once(Duration::from_millis(60), move || {
            for p in &inner.borrow().panes {
                p.set_resizing(false);
            }
        });
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

    /// Apply equal split positions once the container has a real size (after map).
    pub fn relayout_positions(&self) {
        self.inner.borrow().set_positions();
    }

    /// Grid container size in pixels — used by the session-restore poll to know
    /// when the terminal area has reached its final width before sizing splits.
    pub fn container_size(&self) -> (i32, i32) {
        let g = self.inner.borrow();
        (g.container.width(), g.container.height())
    }

    /// Current divider positions as ratios of the container dimension (in paned
    /// order), so a session can restore exactly how the terminals were resized.
    pub fn split_ratios(&self) -> Vec<f64> {
        let g = self.inner.borrow();
        let (w, h) = (g.container.width(), g.container.height());
        g.paneds
            .iter()
            .map(|(paned, is_h, _)| {
                let dim = if *is_h { w } else { h };
                if dim > 1 {
                    paned.position() as f64 / dim as f64
                } else {
                    0.5
                }
            })
            .collect()
    }

    /// Restore divider positions from saved ratios (paned order must match, which
    /// it does for a given pane count). Falls back to leaving a paned untouched
    /// when its ratio is missing or out of range.
    pub fn apply_split_ratios(&self, ratios: &[f64]) {
        let g = self.inner.borrow();
        let (w, h) = (g.container.width(), g.container.height());
        for ((paned, is_h, _), &ratio) in g.paneds.iter().zip(ratios) {
            let dim = if *is_h { w } else { h };
            if dim > 1 && ratio > 0.0 && ratio < 1.0 {
                paned.set_position((ratio * dim as f64).round() as i32);
            }
        }
    }
}
