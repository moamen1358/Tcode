//! The terminal grid — a *resizable* layout built from standard `GtkPaned`
//! splits (drag any border to resize). Panes are arranged in the balanced shape
//! from `tessera_core::grid::layout`, realized as nested Paned chains. Adding a
//! pane (`+` / `Alt+n`) or a shell exiting rebuilds the split tree.
//!
//! State lives behind `Rc<RefCell<GridInner>>` so widget callbacks can mutate it;
//! callbacks hold a `Weak` to avoid leaks across re-grid.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box as GtkBox, EventControllerFocus, Orientation, Paned};
use tessera_core::config::Config;
use tessera_core::grid::{coords, flat_index, layout, neighbor, Dir};
use vte4::prelude::*; // TerminalExt: connect_child_exited

use crate::pane::Pane;

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
    resize_timer: Rc<Cell<Option<glib::SourceId>>>,
}

/// Build a right-nested Paned chain over `items`. Returns the root widget and the
/// created Paneds tagged with orientation + the equal-split divisor.
fn chain(orient: Orientation, items: &[gtk4::Widget], divisor: usize) -> (gtk4::Widget, Vec<PanedInfo>) {
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

        if self.zoomed {
            let f = self.focus;
            self.panes[f].root.set_hexpand(true);
            self.panes[f].root.set_vexpand(true);
            self.container.append(&self.panes[f].root);
        } else {
            let (tree, paneds) = build_tree(&self.panes);
            tree.set_hexpand(true);
            tree.set_vexpand(true);
            self.container.append(&tree);
            self.paneds = paneds;
            self.set_positions();

            // Dragging a divider reflows the terminals; with a wrapping prompt
            // that can leave stale prompt fragments. After the drag settles, send
            // a clean redraw (Ctrl+L) to each terminal.
            let weak = self.self_weak.clone();
            let timer = self.resize_timer.clone();
            for (paned, _, _) in &self.paneds {
                let weak = weak.clone();
                let timer = timer.clone();
                paned.connect_position_notify(move |_| {
                    if let Some(id) = timer.take() {
                        id.remove();
                    }
                    let weak2 = weak.clone();
                    let timer2 = timer.clone();
                    let id = glib::timeout_add_local_once(Duration::from_millis(150), move || {
                        timer2.set(None);
                        if let Some(inner) = weak2.upgrade() {
                            for p in &inner.borrow().panes {
                                p.feed_text("\u{000c}"); // Ctrl+L
                            }
                        }
                    });
                    timer.set(Some(id));
                });
            }
        }
        self.refresh_active();
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
        }
        if self.panes.is_empty() {
            self.window.close();
            return;
        }
        self.zoomed = false;
        self.rebuild();
    }
}

pub struct Grid {
    pub root: GtkBox,
    inner: Rc<RefCell<GridInner>>,
}

impl Grid {
    pub fn new(n: usize, cfg: &Config, window: &ApplicationWindow) -> Grid {
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
        let (id, cfg) = {
            let mut g = self.inner.borrow_mut();
            let id = g.next_id;
            g.next_id += 1;
            (id, g.cfg.clone())
        };
        let pane = Pane::new(&cfg, id);

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
        pane.terminal.add_controller(controller);

        let weak2 = Rc::downgrade(&self.inner);
        pane.terminal.connect_child_exited(move |_t, _status| {
            if let Some(inner) = weak2.upgrade() {
                inner.borrow_mut().remove_by_id(id);
            }
        });

        pane
    }

    pub fn add_pane(&self) {
        let pane = self.make_pane();
        let mut g = self.inner.borrow_mut();
        g.panes.push(pane);
        g.focus = g.panes.len() - 1;
        g.zoomed = false;
        g.rebuild();
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
        g.rebuild();
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

    /// Apply equal split positions once the container has a real size (after map).
    pub fn relayout_positions(&self) {
        self.inner.borrow().set_positions();
    }
}
