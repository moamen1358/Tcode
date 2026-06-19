//! BridgeShot — capture an image (a snapshot of Tessera's own window, or a
//! dropped/opened image), draw numbered boxes with text labels on it, then
//! export an annotated PNG whose path is copied to the clipboard. The result can
//! be handed to an AI to point at specific regions.
//!
//! Capture uses GTK's own renderer to snapshot Tessera's window — reliable on
//! COSMIC/Wayland, where external screenshot tools are blocked.

use std::cell::RefCell;
use std::f64::consts::TAU;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::cairo;
use gtk4::gdk::prelude::GdkCairoContextExt; // cr.set_source_pixbuf
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, DrawingArea, DropTarget, Entry, GestureDrag,
    HeaderBar, Label, ListBox, ListBoxRow, Orientation, Paned, ScrolledWindow, Window,
};

const ACCENT: (f64, f64, f64) = (0.478, 0.635, 0.969); // #7aa2f7
const YELLOW: (f64, f64, f64) = (0.878, 0.686, 0.408); // #e0af68
const BG: (f64, f64, f64) = (0.102, 0.106, 0.149); // #1a1b26

struct Anno {
    n: u32,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    label: String,
}

struct State {
    pixbuf: Option<Pixbuf>,
    boxes: Vec<Anno>,
    next_n: u32,
    drag: Option<(f64, f64, f64, f64)>, // image-space x0,y0,x1,y1
    scale: f64,
    off_x: f64,
    off_y: f64,
    exports: u32,
}

type Shot = Rc<RefCell<State>>;

