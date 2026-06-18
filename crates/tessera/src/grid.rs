//! The tiled grid of panes — a *dynamic* model that can add panes (the `+`
//! button / `Alt+n`) and remove them when their shell exits, re-tiling the
//! survivors with `tessera_core::grid::layout`. State lives behind an
//! `Rc<RefCell<GridInner>>` so widget callbacks (focus, child-exit) can mutate
//! it; callbacks hold a `Weak` to avoid keeping the grid alive after a re-grid.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box as GtkBox, EventControllerFocus, Orientation};
use vte4::prelude::*; // TerminalExt: connect_child_exited
use tessera_core::config::Config;
use tessera_core::grid::{coords, flat_index, layout, neighbor, Dir};

use crate::pane::Pane;

struct GridInner {
    root: GtkBox,
    rows: Vec<GtkBox>,
    panes: Vec<Pane>,
    focus: usize,
    zoomed: bool,
    gap: i32,
    cfg: Config,
    window: ApplicationWindow,
    next_id: u64,
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

    /// Rebuild the row boxes for the current pane count, re-parenting the
    /// existing (live) pane widgets. Resets any zoom.
    fn relayout(&mut self) {
        self.zoomed = false;
        for p in &self.panes {
            p.root.unparent();
        }
        while let Some(child) = self.root.first_child() {
            self.root.remove(&child);
        }
        self.rows.clear();

        if self.panes.is_empty() {
            return;
        }
        if self.focus >= self.panes.len() {
            self.focus = self.panes.len() - 1;
        }

        let widths = layout(self.panes.len());
        let mut idx = 0;
        for &w in &widths {
            let row = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .homogeneous(true)
                .spacing(self.gap)
                .build();
            for _ in 0..w {
                if let Some(pane) = self.panes.get(idx) {
                    pane.root.set_hexpand(true);
                    pane.root.set_vexpand(true);
                    row.append(&pane.root);
                    idx += 1;
                }
            }
            self.root.append(&row);
            self.rows.push(row);
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

    fn toggle_zoom(&mut self) {
        if self.panes.is_empty() {
            return;
        }
        let z = !self.zoomed;
        self.zoomed = z;
        let widths = layout(self.panes.len());
        let (fr, _) = coords(&widths, self.focus);
        for (r, row) in self.rows.iter().enumerate() {
            row.set_visible(!z || r == fr);
        }
        for (i, p) in self.panes.iter().enumerate() {
            p.root.set_visible(!z || i == self.focus);
        }
        if let Some(p) = self.panes.get(self.focus) {
            p.grab_focus();
        }
    }

    fn remove_by_id(&mut self, id: u64) {
        if let Some(pos) = self.panes.iter().position(|p| p.id == id) {
            self.panes[pos].root.unparent();
            self.panes.remove(pos); // drops the Pane -> terminal + PTY torn down
            if self.focus > pos {
                self.focus -= 1;
            }
        }
        if self.panes.is_empty() {
            // Exited the last terminal -> nothing left to show; close the app.
            self.window.close();
            return;
        }
        self.relayout();
    }
}

pub struct Grid {
    pub root: GtkBox,
    inner: Rc<RefCell<GridInner>>,
}

impl Grid {
    pub fn new(n: usize, cfg: &Config, window: &ApplicationWindow) -> Grid {
        let gap = cfg.gap as i32;
        let root = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .homogeneous(true)
            .spacing(gap)
            .build();
        root.add_css_class("grid-root");
        root.set_margin_top(gap);
        root.set_margin_bottom(gap);
        root.set_margin_start(gap);
        root.set_margin_end(gap);

        let inner = Rc::new(RefCell::new(GridInner {
            root: root.clone(),
            rows: Vec::new(),
            panes: Vec::new(),
            focus: 0,
            zoomed: false,
            gap,
            cfg: cfg.clone(),
            window: window.clone(),
            next_id: 0,
        }));

        let grid = Grid { root, inner };
        for _ in 0..n.clamp(1, 16) {
            let pane = grid.make_pane();
            grid.inner.borrow_mut().panes.push(pane);
        }
        grid.inner.borrow_mut().relayout();
        grid
    }

    /// Create a pane and wire its focus + exit callbacks to the shared state.
    fn make_pane(&self) -> Pane {
        let (id, cfg) = {
            let mut g = self.inner.borrow_mut();
            let id = g.next_id;
            g.next_id += 1;
            (id, g.cfg.clone())
        };
        let pane = Pane::new(&cfg, id);

        // Focus (click or keyboard) -> highlight. try_borrow_mut so our own
        // grab_focus (which can re-emit `enter`) doesn't double-borrow + panic.
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

        // Child exited -> remove this pane and re-tile the rest.
        let weak2 = Rc::downgrade(&self.inner);
        pane.terminal.connect_child_exited(move |_t, _status| {
            if let Some(inner) = weak2.upgrade() {
                inner.borrow_mut().remove_by_id(id);
            }
        });

        pane
    }

    /// Add a new terminal pane and focus it.
    pub fn add_pane(&self) {
        let pane = self.make_pane();
        let mut g = self.inner.borrow_mut();
        g.panes.push(pane);
        g.focus = g.panes.len() - 1;
        g.relayout();
    }

    pub fn move_focus(&self, dir: Dir) {
        self.inner.borrow_mut().move_focus(dir);
    }

    pub fn toggle_zoom(&self) {
        self.inner.borrow_mut().toggle_zoom();
    }

    /// Type text into the currently focused pane (used by the sidebar + drop).
    pub fn feed_focused(&self, text: &str) {
        let g = self.inner.borrow();
        if let Some(p) = g.panes.get(g.focus) {
            p.feed_text(text);
        }
    }

    /// Grab keyboard focus on the focused pane (used after the window maps).
    pub fn grab_focused(&self) {
        let g = self.inner.borrow();
        if let Some(p) = g.panes.get(g.focus) {
            p.grab_focus();
        }
    }
}
