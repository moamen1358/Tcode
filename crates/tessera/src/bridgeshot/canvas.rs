//! Canvas: renders the active document + annotations, and turns pointer input
//! into annotations according to the current tool. Text uses an Entry placed on
//! a Fixed overlay above the DrawingArea.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::cairo;
use gtk4::gdk::prelude::GdkCairoContextExt; // cr.set_source_pixbuf
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    DrawingArea, Entry, EventControllerFocus, EventControllerKey, EventControllerMotion,
    EventControllerScroll, EventControllerScrollFlags, Fixed, GestureClick, GestureDrag, Overlay,
};

use super::state::{to_image, Drag, Shot};
use super::tools::{arrow_head, norm, Annotation, Shape, Tool};

const BG: (f64, f64, f64) = (0.043, 0.047, 0.067); // #0b0c11 — darker than the app so a capture stands out
const FRAME: (f64, f64, f64) = (0.34, 0.37, 0.54); // #565f89 — outline around the image edge
const PEN_W: f64 = 3.0; // image-space px
const HL_W: f64 = 16.0;
const TEXT_SIZE: f64 = 22.0; // image-space px
const ARROW_HEAD_LEN: f64 = 18.0;
const ARROW_HEAD_W: f64 = 14.0;
const MIN_SCALE: f64 = 0.05;
const MAX_SCALE: f64 = 20.0;

pub struct CanvasUi {
    pub area: DrawingArea,
    pub overlay: Overlay,
}

pub fn build(shot: &Shot) -> CanvasUi {
    let area = DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.add_css_class("bridgeshot-canvas");

    let fixed = Fixed::new();
    fixed.set_can_target(false); // transparent to input until a text entry exists

    let overlay = Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&fixed);

    // Draw function.
    {
        let shot = shot.clone();
        area.set_draw_func(move |_a, cr, w, h| draw(cr, w, h, &shot));
    }

    // Image navigation.
    install_scroll_zoom(&area, shot);

    // Move / Box / Arrow / Pen / Highlight via drag.
    install_drag(&area, shot);

    // Text via click (only acts when the Text tool is active).
    install_text_click(&area, &fixed, shot);

    CanvasUi { area, overlay }
}

fn draw(cr: &cairo::Context, w: i32, h: i32, shot: &Shot) {
    cr.set_source_rgb(BG.0, BG.1, BG.2);
    let _ = cr.paint();

    let mut s = shot.borrow_mut();
    let Some(doc_idx) = s.active else {
        draw_hint(cr, w, h);
        return;
    };
    let Some(doc) = s.docs.get(doc_idx) else {
        draw_hint(cr, w, h);
        return;
    };
    let pb = doc.pixbuf.clone();

    let iw = pb.width().max(1) as f64;
    let ih = pb.height().max(1) as f64;
    if s.fit || s.scale.abs() < 1e-6 {
        let scale = (w as f64 / iw).min(h as f64 / ih).min(4.0);
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        s.scale = scale;
        s.off_x = (w as f64 - iw * scale) / 2.0;
        s.off_y = (h as f64 - ih * scale) / 2.0;
    }
    let scale = s.scale;

    let _ = cr.save();
    cr.translate(s.off_x, s.off_y);
    cr.scale(scale, scale);
    cr.set_source_pixbuf(&pb, 0.0, 0.0);
    let _ = cr.paint();

    // Outline the image so its edge is visible even when the capture's own
    // background matches the canvas (screen only — not baked into the export).
    cr.set_source_rgb(FRAME.0, FRAME.1, FRAME.2);
    cr.set_line_width(1.5 / scale);
    cr.rectangle(0.0, 0.0, iw, ih);
    let _ = cr.stroke();

    if let Some(doc) = s.docs.get(doc_idx) {
        paint_annotations(cr, &doc.annos, scale);
    }

    // In-progress preview in the current color.
    if let Some(drag) = &s.drag {
        let color = s.color;
        match drag {
            Drag::Rect { x0, y0, x1, y1 } => match s.tool {
                Tool::Arrow => paint_arrow(cr, *x0, *y0, *x1, *y1, color, scale),
                _ => {
                    let (x, y, bw, bh) = norm(*x0, *y0, *x1, *y1);
                    cr.set_source_rgb(color.0, color.1, color.2);
                    cr.set_line_width(2.0 / scale);
                    cr.set_dash(&[6.0 / scale, 4.0 / scale], 0.0);
                    cr.rectangle(x, y, bw, bh);
                    let _ = cr.stroke();
                    cr.set_dash(&[], 0.0);
                }
            },
            Drag::Stroke { points, highlight } => {
                paint_stroke(cr, points, *highlight, color, scale)
            }
            Drag::Pan { .. } => {}
        }
    }
    let _ = cr.restore();
}

fn draw_hint(cr: &cairo::Context, w: i32, h: i32) {
    cr.set_source_rgb(0.55, 0.6, 0.75);
    cr.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(15.0);
    let msg = "Capture, Open an image, or drop one here";
    let tw = cr.text_extents(msg).map(|e| e.width()).unwrap_or(0.0);
    cr.move_to((w as f64 - tw) / 2.0, h as f64 / 2.0);
    let _ = cr.show_text(msg);
}