/// Open the BridgeShot window. `main` is Tessera's window — its self-snapshot is
/// the default capture source.
pub fn launch(main: &ApplicationWindow) {
    let shot: Shot = Rc::new(RefCell::new(State {
        pixbuf: None,
        boxes: Vec::new(),
        next_n: 0,
        drag: None,
        scale: 1.0,
        off_x: 0.0,
        off_y: 0.0,
        exports: 0,
    }));

    let win = Window::builder()
        .title("BridgeShot")
        .default_width(1120)
        .default_height(760)
        .build();
    win.add_css_class("bridgeshot-window");

    let header = HeaderBar::new();
    let capture = Button::with_label("Capture");
    let open = Button::with_label("Open…");
    let undo = Button::with_label("Undo");
    let clear = Button::with_label("Clear");
    let export = Button::with_label("Export");
    export.add_css_class("suggested-action");
    header.pack_start(&capture);
    header.pack_start(&open);
    header.pack_start(&undo);
    header.pack_start(&clear);
    header.pack_end(&export);
    win.set_titlebar(Some(&header));

    let canvas = DrawingArea::new();
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);
    canvas.add_css_class("bridgeshot-canvas");

    let list = ListBox::new();
    list.add_css_class("bridgeshot-list");
    let list_scroll = ScrolledWindow::builder()
        .child(&list)
        .width_request(250)
        .build();

    let split = Paned::new(Orientation::Horizontal);
    split.set_start_child(Some(&canvas));
    split.set_end_child(Some(&list_scroll));
    split.set_resize_start_child(true);
    split.set_resize_end_child(false);
    split.set_position(840);

    let status = Label::new(Some(
        "Capture Tessera, Open an image, or drop one here — then drag on the image to add a labeled box.",
    ));
    status.set_xalign(0.0);
    status.set_selectable(true);
    status.set_wrap(true);
    status.add_css_class("bridgeshot-status");

    let root = GtkBox::new(Orientation::Vertical, 0);
    let split_box = GtkBox::new(Orientation::Vertical, 0);
    split_box.set_vexpand(true);
    split_box.append(&split);
    root.append(&split_box);
    root.append(&status);
    win.set_child(Some(&root));

    // Draw the image + boxes.
    {
        let shot = shot.clone();
        canvas.set_draw_func(move |_a, cr, w, h| draw(cr, w, h, &shot));
    }

    // Drag to add a box.
    {
        let drag = GestureDrag::new();
        let (sb, cb) = (shot.clone(), canvas.clone());
        drag.connect_drag_begin(move |_g, x, y| {
            let mut s = sb.borrow_mut();
            if s.pixbuf.is_none() {
                return;
            }
            let (ix, iy) = to_image(&s, x, y);
            s.drag = Some((ix, iy, ix, iy));
            drop(s);
            cb.queue_draw();
        });
        let (su, cu) = (shot.clone(), canvas.clone());
        drag.connect_drag_update(move |g, dx, dy| {
            let Some((sx, sy)) = g.start_point() else {
                return;
            };
            let mut s = su.borrow_mut();
            if s.drag.is_none() {
                return;
            }
            let (ix, iy) = to_image(&s, sx + dx, sy + dy);
            if let Some(d) = &mut s.drag {
                d.2 = ix;
                d.3 = iy;
            }
            drop(s);
            cu.queue_draw();
        });
        let (se, ce, le) = (shot.clone(), canvas.clone(), list.clone());
        drag.connect_drag_end(move |_g, _dx, _dy| {
            let mut s = se.borrow_mut();
            if let Some((x0, y0, x1, y1)) = s.drag.take() {
                let (x, y, w, h) = norm(x0, y0, x1, y1);
                if w > 4.0 && h > 4.0 {
                    s.next_n += 1;
                    let n = s.next_n;
                    s.boxes.push(Anno {
                        n,
                        x,
                        y,
                        w,
                        h,
                        label: String::new(),
                    });
                    drop(s);
                    add_row(&le, &se, &ce, n);
                    ce.queue_draw();
                    return;
                }
            }
            drop(s);
            ce.queue_draw();
        });
        canvas.add_controller(drag);
    }

    // Re-capture Tessera's window. Hide BridgeShot first so the target window is
    // un-occluded and its frame clock is live, then capture and re-show.
    {
        let (m, sc, cc, stc) = (main.clone(), shot.clone(), canvas.clone(), status.clone());
        let (lc, wc) = (list.clone(), win.clone());
        capture.connect_clicked(move |_| {
            stc.set_text("Capturing Tessera…");
            wc.set_visible(false);
            let (m, sc, cc, stc, lc, wc) = (
                m.clone(),
                sc.clone(),
                cc.clone(),
                stc.clone(),
                lc.clone(),
                wc.clone(),
            );
            // Let the compositor un-occlude Tessera before snapshotting.
            glib::timeout_add_local_once(std::time::Duration::from_millis(90), move || {
                capture_window_async(&m, move |pb| {
                    match pb {
                        Some(pb) => {
                            load_pixbuf(&sc, &lc, pb);
                            cc.queue_draw();
                            stc.set_text("Captured Tessera — drag to add boxes, then Export.");
                        }
                        None => stc.set_text("Capture failed (window not ready)."),
                    }
                    wc.present();
                });
            });
        });
    }

    // Open an image file.
    {
        let (sc, cc, stc, lc) = (shot.clone(), canvas.clone(), status.clone(), list.clone());
        let w = win.clone();
        open.connect_clicked(move |_| {
            let dialog = gtk4::FileDialog::builder().title("Open image").build();
            let (sc, cc, stc, lc) = (sc.clone(), cc.clone(), stc.clone(), lc.clone());
            dialog.open(Some(&w), gtk4::gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        if let Ok(pb) = Pixbuf::from_file(&path) {
                            load_pixbuf(&sc, &lc, pb);
                            cc.queue_draw();
                            stc.set_text("Loaded image — drag to add boxes.");
                        }
                    }
                }
            });
        });
    }

    // Drop an image onto the canvas.
    {
        let dt = DropTarget::new(gtk4::gio::File::static_type(), gtk4::gdk::DragAction::COPY);
        let (sc, cc, stc, lc) = (shot.clone(), canvas.clone(), status.clone(), list.clone());
        dt.connect_drop(move |_t, value, _x, _y| {
            if let Ok(file) = value.get::<gtk4::gio::File>() {
                if let Some(path) = file.path() {
                    if let Ok(pb) = Pixbuf::from_file(&path) {
                        load_pixbuf(&sc, &lc, pb);
                        cc.queue_draw();
                        stc.set_text("Loaded image — drag to add boxes.");
                        return true;
                    }
                }
            }
            false
        });
        canvas.add_controller(dt);
    }

    // Undo last box.
    {
        let (sc, cc, lc) = (shot.clone(), canvas.clone(), list.clone());
        undo.connect_clicked(move |_| {
            let mut s = sc.borrow_mut();
            s.boxes.pop();
            drop(s);
            if let Some(row) = lc.last_child() {
                lc.remove(&row);
            }
            cc.queue_draw();
        });
    }

    // Clear all boxes.
    {
        let (sc, cc, lc) = (shot.clone(), canvas.clone(), list.clone());
        clear.connect_clicked(move |_| {
            let mut s = sc.borrow_mut();
            s.boxes.clear();
            s.next_n = 0;
            drop(s);
            while let Some(row) = lc.last_child() {
                lc.remove(&row);
            }
            cc.queue_draw();
        });
    }

    // Export annotated PNG.
    {
        let (sc, stc) = (shot.clone(), status.clone());
        let w = win.clone();
        export.connect_clicked(move |_| match export_png(&sc) {
            Ok(path) => {
                w.clipboard().set_text(&path.to_string_lossy());
                stc.set_text(&format!("Exported → {} (path copied to clipboard)", path.display()));
            }
            Err(e) => stc.set_text(&format!("Export failed: {e}")),
        });
    }

    // Auto-capture Tessera on open — while it is still the live, focused window,
    // before BridgeShot is presented on top of it — then show BridgeShot.
    {
        let (sc, cc, stc, lc, w) = (
            shot.clone(),
            canvas.clone(),
            status.clone(),
            list.clone(),
            win.clone(),
        );
        capture_window_async(main, move |pb| {
            if let Some(pb) = pb {
                load_pixbuf(&sc, &lc, pb);
                cc.queue_draw();
                stc.set_text("Captured Tessera — drag to add boxes, then Export.");
            }
            w.present();
        });
    }
}

