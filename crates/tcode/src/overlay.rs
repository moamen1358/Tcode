//! A single floating-overlay layer over the work area: a dim scrim plus named
//! floating panels (the clipboard palette, screenshot preview, shots tray). One
//! panel is "open" at a time; Esc or a scrim click closes it. Wraps Frame's
//! content `Overlay` so all floating chrome shares one stacking context.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk::Key;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, EventControllerKey, GestureClick, Orientation, Overlay,
    PropagationPhase, Widget,
};

pub struct OverlayHost {
    /// The window/stack child: wraps the content with the scrim + floating panels.
    pub root: Overlay,
    /// Dim layer behind an open panel; click it to close. Hidden when nothing's open.
    scrim: GtkBox,
    /// The currently-open modal panel, if any (preview toasts don't go here).
    open: RefCell<Option<Widget>>,
    /// Fired whenever an open panel closes — lets a titlebar toggle (e.g. the
    /// screenshots-tray button) release itself so it never reads "on" with the
    /// panel gone.
    on_close: RefCell<Option<Rc<dyn Fn()>>>,
}

impl OverlayHost {
    /// Wrap `content` with the overlay layer. Returns an `Rc` so widget callbacks
    /// (scrim click, Esc) can hold a `Weak` back to it without a cycle.
    pub fn new(content: &impl IsA<Widget>) -> Rc<OverlayHost> {
        let root = Overlay::new();
        root.set_child(Some(content));

        // Dim scrim: full-area, click-catching, hidden until a panel opens.
        let scrim = GtkBox::new(Orientation::Horizontal, 0);
        scrim.add_css_class("overlay-scrim");
        scrim.set_hexpand(true);
        scrim.set_vexpand(true);
        scrim.set_visible(false);
        root.add_overlay(&scrim);

        let host = Rc::new(OverlayHost {
            root: root.clone(),
            scrim: scrim.clone(),
            open: RefCell::new(None),
            on_close: RefCell::new(None),
        });

        // Click the scrim (outside the open panel) → close.
        {
            let click = GestureClick::new();
            let weak = Rc::downgrade(&host);
            click.connect_pressed(move |_, _, _, _| {
                if let Some(h) = weak.upgrade() {
                    h.close();
                }
            });
            scrim.add_controller(click);
        }
        // Esc anywhere in the host → close the open panel (Capture phase so it
        // pre-empts the panes; only consumes Esc when something is actually open,
        // leaving Frame's own annotation-Escape untouched).
        {
            let key = EventControllerKey::new();
            key.set_propagation_phase(PropagationPhase::Capture);
            let weak = Rc::downgrade(&host);
            key.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == Key::Escape {
                    if let Some(h) = weak.upgrade() {
                        if h.is_open() {
                            h.close();
                            return gtk4::glib::Propagation::Stop;
                        }
                    }
                }
                gtk4::glib::Propagation::Proceed
            });
            root.add_controller(key);
        }
        host
    }

    /// Register a floating panel as a hidden overlay child at the given alignment
    /// and a uniform margin. Call `open`/`toggle` to show it (or, for a passive
    /// toast like the screenshot preview, show it directly so it doesn't dim).
    pub fn add_panel(&self, child: &impl IsA<Widget>, halign: Align, valign: Align, margin: i32) {
        child.set_halign(halign);
        child.set_valign(valign);
        child.set_margin_top(margin);
        child.set_margin_bottom(margin);
        child.set_margin_start(margin);
        child.set_margin_end(margin);
        child.set_visible(false);
        self.root.add_overlay(child);
    }

    pub fn is_open(&self) -> bool {
        self.open.borrow().is_some()
    }

    /// Register a callback fired when an open panel closes (see `on_close`).
    pub fn set_on_close(&self, f: Rc<dyn Fn()>) {
        *self.on_close.borrow_mut() = Some(f);
    }

    /// Show `panel` as the modal overlay: reveal the scrim, show + focus it.
    pub fn open(&self, panel: &impl IsA<Widget>) {
        self.close(); // hide any currently-open panel first
        self.scrim.set_visible(true);
        let w: Widget = panel.clone().upcast();
        w.set_visible(true);
        w.grab_focus();
        *self.open.borrow_mut() = Some(w);
    }

    /// Hide the open panel + scrim (no-op if nothing is open). Fires `on_close`
    /// only when a panel was actually showing.
    pub fn close(&self) {
        let closed = self
            .open
            .borrow_mut()
            .take()
            .map(|w| w.set_visible(false))
            .is_some();
        self.scrim.set_visible(false);
        if closed {
            // Clone the callback out first so no borrow is held while it runs (it
            // may re-enter close() via a toggled titlebar button).
            let cb = self.on_close.borrow().clone();
            if let Some(cb) = cb {
                cb();
            }
        }
    }

    /// Open `panel` if it isn't the open one; close if it is.
    pub fn toggle(&self, panel: &impl IsA<Widget>) {
        let target: Widget = panel.clone().upcast();
        let same = self.open.borrow().as_ref() == Some(&target);
        if same {
            self.close();
        } else {
            self.open(panel);
        }
    }
}
