//! BridgeShot session state: a list of captured documents (each with its own
//! annotations) plus the current tool/color and the active canvas transform.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk_pixbuf::Pixbuf;

use super::tools::{Annotation, Rgb, Tool, DEFAULT_COLOR};

/// One captured/opened image and its annotations.
pub struct Doc {
    pub pixbuf: Pixbuf,
    pub annos: Vec<Annotation>,
}

/// An annotation being drawn (not yet committed), in image space.
pub enum Drag {
    Pan {
        off_x: f64,
        off_y: f64,
    },
    Rect {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    },
    Stroke {
        points: Vec<(f64, f64)>,
        highlight: bool,
    },
}

pub struct State {
    pub docs: Vec<Doc>,
    pub active: Option<usize>,
    pub tool: Tool,
    pub color: Rgb,
    pub drag: Option<Drag>,
    // Canvas transform for the active doc. Fit mode recomputes it on draw until
    // the user pans the image.
    pub scale: f64,
    pub off_x: f64,
    pub off_y: f64,
    pub fit: bool,
}

pub type Shot = Rc<RefCell<State>>;

impl State {
    pub fn new() -> Self {
        State {
            docs: Vec::new(),
            active: None,
            tool: Tool::Move,
            color: DEFAULT_COLOR,
            drag: None,
            scale: 1.0,
            off_x: 0.0,
            off_y: 0.0,
            fit: true,
        }
    }

    pub fn active_doc(&self) -> Option<&Doc> {
        self.active.and_then(|i| self.docs.get(i))
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut Doc> {
        match self.active {
            Some(i) => self.docs.get_mut(i),
            None => None,
        }
    }

    pub fn push_anno(&mut self, a: Annotation) {
        if let Some(d) = self.active_doc_mut() {
            d.annos.push(a);
        }
    }

    pub fn undo(&mut self) {
        if let Some(d) = self.active_doc_mut() {
            d.annos.pop();
        }
    }

    pub fn clear_active(&mut self) {
        if let Some(d) = self.active_doc_mut() {
            d.annos.clear();
        }
    }

    /// Drop the current annotation document (after save or cancel).
    pub fn clear_docs(&mut self) {
        self.docs.clear();
        self.active = None;
        self.drag = None;
        self.fit = true;
    }
}

/// Make `pixbuf` the active annotation document, returning its index.
///
/// Only the active doc is ever rendered/exported, so we replace rather than
/// append — otherwise reopening saved shots would pile up full-resolution
/// pixbufs in memory with no way to reach the older ones.
pub fn add_doc(shot: &Shot, pixbuf: Pixbuf) -> usize {
    let mut s = shot.borrow_mut();
    s.docs.clear();
    s.docs.push(Doc {
        pixbuf,
        annos: Vec::new(),
    });
    s.active = Some(0);
    s.drag = None;
    s.fit = true;
    0
}

/// Widget point -> image point using the active transform.
pub fn to_image(s: &State, wx: f64, wy: f64) -> (f64, f64) {
    let sc = if s.scale.abs() < 1e-6 { 1.0 } else { s.scale };
    ((wx - s.off_x) / sc, (wy - s.off_y) / sc)
}
