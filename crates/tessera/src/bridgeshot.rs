//! BridgeShot — capture any window/region (via the desktop screenshot portal),
//! annotate it with boxes, arrows, freehand pen, highlighter, and text in a
//! chosen color, switch between this session's captures via a toggleable left
//! thumbnail gallery, then export an annotated PNG (also copied to the
//! clipboard) to hand to an AI or paste into a chat.

mod canvas;
mod capture;
mod export;
mod gallery;
mod state;
mod tools;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk::Texture;
use gtk4::gdk::{Key, ModifierType};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, DrawingArea, DropTarget, EventControllerKey,
    HeaderBar, Label, Orientation, Paned, Separator, ToggleButton, Window,
};

use state::{add_doc, Shot, State};
use tools::{Tool, PALETTE};

/// Open the BridgeShot window. `main` is Tessera's window — used as the capture
/// fallback when the portal is unavailable.
pub fn launch(main: &ApplicationWindow) {
    let shot: Shot = Rc::new(RefCell::new(State::new()));

    let win = Window::builder()
        .title("BridgeShot")
        .default_width(1180)
        .default_height(780)
        .build();
    win.add_css_class("bridgeshot-window");

    // ----- header -----
    let header = HeaderBar::new();
    let gallery_btn = ToggleButton::new();
    gallery_btn.set_icon_name("sidebar-show-symbolic");
    gallery_btn.set_active(true);
    gallery_btn.set_tooltip_text(Some("Toggle gallery (Alt+G)"));
    gallery_btn.add_css_class("flat");
    let capture_btn = Button::with_label("Capture");
    capture_btn.add_css_class("suggested-action");
    let open_btn = Button::with_label("Open…");
    let undo_btn = Button::with_label("Undo");
    let clear_btn = Button::with_label("Clear");
    let export_btn = Button::with_label("Export");
    export_btn.add_css_class("suggested-action");
    header.pack_start(&gallery_btn);
    header.pack_start(&capture_btn);
    header.pack_start(&open_btn);
    header.pack_end(&export_btn);
    header.pack_end(&clear_btn);
    header.pack_end(&undo_btn);
    win.set_titlebar(Some(&header));

    // ----- canvas + gallery -----
    let canvas_ui = canvas::build(&shot);
    let area = canvas_ui.area.clone();
    let gallery = gallery::new();

    let split = Paned::new(Orientation::Horizontal);
    split.set_start_child(Some(&gallery.root));
    split.set_end_child(Some(&canvas_ui.overlay));
    split.set_resize_start_child(false);
    split.set_shrink_start_child(false);
    split.set_resize_end_child(true);
    split.set_position(160);

    // ----- toolbar row (tools + colors) -----
    let toolbar = GtkBox::new(Orientation::Horizontal, 6);
    toolbar.add_css_class("bridgeshot-toolbar");
    let tools_def = [
        (Tool::Box, "Box"),
        (Tool::Arrow, "Arrow"),
        (Tool::Text, "Text"),
        (Tool::Pen, "Pen"),
        (Tool::Highlight, "Highlight"),
    ];
    let tool_btns: Rc<Vec<(Tool, ToggleButton)>> = Rc::new(
        tools_def
            .iter()
            .map(|(tool, label)| {
                let b = ToggleButton::with_label(label);
                b.add_css_class("bridgeshot-tool");
                (*tool, b)
            })
            .collect(),
    );
    // Radio grouping: exactly one tool active at a time. connect_toggled then
    // fires reliably for both clicks and keyboard `set_active` shortcuts.
    let group_leader = tool_btns[0].1.clone();
    for (_, b) in tool_btns.iter().skip(1) {
        b.set_group(Some(&group_leader));
    }
    for (tool, btn) in tool_btns.iter() {
        let (tool, sb) = (*tool, shot.clone());
        btn.connect_toggled(move |b| {
            if b.is_active() {
                sb.borrow_mut().tool = tool;
            }
        });
        toolbar.append(btn);
    }
    tool_btns[0].1.set_active(true); // Box default

    toolbar.append(&Separator::new(Orientation::Vertical));

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
    toolbar.append(&swatches);

    // ----- status bar -----
    let status = Label::new(Some(
        "Capture anything, Open an image, or drop one here — then pick a tool and draw.",
    ));
    status.set_xalign(0.0);
    status.set_selectable(true);
    status.set_wrap(true);
    status.add_css_class("bridgeshot-status");

    // ----- assemble -----
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&toolbar);
    let mid = GtkBox::new(Orientation::Vertical, 0);
    mid.set_vexpand(true);
    mid.append(&split);
    root.append(&mid);
    root.append(&status);
    win.set_child(Some(&root));

    let gallery = Rc::new(gallery);

    // Helper to load a Pixbuf as a new doc + thumbnail.
    let load: Rc<dyn Fn(Pixbuf)> = {
        let (shot, area, gallery) = (shot.clone(), area.clone(), gallery.clone());
        Rc::new(move |pb: Pixbuf| {
            let idx = add_doc(&shot, pb);
            let thumb = shot.borrow().docs[idx].thumb.clone();
            gallery.add_thumb(&shot, &area, idx, &thumb);
            area.queue_draw();
        })
    };

    // ----- gallery toggle -----
    {
        let gr = gallery.root.clone();
        gallery_btn.connect_toggled(move |b| gr.set_visible(b.is_active()));
    }

    // ----- capture (portal, window fallback) -----
    {
        let (main, win, status, load) = (main.clone(), win.clone(), status.clone(), load.clone());
        capture_btn.connect_clicked(move |_| {
            status.set_text("Choose what to capture…");
            win.set_visible(false);
            let (win, status, load) = (win.clone(), status.clone(), load.clone());
            // Let the window hide before the picker appears.
            let main = main.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
                capture::capture_screen(&main, move |pb| {
                    match pb {
                        Some(pb) => {
                            load(pb);
                            status.set_text("Captured — pick a tool and draw, then Export.");
                        }
                        None => status.set_text("Capture cancelled or unavailable."),
                    }
                    win.present();
                });
            });
        });
    }

    // ----- open -----
    {
        let (win, status, load) = (win.clone(), status.clone(), load.clone());
        open_btn.connect_clicked(move |_| {
            let dialog = gtk4::FileDialog::builder().title("Open image").build();
            let (status, load) = (status.clone(), load.clone());
            dialog.open(Some(&win), gtk4::gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        if let Ok(pb) = Pixbuf::from_file(&path) {
                            load(pb);
                            status.set_text("Loaded image — pick a tool and draw.");
                        }
                    }
                }
            });
        });
    }

    // ----- drop -----
    {
        let dt = DropTarget::new(gtk4::gio::File::static_type(), gtk4::gdk::DragAction::COPY);
        let (status, load) = (status.clone(), load.clone());
        dt.connect_drop(move |_t, value, _x, _y| {
            if let Ok(file) = value.get::<gtk4::gio::File>() {
                if let Some(path) = file.path() {
                    if let Ok(pb) = Pixbuf::from_file(&path) {
                        load(pb);
                        status.set_text("Loaded image — pick a tool and draw.");
                        return true;
                    }
                }
            }
            false
        });
        area.add_controller(dt);
    }

    // ----- undo / clear -----
    {
        let (shot, area) = (shot.clone(), area.clone());
        undo_btn.connect_clicked(move |_| {
            shot.borrow_mut().undo();
            area.queue_draw();
        });
    }
    {
        let (shot, area) = (shot.clone(), area.clone());
        clear_btn.connect_clicked(move |_| {
            shot.borrow_mut().clear_active();
            area.queue_draw();
        });
    }

    // ----- export (save PNG + copy image to clipboard) -----
    {
        let (shot, status, win) = (shot.clone(), status.clone(), win.clone());
        export_btn.connect_clicked(move |_| match export::export_png(&shot) {
            Ok((path, pb)) => {
                let texture = Texture::for_pixbuf(&pb);
                win.clipboard().set_texture(&texture);
                status.set_text(&format!(
                    "Exported → {} (image copied to clipboard)",
                    path.display()
                ));
            }
            Err(e) => status.set_text(&format!("Export failed: {e}")),
        });
    }

    // ----- window-local keys -----
    install_keys(&win, &shot, &area, &gallery_btn, &tool_btns);

    // ----- auto-capture Tessera on open (self-snapshot, like before) -----
    {
        let (status, load, win) = (status.clone(), load.clone(), win.clone());
        capture::capture_window_async(main, move |pb| {
            if let Some(pb) = pb {
                load(pb);
                status.set_text("Captured Tessera — or Capture anything else.");
            }
            win.present();
        });
    }
}

