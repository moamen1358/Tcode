//! Frame — integrated screenshot annotator for Tcode's main window.
//!
//! A persistent panel on the left lists saved screenshots (loaded from the cache
//! dir, so they survive restarts). Capturing (its Capture button, or Alt+P opens
//! the panel) grabs any window/region via the desktop screenshot portal and
//! shows an annotation canvas *in place of the center work area* — draw boxes,
//! arrows, freehand pen, highlighter, and text in a chosen color — then Save
//! exports a PNG (also copied to the clipboard) and adds it to the panel.

mod canvas;
mod capture;
mod export;
mod gallery;
mod state;
mod tools;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk4::cairo;
use gtk4::gdk::Texture;
use gtk4::gdk::{DragAction, Key, ModifierType};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, CenterBox, DragSource, DrawingArea,
    EventControllerKey, EventControllerMotion, GestureClick, Orientation, Overlay, Picture,
    Revealer, Separator, ToggleButton, Widget,
};

use state::{add_doc, Shot, State};
use tools::{Tool, PALETTE};

/// The integrated Frame UI: a horizontal split of the left screenshots
/// panel and the (overlay-wrapped) main content. `root` is meant to be the
/// window's child; `panel_root` is exposed so the host can toggle it.
pub struct Frame {
    /// The window's child: the content with the (hidden) annotation overlay.
    pub root: Overlay,
    /// The screenshots gallery — the host embeds this at the sidebar's bottom.
    pub panel_root: GtkBox,
    /// Start a capture (region-select → annotate). Wired to the titlebar camera.
    pub capture: Rc<dyn Fn()>,
    /// Re-open a saved screenshot path in the annotator — used by the floating
    /// bottom-left capture preview (click the image to keep annotating it).
    pub reopen: Rc<dyn Fn(PathBuf)>,
}

