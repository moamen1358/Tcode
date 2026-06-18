//! The tiled grid of panes: nested homogeneous boxes built from
//! `tessera_core::grid::layout`, with focus tracking (keyboard + click) and a
//! zoom toggle. Focus lives in a shared `Cell` so an `EventControllerFocus` on
//! each terminal keeps the active-pane highlight in sync however focus changed.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, EventControllerFocus, Orientation};
use tessera_core::config::Config;
use tessera_core::grid::{flat_index, layout, neighbor, Dir};

use crate::pane::Pane;

pub struct Grid {
    pub root: GtkBox,
    panes: Rc<Vec<Pane>>,
    rows: Vec<GtkBox>,
    widths: Rc<Vec<usize>>,
    focus: Rc<Cell<(usize, usize)>>,
    zoomed: Cell<bool>,
}

/// Apply the `.active-pane` highlight to the focused pane only.
fn set_active(panes: &[Pane], widths: &[usize], focus: (usize, usize)) {
    let active = flat_index(widths, focus.0, focus.1);
    for (i, p) in panes.iter().enumerate() {
        p.set_active(i == active);
    }
}

impl Grid {
    pub fn new(n: usize, cfg: &Config) -> Grid {
        let widths = Rc::new(layout(n.clamp(1, 16)));
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

        let mut panes_vec = Vec::new();
        let mut rows = Vec::new();
        for &w in widths.iter() {
            let row = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .homogeneous(true)
                .spacing(gap)
                .build();
            for _ in 0..w {
                let pane = Pane::new(cfg);
                pane.root.set_hexpand(true);
                pane.root.set_vexpand(true);
                row.append(&pane.root);
                panes_vec.push(pane);
            }
            root.append(&row);
            rows.push(row);
        }

        let panes = Rc::new(panes_vec);
        let focus = Rc::new(Cell::new((0usize, 0usize)));

        // When a terminal gains focus (by click or keyboard), mark it active.
        for (r, &w) in widths.iter().enumerate() {
            for c in 0..w {
                let i = flat_index(&widths, r, c);
                let controller = EventControllerFocus::new();
                // Weak ref breaks the terminal -> controller -> closure -> panes
                // cycle, so old grids (and their child shells) free on re-grid.
                let panes_weak = Rc::downgrade(&panes);
                let widths_c = widths.clone();
                let focus_c = focus.clone();
                controller.connect_enter(move |_| {
                    if let Some(panes) = panes_weak.upgrade() {
                        focus_c.set((r, c));
                        set_active(&panes, &widths_c, (r, c));
                    }
                });
                panes[i].terminal.add_controller(controller);
            }
        }

        set_active(&panes, &widths, (0, 0));

        Grid {
            root,
            panes,
            rows,
            widths,
            focus,
            zoomed: Cell::new(false),
        }
    }

    pub fn focused_pane(&self) -> &Pane {
        let (r, c) = self.focus.get();
        &self.panes[flat_index(&self.widths, r, c)]
    }

    pub fn move_focus(&self, dir: Dir) {
        if self.zoomed.get() {
            return;
        }
        let (r, c) = self.focus.get();
        let nf = neighbor(&self.widths, r, c, dir);
        self.focus.set(nf);
        let idx = flat_index(&self.widths, nf.0, nf.1);
        self.panes[idx].grab_focus(); // focus-enter handler updates the highlight
    }

    /// Hide every pane except the focused one (hidden box children take zero
    /// space, so the focused pane fills the window). Toggle to restore.
    pub fn toggle_zoom(&self) {
        let z = !self.zoomed.get();
        self.zoomed.set(z);
        let (fr, fc) = self.focus.get();
        for (r, row) in self.rows.iter().enumerate() {
            row.set_visible(!z || r == fr);
        }
        let mut idx = 0;
        for (r, &w) in self.widths.iter().enumerate() {
            for c in 0..w {
                self.panes[idx].root.set_visible(!z || (r == fr && c == fc));
                idx += 1;
            }
        }
        self.focused_pane().grab_focus();
    }

    pub fn restart_focused(&self, cfg: &Config) {
        self.focused_pane().respawn(cfg);
    }
}