fn install_keys(
    win: &Window,
    shot: &Shot,
    area: &DrawingArea,
    gallery_btn: &ToggleButton,
    tool_btns: &Rc<Vec<(Tool, ToggleButton)>>,
) {
    let controller = EventControllerKey::new();
    let (shot, area, gallery_btn, tool_btns) = (
        shot.clone(),
        area.clone(),
        gallery_btn.clone(),
        tool_btns.clone(),
    );
    controller.connect_key_pressed(move |_c, keyval, _code, mods| {
        let ctrl = mods.contains(ModifierType::CONTROL_MASK);
        let alt = mods.contains(ModifierType::ALT_MASK);

        // Ctrl+Z -> undo.
        if ctrl && keyval == Key::z {
            shot.borrow_mut().undo();
            area.queue_draw();
            return glib::Propagation::Stop;
        }
        // Alt+G -> toggle gallery.
        if alt && keyval == Key::g {
            gallery_btn.set_active(!gallery_btn.is_active());
            return glib::Propagation::Stop;
        }
        // Bare letters -> select tool (skipped automatically while an Entry has
        // focus, since the Entry consumes the keypress before it bubbles here).
        let pick = match keyval {
            Key::b => Some(0),
            Key::a => Some(1),
            Key::t => Some(2),
            Key::p => Some(3),
            Key::h => Some(4),
            _ => None,
        };
        if let Some(i) = pick {
            if !ctrl && !alt {
                tool_btns[i].1.set_active(true); // grouped -> fires toggled -> sets tool
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
    win.add_controller(controller);
}