/// Draw all committed annotations. `scale` keeps stroke widths constant on screen
/// and at export (export passes scale = 1.0).
pub fn paint_annotations(cr: &cairo::Context, annos: &[Annotation], scale: f64) {
    for a in annos {
        let c = a.color;
        match &a.shape {
            Shape::Box { x, y, w, h } => {
                cr.set_source_rgb(c.0, c.1, c.2);
                cr.set_line_width(3.0 / scale);
                cr.rectangle(*x, *y, *w, *h);
                let _ = cr.stroke();
            }
            Shape::Arrow { x0, y0, x1, y1 } => paint_arrow(cr, *x0, *y0, *x1, *y1, c, scale),
            Shape::Stroke { points, highlight } => paint_stroke(cr, points, *highlight, c, scale),
            Shape::Text {
                x,
                y,
                content,
                size,
            } => {
                cr.set_source_rgb(c.0, c.1, c.2);
                cr.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                cr.set_font_size(*size);
                cr.move_to(*x, *y + *size); // x,y is top-left; baseline below
                let _ = cr.show_text(content);
            }
        }
    }
}

fn paint_arrow(
    cr: &cairo::Context,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    c: (f64, f64, f64),
    scale: f64,
) {
    cr.set_source_rgb(c.0, c.1, c.2);
    cr.set_line_width(3.0 / scale);
    cr.move_to(x0, y0);
    cr.line_to(x1, y1);
    let _ = cr.stroke();
    let [tip, l, r] = arrow_head(x0, y0, x1, y1, ARROW_HEAD_LEN, ARROW_HEAD_W);
    cr.move_to(tip.0, tip.1);
    cr.line_to(l.0, l.1);
    cr.line_to(r.0, r.1);
    cr.close_path();
    let _ = cr.fill();
}

fn paint_stroke(
    cr: &cairo::Context,
    points: &[(f64, f64)],
    highlight: bool,
    c: (f64, f64, f64),
    scale: f64,
) {
    if points.is_empty() {
        return;
    }
    if highlight {
        cr.set_source_rgba(c.0, c.1, c.2, 0.35);
        cr.set_line_width(HL_W / scale);
    } else {
        cr.set_source_rgb(c.0, c.1, c.2);
        cr.set_line_width(PEN_W / scale);
    }
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(points[0].0, points[0].1);
    for p in &points[1..] {
        cr.line_to(p.0, p.1);
    }
    let _ = cr.stroke();
}

fn install_scroll_zoom(area: &DrawingArea, shot: &Shot) {
    let ptr = Rc::new(Cell::new((0.0_f64, 0.0_f64)));

    let motion = EventControllerMotion::new();
    {
        let ptr = ptr.clone();
        motion.connect_motion(move |_c, x, y| ptr.set((x, y)));
    }
    area.add_controller(motion);

    let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
    {
        let (shot, area, ptr) = (shot.clone(), area.clone(), ptr.clone());
        scroll.connect_scroll(move |_c, _dx, dy| {
            let factor = 1.15_f64.powf(-dy);
            if !factor.is_finite() || factor <= 0.0 {
                return glib::Propagation::Stop;
            }
            let (px, py) = ptr.get();
            let mut s = shot.borrow_mut();
            if s.active.is_none() || s.scale <= 0.0 {
                return glib::Propagation::Stop;
            }
            let old_scale = s.scale;
            let new_scale = (old_scale * factor).clamp(MIN_SCALE, MAX_SCALE);
            let ix = (px - s.off_x) / old_scale;
            let iy = (py - s.off_y) / old_scale;
            s.scale = new_scale;
            s.off_x = px - ix * new_scale;
            s.off_y = py - iy * new_scale;
            s.fit = false;
            s.drag = None;
            drop(s);
            area.queue_draw();
            glib::Propagation::Stop
        });
    }
    area.add_controller(scroll);
}