fn load_pixbuf(shot: &Shot, list: &ListBox, pb: Pixbuf) {
    let mut s = shot.borrow_mut();
    s.pixbuf = Some(pb);
    s.boxes.clear();
    s.next_n = 0;
    s.drag = None;
    drop(s);
    while let Some(row) = list.last_child() {
        list.remove(&row);
    }
}

fn to_image(s: &State, wx: f64, wy: f64) -> (f64, f64) {
    let sc = if s.scale.abs() < 1e-6 { 1.0 } else { s.scale };
    ((wx - s.off_x) / sc, (wy - s.off_y) / sc)
}

fn norm(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64, f64, f64) {
    (x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs())
}

fn draw(cr: &cairo::Context, w: i32, h: i32, shot: &Shot) {
    cr.set_source_rgb(BG.0, BG.1, BG.2);
    let _ = cr.paint();

    let mut s = shot.borrow_mut();
    let Some(pb) = s.pixbuf.clone() else {
        cr.set_source_rgb(0.55, 0.6, 0.75);
        cr.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(15.0);
        let msg = "Capture Tessera, Open an image, or drop one here";
        let tw = cr.text_extents(msg).map(|e| e.width()).unwrap_or(0.0);
        cr.move_to((w as f64 - tw) / 2.0, h as f64 / 2.0);
        let _ = cr.show_text(msg);
        return;
    };

    let iw = pb.width() as f64;
    let ih = pb.height() as f64;
    let scale = (w as f64 / iw).min(h as f64 / ih).min(4.0);
    s.scale = scale;
    s.off_x = (w as f64 - iw * scale) / 2.0;
    s.off_y = (h as f64 - ih * scale) / 2.0;

    let _ = cr.save();
    cr.translate(s.off_x, s.off_y);
    cr.scale(scale, scale);
    cr.set_source_pixbuf(&pb, 0.0, 0.0);
    let _ = cr.paint();

    cr.set_line_width(2.0 / scale);
    for b in &s.boxes {
        cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
        cr.rectangle(b.x, b.y, b.w, b.h);
        let _ = cr.stroke();
        draw_badge(cr, b.x, b.y, b.n, scale);
    }
    if let Some((x0, y0, x1, y1)) = s.drag {
        let (x, y, bw, bh) = norm(x0, y0, x1, y1);
        cr.set_dash(&[6.0 / scale, 4.0 / scale], 0.0);
        cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
        cr.rectangle(x, y, bw, bh);
        let _ = cr.stroke();
        cr.set_dash(&[], 0.0);
    }
    let _ = cr.restore();
}

fn draw_badge(cr: &cairo::Context, x: f64, y: f64, n: u32, scale: f64) {
    let r = 11.0 / scale;
    cr.set_source_rgb(BG.0, BG.1, BG.2);
    cr.arc(x + r, y + r, r, 0.0, TAU);
    let _ = cr.fill_preserve();
    cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
    cr.set_line_width(1.5 / scale);
    let _ = cr.stroke();
    cr.set_source_rgb(YELLOW.0, YELLOW.1, YELLOW.2);
    cr.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(13.0 / scale);
    let t = n.to_string();
    if let Ok(e) = cr.text_extents(&t) {
        cr.move_to(
            x + r - e.width() / 2.0 - e.x_bearing(),
            y + r - e.height() / 2.0 - e.y_bearing(),
        );
        let _ = cr.show_text(&t);
    }
}

