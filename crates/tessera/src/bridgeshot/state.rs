//! BridgeShot session state: a list of captured documents (each with its own
//! annotations) plus the current tool/color and the active canvas transform.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk_pixbuf::{InterpType, Pixbuf};

use super::tools::{thumb_dims, Annotation, Rgb, Tool, DEFAULT_COLOR};

const THUMB_W: i32 = 128;

/// One captured/opened image and its annotations.
pub struct Doc {
    pub pixbuf: Pixbuf,
    pub annos: Vec<Annotation>,
    pub thumb: Pixbuf,
}

/// An annotation being drawn (not yet committed), in image space.
pub enum Drag {
    Rect { x0: f64, y0: f64, x1: f64, y1: f64 },
    Stroke { points: Vec<(f64, f64)>, highlight: bool },
}

pub struct State {
    pub docs: Vec<Doc>,
    pub active: Option<usize>,
    pub tool: Tool,
    pub color: Rgb,
    pub drag: Option<Drag>,
    // Canvas transform for the active doc, recomputed every draw().
    pub scale: f64,
    pub off_x: f64,
    pub off_y: f64,
    pub exports: u32,
}

pub type Shot = Rc<RefCell<State>>;

impl State {
    pub fn new() -> Self {
        State {
            docs: Vec::new(),
            active: None,
            tool: Tool::Box,
            color: DEFAULT_COLOR,
            drag: None,
            scale: 1.0,
            off_x: 0.0,
            off_y: 0.0,
            exports: 0,
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
}

/// Append a new document (building its thumbnail), make it active, return index.
pub fn add_doc(shot: &Shot, pixbuf: Pixbuf) -> usize {
    let (tw, th) = thumb_dims(pixbuf.width(), pixbuf.height(), THUMB_W);
    let thumb = pixbuf
        .scale_simple(tw, th, InterpType::Bilinear)
        .unwrap_or_else(|| pixbuf.clone());
    let mut s = shot.borrow_mut();
    s.docs.push(Doc {
        pixbuf,
        annos: Vec::new(),
        thumb,
    });
    let idx = s.docs.len() - 1;
    s.active = Some(idx);
    s.drag = None;
    idx
}

/// Widget point -> image point using the active transform.
pub fn to_image(s: &State, wx: f64, wy: f64) -> (f64, f64) {
    let sc = if s.scale.abs() < 1e-6 { 1.0 } else { s.scale };
    ((wx - s.off_x) / sc, (wy - s.off_y) / sc)
}