fn install_drag(area: &DrawingArea, shot: &Shot) {
    let drag = GestureDrag::new();

    let (sb, cb) = (shot.clone(), area.clone());
    drag.connect_drag_begin(move |_g, x, y| {
        let mut s = sb.borrow_mut();
        if s.active.is_none() || s.tool == Tool::Text {
            return;
        }
        if s.tool == Tool::Move {
            s.drag = Some(Drag::Pan {
                off_x: s.off_x,
                off_y: s.off_y,
            });
            drop(s);
            cb.set_cursor_from_name(Some("grabbing"));
            return;
        }
        let (ix, iy) = to_image(&s, x, y);
        s.drag = Some(match s.tool {
            Tool::Pen => Drag::Stroke {
                points: vec![(ix, iy)],
                highlight: false,
            },
            Tool::Highlight => Drag::Stroke {
                points: vec![(ix, iy)],
                highlight: true,
            },
            _ => Drag::Rect {
                x0: ix,
                y0: iy,
                x1: ix,
                y1: iy,
            },
        });
        drop(s);
        cb.queue_draw();
    });

    let (su, cu) = (shot.clone(), area.clone());
    drag.connect_drag_update(move |g, dx, dy| {
        let Some((sx, sy)) = g.start_point() else {
            return;
        };
        let mut s = su.borrow_mut();
        if let Some((off_x, off_y)) = match &s.drag {
            Some(Drag::Pan { off_x, off_y }) => Some((*off_x, *off_y)),
            _ => None,
        } {
            s.off_x = off_x + dx;
            s.off_y = off_y + dy;
            s.fit = false;
            drop(s);
            cu.queue_draw();
            return;
        }
        let (ix, iy) = to_image(&s, sx + dx, sy + dy);
        match s.drag.as_mut() {
            Some(Drag::Rect { x1, y1, .. }) => {
                *x1 = ix;
                *y1 = iy;
            }
            Some(Drag::Stroke { points, .. }) => points.push((ix, iy)),
            Some(Drag::Pan { .. }) => return,
            None => return,
        }
        drop(s);
        cu.queue_draw();
    });

    let (se, ce) = (shot.clone(), area.clone());
    drag.connect_drag_end(move |_g, _dx, _dy| {
        let mut s = se.borrow_mut();
        let color = s.color;
        let tool = s.tool;
        if let Some(drag) = s.drag.take() {
            match drag {
                Drag::Pan { .. } => {}
                Drag::Rect { x0, y0, x1, y1 } => {
                    if tool == Tool::Arrow {
                        let dist = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
                        if dist > 4.0 {
                            s.push_anno(Annotation {
                                shape: Shape::Arrow { x0, y0, x1, y1 },
                                color,
                            });
                        }
                    } else {
                        let (x, y, w, h) = norm(x0, y0, x1, y1);
                        if w > 4.0 && h > 4.0 {
                            s.push_anno(Annotation {
                                shape: Shape::Box { x, y, w, h },
                                color,
                            });
                        }
                    }
                }
                Drag::Stroke { points, highlight } => {
                    if points.len() > 1 {
                        s.push_anno(Annotation {
                            shape: Shape::Stroke { points, highlight },
                            color,
                        });
                    }
                }
            }
        }
        drop(s);
        if tool == Tool::Move {
            ce.set_cursor_from_name(Some("grab"));
        }
        ce.queue_draw();
    });

    area.add_controller(drag);
}

fn install_text_click(area: &DrawingArea, fixed: &Fixed, shot: &Shot) {
    let click = GestureClick::new();
    let (sb, fb, ab) = (shot.clone(), fixed.clone(), area.clone());
    click.connect_pressed(move |_g, n, x, y| {
        if n == 2 {
            let mut s = sb.borrow_mut();
            s.fit = true;
            s.drag = None;
            drop(s);
            ab.queue_draw();
            return;
        }
        let s = sb.borrow();
        if s.active.is_none() || s.tool != Tool::Text {
            return;
        }
        let (ix, iy) = to_image(&s, x, y);
        drop(s);
        spawn_text_entry(&sb, &fb, &ab, x, y, ix, iy);
    });
    area.add_controller(click);
}

/// Place an Entry at widget point (wx,wy); commit to a Text annotation at image
/// point (ix,iy) on Enter or focus-out, cancel on Escape.
fn spawn_text_entry(
    shot: &Shot,
    fixed: &Fixed,
    area: &DrawingArea,
    wx: f64,
    wy: f64,
    ix: f64,
    iy: f64,
) {
    let entry = Entry::new();
    entry.set_placeholder_text(Some("type, Enter to place"));
    entry.add_css_class("bridgeshot-text-entry");
    entry.set_width_request(160);
    fixed.set_can_target(true);
    fixed.put(&entry, wx, wy);
    entry.grab_focus();

    let committed = std::rc::Rc::new(std::cell::Cell::new(false));

    let commit = {
        let (shot, fixed, area, entry, committed) = (
            shot.clone(),
            fixed.clone(),
            area.clone(),
            entry.clone(),
            committed.clone(),
        );
        move |save: bool| {
            if committed.replace(true) {
                return;
            }
            let text = entry.text().to_string();
            if save && !text.trim().is_empty() {
                let mut s = shot.borrow_mut();
                let color = s.color;
                s.push_anno(Annotation {
                    shape: Shape::Text {
                        x: ix,
                        y: iy,
                        content: text,
                        size: TEXT_SIZE,
                    },
                    color,
                });
            }
            fixed.remove(&entry);
            fixed.set_can_target(false);
            area.queue_draw();
        }
    };

    {
        let commit = commit.clone();
        entry.connect_activate(move |_| commit(true));
    }
    {
        let focus = EventControllerFocus::new();
        let commit = commit.clone();
        focus.connect_leave(move |_| commit(true));
        entry.add_controller(focus);
    }
    {
        let key = EventControllerKey::new();
        let commit = commit.clone();
        key.connect_key_pressed(move |_c, keyval, _code, _mods| {
            if keyval == gtk4::gdk::Key::Escape {
                commit(false);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        entry.add_controller(key);
    }
}