fn add_row(list: &ListBox, shot: &Shot, canvas: &DrawingArea, n: u32) {
    let row = ListBoxRow::new();
    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);
    hbox.set_margin_start(6);
    hbox.set_margin_end(6);
    let badge = Label::new(Some(&format!("#{n}")));
    badge.add_css_class("bridgeshot-badge");
    let entry = Entry::new();
    entry.set_placeholder_text(Some("describe this box…"));
    entry.set_hexpand(true);
    entry.add_css_class("bridgeshot-entry");
    let (sc, cc) = (shot.clone(), canvas.clone());
    entry.connect_changed(move |e| {
        let text = e.text().to_string();
        let mut s = sc.borrow_mut();
        if let Some(b) = s.boxes.iter_mut().find(|b| b.n == n) {
            b.label = text;
        }
        drop(s);
        cc.queue_draw();
    });
    hbox.append(&badge);
    hbox.append(&entry);
    row.set_child(Some(&hbox));
    list.append(&row);
    entry.grab_focus();
}

/// Snapshot the live window into a `Pixbuf`, asynchronously.
///
/// `WidgetPaintable` only records a render node on the widget's *next* snapshot
/// after it is attached — so a synchronous capture of an already-drawn, static
/// widget yields an empty node. Instead we attach the paintable, force a
/// redraw, and read back two frame-clock ticks later. A timeout backstop avoids
/// hanging if the frame clock is idle (e.g. the window is fully occluded).
fn capture_window_async<F: Fn(Option<Pixbuf>) + 'static>(window: &ApplicationWindow, done: F) {
    // The content child snapshots more reliably than the CSD toplevel.
    let target: gtk4::Widget = window
        .child()
        .unwrap_or_else(|| window.clone().upcast::<gtk4::Widget>());
    let renderer = match window.renderer() {
        Some(r) => r,
        None => {
            done(None);
            return;
        }
    };
    let paintable = gtk4::WidgetPaintable::new(Some(&target));
    target.queue_draw();

    let done = std::rc::Rc::new(std::cell::RefCell::new(Some(done)));
    let ticks = std::cell::Cell::new(0u32);
    {
        let done = done.clone();
        target.add_tick_callback(move |w, _clock| {
            let n = ticks.get() + 1;
            ticks.set(n);
            if n < 2 {
                return glib::ControlFlow::Continue;
            }
            let pb = capture_paintable(&paintable, &renderer, w);
            if let Some(cb) = done.borrow_mut().take() {
                cb(pb);
            }
            glib::ControlFlow::Break
        });
    }

    let done2 = done;
    glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
        if let Some(cb) = done2.borrow_mut().take() {
            cb(None);
        }
    });
}

/// Render the paintable's current node to a `Pixbuf` via the window renderer.
fn capture_paintable(
    paintable: &gtk4::WidgetPaintable,
    renderer: &gtk4::gsk::Renderer,
    w: &gtk4::Widget,
) -> Option<Pixbuf> {
    let (pw, ph) = (w.width(), w.height());
    if pw <= 0 || ph <= 0 {
        return None;
    }
    let snapshot = gtk4::Snapshot::new();
    paintable.snapshot(snapshot.upcast_ref::<gtk4::gdk::Snapshot>(), pw as f64, ph as f64);
    let node = snapshot.to_node()?;
    let texture = renderer.render_texture(&node, None);
    let tmp = std::env::temp_dir().join("tessera-bridgeshot-capture.png");
    texture.save_to_png(&tmp).ok()?;
    Pixbuf::from_file(&tmp).ok()
}

fn export_png(shot: &Shot) -> Result<PathBuf, String> {
    let mut s = shot.borrow_mut();
    let pb = s.pixbuf.clone().ok_or("nothing to export")?;
    let (iw, ih) = (pb.width(), pb.height());

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, iw, ih)
        .map_err(|e| e.to_string())?;
    {
        let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
        cr.set_source_pixbuf(&pb, 0.0, 0.0);
        let _ = cr.paint();
        cr.set_line_width(3.0);
        for b in &s.boxes {
            cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
            cr.rectangle(b.x, b.y, b.w, b.h);
            let _ = cr.stroke();
            draw_badge(&cr, b.x, b.y, b.n, 1.0);
            if !b.label.is_empty() {
                cr.set_source_rgb(YELLOW.0, YELLOW.1, YELLOW.2);
                cr.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                cr.set_font_size(16.0);
                cr.move_to(b.x + 2.0, (b.y - 6.0).max(14.0));
                let _ = cr.show_text(&format!("{}. {}", b.n, b.label));
            }
        }
    }

    let dir = glib::user_cache_dir().join("tessera").join("bridgeshot");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    s.exports += 1;
    let path = dir.join(format!("shot-{}.png", s.exports));
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    surface
        .write_to_png(&mut file)
        .map_err(|e| e.to_string())?;
    Ok(path)
}