/// Wrap `content` (Tcode's sidebar+center) with the Frame panel and a
/// hidden annotation layer. `main` is the window — hidden during capture so it
/// isn't in the shot, and the clipboard owner on export.
pub fn integrate(
    main: &ApplicationWindow,
    content: &impl IsA<Widget>,
    on_saved: Rc<dyn Fn(PathBuf)>,
    on_annot_open: Rc<dyn Fn()>,
) -> Frame {
    let shot: Shot = Rc::new(RefCell::new(State::new()));
    let canvas_ui = canvas::build(&shot);
    let area = canvas_ui.area.clone();
    let toolbar = build_toolbar(&shot, &area);

    // Annotation layer: hidden until a capture/open; shown over the content.
    let annot = GtkBox::new(Orientation::Vertical, 0);
    annot.add_css_class("frame-window");
    annot.set_hexpand(true);
    annot.set_vexpand(true);
    annot.set_visible(false);
    annot.append(&toolbar.row);
    annot.append(&canvas_ui.overlay);

    let content_overlay = Overlay::new();
    content_overlay.set_child(Some(content));
    content_overlay.add_overlay(&annot);

    // Show the annotation layer with a freshly captured/opened image.
    let show_annot: Rc<dyn Fn(Pixbuf)> = {
        let (shot, annot, area, on_annot_open) =
            (shot.clone(), annot.clone(), area.clone(), on_annot_open.clone());
        Rc::new(move |pb: Pixbuf| {
            // The annotator takes over the whole work area — hide the floating
            // panels (the right-edge shots tray, the clipboard palette) so they
            // don't overlap its toolbar (Save/Close sit at the toolbar's right).
            on_annot_open();
            add_doc(&shot, pb);
            annot.set_visible(true);
            area.set_focusable(true);
            area.grab_focus();
            area.queue_draw();
        })
    };

    // Close the annotation layer, discarding the current doc.
    let close_annot: Rc<dyn Fn()> = {
        let (shot, annot) = (shot.clone(), annot.clone());
        Rc::new(move || {
            shot.borrow_mut().clear_docs();
            annot.set_visible(false);
        })
    };

    // Capture: keep Tcode visible (so you can capture it too, and so the
    // self-snapshot fallback has a window to snapshot) and run the portal picker;
    // the compositor's picker overlays the desktop and lets you choose any
    // window/region. The result is loaded into the annotation canvas.
    let on_capture: Rc<dyn Fn()> = {
        let (main, show_annot) = (main.clone(), show_annot.clone());
        Rc::new(move || {
            let show_annot = show_annot.clone();
            capture::capture_screen(&main, move |pb| {
                if let Some(pb) = pb {
                    show_annot(pb);
                }
            });
        })
    };

    // Re-open a saved screenshot to annotate further.
    let on_pick: Rc<dyn Fn(PathBuf)> = {
        let show_annot = show_annot.clone();
        Rc::new(move |path: PathBuf| {
            if let Ok(pb) = Pixbuf::from_file(&path) {
                show_annot(pb);
            }
        })
    };

    let panel = gallery::build(on_pick.clone());

    // Save: export PNG → add to panel → copy image to clipboard → float the
    // bottom-left preview (the on_saved hook) → close.
    {
        let (shot, panel, main, close_annot, on_saved) = (
            shot.clone(),
            panel.clone(),
            main.clone(),
            close_annot.clone(),
            on_saved.clone(),
        );
        toolbar.save_btn.connect_clicked(move |_| {
            match export::export_png(&shot) {
                Ok((path, pb)) => {
                    main.clipboard().set_texture(&Texture::for_pixbuf(&pb));
                    panel.add_saved(path.clone());
                    on_saved(path);
                    close_annot();
                }
                // Keep the annotation canvas open on failure so the user's work
                // isn't silently discarded (e.g. an image too large for cairo).
                Err(e) => eprintln!("tcode: screenshot export failed: {e}"),
            }
        });
    }
    // Cancel: discard and close.
    {
        let close_annot = close_annot.clone();
        toolbar.cancel_btn.connect_clicked(move |_| close_annot());
    }

    // Annotation-mode keys: Escape cancels, Ctrl+Z undoes.
    {
        let key = EventControllerKey::new();
        let (shot, area, close_annot) = (shot.clone(), area.clone(), close_annot.clone());
        key.connect_key_pressed(move |_c, keyval, _code, mods| {
            if keyval == Key::Escape {
                close_annot();
                return glib::Propagation::Stop;
            }
            if mods.contains(ModifierType::CONTROL_MASK) && keyval == Key::z {
                shot.borrow_mut().undo();
                area.queue_draw();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        annot.add_controller(key);
    }

    Frame {
        root: content_overlay,
        panel_root: panel.root.clone(),
        capture: on_capture,
        reopen: on_pick,
    }
}

/// Float a single just-captured image in the bottom-left corner over the work
/// area (no scrim). It fades out after ~30 seconds left alone; hovering cancels
/// the fade. Drag it onto a terminal to insert its path, or click it to re-open
/// the shot in the annotator. The full history lives in the right-side tray (Alt+P)
/// — this is just the "you captured this" glance.
pub fn show_screenshot_preview(
    host: &Rc<crate::overlay::OverlayHost>,
    path: PathBuf,
    reopen: Rc<dyn Fn(PathBuf)>,
) {
    // Decode a bounded thumbnail FIRST: a Picture takes its paintable's intrinsic size
    // as its natural size, so handing it the full-resolution screenshot made the
    // bottom-left preview balloon to fill the whole left edge (set_size_request only
    // sets a floor, never a cap). Pre-scaling to ~200px keeps the floating card small
    // and fixed — a little bigger than the tray thumbnails, nowhere near full size.
    let Ok(pb) = Pixbuf::from_file_at_scale(&path, 200, 110, true) else {
        return;
    };
    let texture = Texture::for_pixbuf(&pb);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("shot-preview");
    let img = Picture::for_paintable(&texture);
    img.add_css_class("shot-preview-img");
    img.set_cursor_from_name(Some("pointer"));
    card.append(&img);

    // Drag the preview straight onto a terminal (inserts the file path) — the whole
    // point of the floating image. A plain Image (not a Button) so the drag starts.
    let drag = DragSource::new();
    drag.set_actions(DragAction::COPY);
    drag.set_content(Some(&crate::dnd::file_drag_provider(&path)));
    {
        let texture = texture.clone();
        drag.connect_drag_begin(move |d, _| d.set_icon(Some(&texture), 0, 0));
    }
    img.add_controller(drag);

    let revealer = Revealer::builder()
        .transition_type(gtk4::RevealerTransitionType::Crossfade)
        .transition_duration(250)
        .child(&card)
        .build();
    host.add_panel(&revealer, Align::Start, Align::End, 14);

    // Fade timer: a ~1-minute countdown to hide; hovering cancels, leaving restarts.
    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let arm: Rc<dyn Fn()> = {
        let (timer, revealer) = (timer.clone(), revealer.downgrade());
        Rc::new(move || {
            if let Some(id) = timer.borrow_mut().take() {
                id.remove();
            }
            let (timer2, revealer2) = (timer.clone(), revealer.clone());
            let id = glib::timeout_add_local(Duration::from_secs(30), move || {
                if let Some(r) = revealer2.upgrade() {
                    r.set_reveal_child(false);
                }
                *timer2.borrow_mut() = None;
                glib::ControlFlow::Break
            });
            *timer.borrow_mut() = Some(id);
        })
    };

    // Drop the overlay child once it has fully faded out (host held weakly).
    {
        let host_weak = Rc::downgrade(host);
        revealer.connect_child_revealed_notify(move |r| {
            if !r.is_child_revealed() {
                if let Some(h) = host_weak.upgrade() {
                    h.root.remove_overlay(r);
                }
            }
        });
    }

    // Hover cancels the pending fade; leaving restarts the countdown.
    {
        let motion = EventControllerMotion::new();
        let (timer_e, arm_l) = (timer.clone(), arm.clone());
        motion.connect_enter(move |_, _, _| {
            if let Some(id) = timer_e.borrow_mut().take() {
                id.remove();
            }
        });
        motion.connect_leave(move |_| arm_l());
        card.add_controller(motion);
    }

    // Click the image → re-open it in the annotator and dismiss the preview.
    {
        let (reopen, path, revealer) = (reopen.clone(), path.clone(), revealer.downgrade());
        let click = GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            reopen(path.clone());
            if let Some(r) = revealer.upgrade() {
                r.set_reveal_child(false);
            }
        });
        img.add_controller(click);
    }

    revealer.set_visible(true);
    revealer.set_reveal_child(true);
    arm();
}

struct Toolbar {
    row: CenterBox,
    save_btn: Button,
    cancel_btn: Button,
}

/// The annotation toolbar: tool selector (radio group), color swatches, undo /
/// clear, and Cancel / Save. Tool and color writes go straight to `shot`.
fn build_toolbar(shot: &Shot, area: &DrawingArea) -> Toolbar {
    let row = CenterBox::new();
    row.add_css_class("frame-toolbar");
    row.set_hexpand(true);

    let center = GtkBox::new(Orientation::Horizontal, 8);
    center.add_css_class("frame-toolbar-center");

    let tool_group = GtkBox::new(Orientation::Horizontal, 0);
    tool_group.add_css_class("frame-tool-group");

    let tools_def = [
        (Tool::Move, ToolPreview::Move, "Move image"),
        (Tool::Box, ToolPreview::Box, "Draw a rectangle"),
        (Tool::Arrow, ToolPreview::Arrow, "Draw an arrow"),
        (Tool::Text, ToolPreview::Text, "Add text"),
        (Tool::Pen, ToolPreview::Pen, "Draw freehand"),
        (Tool::Highlight, ToolPreview::Highlight, "Highlight an area"),
    ];
    let tool_btns: Vec<ToggleButton> = tools_def
        .iter()
        .map(|(_, preview, tip)| {
            let b = ToggleButton::new();
            b.add_css_class("frame-tool");
            b.set_tooltip_text(Some(tip));
            b.set_child(Some(&build_tool_preview(*preview)));
            b
        })
        .collect();
    let leader = tool_btns[0].clone();
    for b in tool_btns.iter().skip(1) {
        b.set_group(Some(&leader));
    }
    for (i, (tool, _, _)) in tools_def.iter().enumerate() {
        let (tool, sb, canvas) = (*tool, shot.clone(), area.clone());
        tool_btns[i].connect_toggled(move |b| {
            if b.is_active() {
                sb.borrow_mut().tool = tool;
                canvas.set_cursor_from_name(if tool == Tool::Move {
                    Some("grab")
                } else {
                    None
                });
            }
        });
        tool_group.append(&tool_btns[i]);
    }
    tool_btns[0].set_active(true); // Move default

    center.append(&tool_group);
    center.append(&Separator::new(Orientation::Vertical));

    let swatches = GtkBox::new(Orientation::Horizontal, 4);
    swatches.add_css_class("frame-swatches");
    for (i, color) in PALETTE.iter().enumerate() {
        let sw = Button::new();
        sw.add_css_class("frame-swatch");
        sw.add_css_class(&format!("swatch-{i}"));
        sw.set_tooltip_text(Some("Annotation color"));
        let (c, sb) = (*color, shot.clone());
        sw.connect_clicked(move |b| {
            sb.borrow_mut().color = c;
            if let Some(p) = b.parent() {
                let mut ch = p.first_child();
                while let Some(w) = ch {
                    w.remove_css_class("selected");
                    ch = w.next_sibling();
                }
            }
            b.add_css_class("selected");
        });
        if i == 0 {
            sw.add_css_class("selected"); // default red
        }
        swatches.append(&sw);
    }
    center.append(&swatches);

    center.append(&Separator::new(Orientation::Vertical));

    let undo_btn = Button::from_icon_name("edit-undo-symbolic");
    undo_btn.add_css_class("frame-utility");
    undo_btn.set_tooltip_text(Some("Undo last annotation"));
    {
        let (sb, ca) = (shot.clone(), area.clone());
        undo_btn.connect_clicked(move |_| {
            sb.borrow_mut().undo();
            ca.queue_draw();
        });
    }
    let clear_btn = Button::from_icon_name("edit-clear-symbolic");
    clear_btn.add_css_class("frame-utility");
    clear_btn.set_tooltip_text(Some("Clear annotations"));
    {
        let (sb, ca) = (shot.clone(), area.clone());
        clear_btn.connect_clicked(move |_| {
            sb.borrow_mut().clear_active();
            ca.queue_draw();
        });
    }
    center.append(&undo_btn);
    center.append(&clear_btn);

    let start = GtkBox::new(Orientation::Horizontal, 0);
    start.set_hexpand(true);

    let actions = GtkBox::new(Orientation::Horizontal, 6);
    actions.add_css_class("frame-actions");
    let cancel_btn = Button::from_icon_name("window-close-symbolic");
    cancel_btn.add_css_class("frame-cancel");
    cancel_btn.set_tooltip_text(Some("Close without saving"));
    let save_btn = Button::from_icon_name("document-save-symbolic");
    save_btn.add_css_class("suggested-action");
    save_btn.add_css_class("frame-save");
    save_btn.set_tooltip_text(Some("Save annotated image"));
    actions.append(&cancel_btn);
    actions.append(&save_btn);

    row.set_start_widget(Some(&start));
    row.set_center_widget(Some(&center));
    row.set_end_widget(Some(&actions));

    Toolbar {
        row,
        save_btn,
        cancel_btn,
    }
}

#[derive(Clone, Copy)]
enum ToolPreview {
    Move,
    Box,
    Arrow,
    Text,
    Pen,
    Highlight,
}

fn build_tool_preview(preview: ToolPreview) -> DrawingArea {
    let icon = DrawingArea::new();
    icon.set_size_request(32, 20);
    icon.set_draw_func(move |_area, cr, w, h| draw_tool_preview(preview, cr, w, h));
    icon
}

fn draw_tool_preview(preview: ToolPreview, cr: &cairo::Context, w: i32, h: i32) {
    let (w, h) = (w as f64, h as f64);
    cr.set_antialias(cairo::Antialias::Best);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    let accent = (0.878, 0.686, 0.408);

    match preview {
        ToolPreview::Move => {
            cr.set_source_rgb(accent.0, accent.1, accent.2);
            cr.set_line_width(1.8);
            let (cx, cy) = (w / 2.0, h / 2.0);
            cr.move_to(cx, 4.5);
            cr.line_to(cx, h - 4.5);
            cr.move_to(6.0, cy);
            cr.line_to(w - 6.0, cy);
            let _ = cr.stroke();
        }
        ToolPreview::Box => {
            cr.set_source_rgb(accent.0, accent.1, accent.2);
            cr.set_line_width(1.8);
            cr.rectangle(7.0, 4.5, w - 14.0, h - 9.0);
            let _ = cr.stroke();
        }
        ToolPreview::Arrow => {
            cr.set_source_rgb(accent.0, accent.1, accent.2);
            cr.set_line_width(2.1);
            let (x0, y0, x1, y1) = (5.5, h - 5.0, w - 6.0, 5.0);
            cr.move_to(x0, y0);
            cr.line_to(x1, y1);
            let _ = cr.stroke();
            draw_arrow_tip(cr, x1, y1, x1 - x0, y1 - y0, 5.6);
        }
        ToolPreview::Text => {
            cr.set_source_rgb(accent.0, accent.1, accent.2);
            cr.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(16.0);
            cr.move_to(10.0, h - 4.0);
            let _ = cr.show_text("T");
        }
        ToolPreview::Pen => {
            cr.set_source_rgb(accent.0, accent.1, accent.2);
            cr.set_line_width(2.0);
            cr.move_to(5.0, h - 5.0);
            cr.curve_to(11.0, 4.0, 18.0, h - 4.0, w - 5.0, 6.0);
            let _ = cr.stroke();
        }
        ToolPreview::Highlight => {
            cr.set_source_rgba(accent.0, accent.1, accent.2, 0.72);
            cr.set_line_width(6.0);
            cr.move_to(6.0, h / 2.0);
            cr.line_to(w - 6.0, h / 2.0);
            let _ = cr.stroke();
        }
    }
}

fn draw_arrow_tip(cr: &cairo::Context, x: f64, y: f64, dx: f64, dy: f64, size: f64) {
    let len = (dx * dx + dy * dy).sqrt();
    if len <= f64::EPSILON {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let (px, py) = (-uy, ux);
    let back = size;
    let wing = size * 0.58;
    cr.move_to(x, y);
    cr.line_to(x - ux * back + px * wing, y - uy * back + py * wing);
    cr.line_to(x - ux * back - px * wing, y - uy * back - py * wing);
    cr.close_path();
    let _ = cr.fill();
}
