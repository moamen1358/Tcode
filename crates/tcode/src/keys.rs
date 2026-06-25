//! Global Alt-based keyboard shortcuts, installed on the window in the CAPTURE
//! phase so the focused VTE terminal doesn't swallow them first.

use gtk4::gdk::{Key, ModifierType};
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, EventControllerKey, PropagationPhase};
use tcode_core::grid::Dir;

use crate::app::{change_scale, reset_view, set_panes, Shared};

pub fn install(window: &ApplicationWindow, state: &Shared) {
    let controller = EventControllerKey::new();
    controller.set_propagation_phase(PropagationPhase::Capture);

    let st = state.clone();
    controller.connect_key_pressed(move |_c, keyval, _code, mods| {
        // Ctrl+Shift+C / Ctrl+Shift+V: copy / paste in the focused terminal
        // (plain Ctrl+C must stay SIGINT, so copy/paste take the Shift variant).
        if mods.contains(ModifierType::CONTROL_MASK) && mods.contains(ModifierType::SHIFT_MASK) {
            match keyval {
                Key::C | Key::c => {
                    if let Some(g) = st.borrow().grid.as_ref() {
                        g.copy_focused();
                    }
                    return Propagation::Stop;
                }
                Key::V | Key::v => {
                    if let Some(g) = st.borrow().grid.as_ref() {
                        g.paste_focused();
                    }
                    return Propagation::Stop;
                }
                _ => {}
            }
        }

        // Ctrl +/- / 0: zoom the whole UI in / out / reset.
        if mods.contains(ModifierType::CONTROL_MASK) {
            match keyval {
                Key::plus | Key::equal | Key::KP_Add => {
                    change_scale(&st, 1);
                    return Propagation::Stop;
                }
                Key::minus | Key::KP_Subtract => {
                    change_scale(&st, -1);
                    return Propagation::Stop;
                }
                Key::_0 | Key::KP_0 => {
                    reset_view(&st);
                    return Propagation::Stop;
                }
                _ => {}
            }
        }

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
            set_panes(&st, n);
            return Propagation::Stop;
        }

        // Alt+h/j/k/l or Alt+arrows -> move focus between terminals.
        let dir = match keyval {
            Key::h | Key::Left => Some(Dir::Left),
            Key::j | Key::Down => Some(Dir::Down),
            Key::k | Key::Up => Some(Dir::Up),
            Key::l | Key::Right => Some(Dir::Right),
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
            Key::p => {
                let btn = st.borrow().shots_btn.clone();
                btn.set_active(!btn.is_active());
                Propagation::Stop
            }
            _ => Propagation::Proceed,
        }
    });

    window.add_controller(controller);
}
