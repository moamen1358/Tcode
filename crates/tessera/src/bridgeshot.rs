//! BridgeShot — integrated screenshot annotator for Tessera's main window.
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

use gtk4::gdk::Texture;
use gtk4::gdk::{Key, ModifierType};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, DrawingArea, EventControllerKey, Label, Orientation,
    Overlay, Separator, ToggleButton, Widget,
};

use state::{add_doc, Shot, State};
use tools::{Tool, PALETTE};

/// The integrated BridgeShot UI: a horizontal split of the left screenshots
/// panel and the (overlay-wrapped) main content. `root` is meant to be the
/// window's child; `panel_root` is exposed so the host can toggle it.
pub struct BridgeShot {
    /// The window's child: the content with the (hidden) annotation overlay.
    pub root: Overlay,
    /// The screenshots gallery — the host embeds this at the sidebar's bottom.
    pub panel_root: GtkBox,
    /// Start a capture (region-select → annotate). Wired to the titlebar camera.
    pub capture: Rc<dyn Fn()>,
}

/// Wrap `content` (Tessera's sidebar+center) with the BridgeShot panel and a
/// hidden annotation layer. `main` is the window — hidden during capture so it
/// isn't in the shot, and the clipboard owner on export.
pub fn integrate(main: &ApplicationWindow, content: &impl IsA<Widget>) -> BridgeShot {
    let shot: Shot = Rc::new(RefCell::new(State::new()));
    let canvas_ui = canvas::build(&shot);
    let area = canvas_ui.area.clone();
    let toolbar = build_toolbar(&shot, &area);

    // Annotation layer: hidden until a capture/open; shown over the content.
    let annot = GtkBox::new(Orientation::Vertical, 0);
    annot.add_css_class("bridgeshot-window");
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
        let (shot, annot, area) = (shot.clone(), annot.clone(), area.clone());
        Rc::new(move |pb: Pixbuf| {
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

    // Capture: keep Tessera visible (so you can capture it too, and so the
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

    let panel = gallery::build(on_pick);

    // Save: export PNG → add to panel → copy image to clipboard → close.
    {
        let (shot, panel, main, close_annot) =
            (shot.clone(), panel.clone(), main.clone(), close_annot.clone());
        toolbar.save_btn.connect_clicked(move |_| {
            if let Ok((path, pb)) = export::export_png(&shot) {
                main.clipboard().set_texture(&Texture::for_pixbuf(&pb));
                panel.add_saved(path);
            }
            close_annot();
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

    BridgeShot {
        root: content_overlay,
        panel_root: panel.root.clone(),
        capture: on_capture,
    }
}

struct Toolbar {
    row: GtkBox,
    save_btn: Button,
    cancel_btn: Button,
}

/// The annotation toolbar: tool selector (radio group), color swatches, undo /
/// clear, and Cancel / Save. Tool and color writes go straight to `shot`.
fn build_toolbar(shot: &Shot, area: &DrawingArea) -> Toolbar {
    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.add_css_class("bridgeshot-toolbar");

    let tools_def = [
        (Tool::Box, "Box"),
        (Tool::Arrow, "Arrow"),
        (Tool::Text, "Text"),
        (Tool::Pen, "Pen"),
        (Tool::Highlight, "Highlight"),
    ];
    let tool_btns: Vec<ToggleButton> = tools_def
        .iter()
        .map(|(_, label)| {
            let b = ToggleButton::with_label(label);
            b.add_css_class("bridgeshot-tool");
            b
        })
        .collect();
    let leader = tool_btns[0].clone();
    for b in tool_btns.iter().skip(1) {
        b.set_group(Some(&leader));
    }
    for (i, (tool, _)) in tools_def.iter().enumerate() {
        let (tool, sb) = (*tool, shot.clone());
        tool_btns[i].connect_toggled(move |b| {
            if b.is_active() {
                sb.borrow_mut().tool = tool;
            }
        });
        row.append(&tool_btns[i]);
    }
    tool_btns[0].set_active(true); // Box default

    row.append(&Separator::new(Orientation::Vertical));

    let swatches = GtkBox::new(Orientation::Horizontal, 4);
    swatches.add_css_class("bridgeshot-swatches");
    for (i, color) in PALETTE.iter().enumerate() {
        let sw = Button::new();
        sw.add_css_class("bridgeshot-swatch");
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
        if i == 1 {
            sw.add_css_class("selected"); // default blue
        }
        swatches.append(&sw);
    }
    row.append(&swatches);

    row.append(&Separator::new(Orientation::Vertical));

    let undo_btn = Button::with_label("Undo");
    {
        let (sb, ca) = (shot.clone(), area.clone());
        undo_btn.connect_clicked(move |_| {
            sb.borrow_mut().undo();
            ca.queue_draw();
        });
    }
    let clear_btn = Button::with_label("Clear");
    {
        let (sb, ca) = (shot.clone(), area.clone());
        clear_btn.connect_clicked(move |_| {
            sb.borrow_mut().clear_active();
            ca.queue_draw();
        });
    }
    row.append(&undo_btn);
    row.append(&clear_btn);

    let spacer = Label::new(None);
    spacer.set_hexpand(true);
    row.append(&spacer);

    let cancel_btn = Button::with_label("Cancel");
    let save_btn = Button::with_label("Save");
    save_btn.add_css_class("suggested-action");
    row.append(&cancel_btn);
    row.append(&save_btn);

    Toolbar {
        row,
        save_btn,
        cancel_btn,
    }
}
