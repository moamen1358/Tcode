//! Global Alt-based keyboard shortcuts, installed on the window in the CAPTURE
//! phase so the focused VTE terminal doesn't swallow them first.

use gtk4::gdk::{Key, ModifierType};
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, EventControllerKey, PropagationPhase};
use tessera_core::grid::Dir;

use crate::app::{show_grid, Shared};

pub fn install(window: &ApplicationWindow, state: &Shared) {
    let controller = EventControllerKey::new();
    controller.set_propagation_phase(PropagationPhase::Capture);

    let st = state.clone();
    controller.connect_key_pressed(move |_c, keyval, _code, mods| {
        if !mods.contains(ModifierType::ALT_MASK) {
            return Propagation::Proceed;
        }

        // Alt+digit -> rebuild the grid with that many panes.
        let digit = match keyval {
            Key::_1 => Some(1),
            Key::_2 => Some(2),
            Key::_3 => Some(3),
            Key::_4 => Some(4),
            Key::_5 => Some(5),
            Key::_6 => Some(6),
            Key::_7 => Some(7),
            Key::_8 => Some(8),
            Key::_9 => Some(9),
            _ => None,
        };
        if let Some(n) = digit {
            show_grid(&st, n);
            return Propagation::Stop;
        }

        // Alt+h/j/k/l -> move focus.
        let dir = match keyval {
            Key::h => Some(Dir::Left),
            Key::j => Some(Dir::Down),
            Key::k => Some(Dir::Up),
            Key::l => Some(Dir::Right),
            _ => None,
        };
        if let Some(dir) = dir {
            if let Some(g) = st.borrow().grid.as_ref() {
                g.move_focus(dir);
            }
            return Propagation::Stop;
        }

        match keyval {
            Key::z => {
                if let Some(g) = st.borrow().grid.as_ref() {
                    g.toggle_zoom();
                }
                Propagation::Stop
            }
            Key::n => {
                if let Some(g) = st.borrow().grid.as_ref() {
                    g.add_pane();
                }
                Propagation::Stop
            }
            Key::b => {
                let btn = st.borrow().sidebar_btn.clone();
                btn.set_active(!btn.is_active());
                Propagation::Stop
            }
            Key::f => {
                let win = st.borrow().window.clone();
                if win.is_fullscreen() {
                    win.unfullscreen();
                } else {
                    win.fullscreen();
                }
                Propagation::Stop
            }
            Key::q => {
                st.borrow().window.close();
                Propagation::Stop
            }
            _ => Propagation::Proceed,
        }
    });

    window.add_controller(controller);
}
