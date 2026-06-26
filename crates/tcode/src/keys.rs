//! Global Alt-based keyboard shortcuts, installed on the window in the CAPTURE
//! phase so the focused VTE terminal doesn't swallow them first.

use gtk4::gdk::{Display, Key, ModifierType};
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, EventControllerKey, PropagationPhase};
use tcode_core::grid::Dir;

use crate::app::{change_scale, reset_view, set_panes, Shared};

pub fn install(window: &ApplicationWindow, state: &Shared) {
    let controller = EventControllerKey::new();
    controller.set_propagation_phase(PropagationPhase::Capture);

    let st = state.clone();
    controller.connect_key_pressed(move |_c, keyval, keycode, mods| {
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
                // Reset zoom on Ctrl+0 from the number row on non-US layouts too
                // (e.g. AZERTY, where the bare 0 key's keyval isn't Key::_0):
                // resolve the digit layout-robustly from the hardware keycode.
                _ if number_row_digit(keyval, keycode, mods) == Some(0) => {
                    reset_view(&st);
                    return Propagation::Stop;
                }
                _ => {}
            }
        }

        if !mods.contains(ModifierType::ALT_MASK) {
            return Propagation::Proceed;
        }

        // Alt+digit -> rebuild the grid with that many panes. Resolve the digit
        // layout-robustly (see number_row_digit) so the physical number row
        // drives this even on layouts that need Shift for digits (e.g. AZERTY).
        if let Some(n) = number_row_digit(keyval, keycode, mods).filter(|&d| (1..=9).contains(&d)) {
            set_panes(&st, n as usize);
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
                // Clone the window out and DROP the borrow before close(): a realized
                // window emits close-request synchronously, whose handler calls
                // save_current -> state.borrow_mut(). Holding this borrow across
                // close() would be a BorrowMutError panic (and the session wouldn't
                // save). Mirrors Key::f above.
                let win = st.borrow().window.clone();
                win.close();
                Propagation::Stop
            }
            Key::p => {
                let btn = st.borrow().shots_btn.clone();
                btn.set_active(!btn.is_active());
                Propagation::Stop
            }
            // Alt+V -> toggle the clipboard command palette (floating, searchable).
            Key::v => {
                let (host, palette) = {
                    let s = st.borrow();
                    (s.host.clone(), s.palette.clone())
                };
                if let (Some(host), Some(palette)) = (host, palette) {
                    host.toggle(&palette);
                }
                Propagation::Stop
            }
            _ => Propagation::Proceed,
        }
    });

    window.add_controller(controller);
}

/// The decimal digit (0..=9) a number-row key produces, resolved by the physical
/// key so the digit shortcuts survive non-US keyboard layouts.
///
/// `keyval` is already the digit on US/QWERTY (tried first, so that path stays
/// exact). Otherwise, only when Shift is NOT held, a non-digit number-row keyval
/// means a layout that needs Shift for its digits (e.g. AZERTY, where the bare
/// `1` key yields `&`): we ask the keymap which keyvals that hardware `keycode`
/// can produce and take the digit. With Shift held the non-digit keyval is a
/// deliberate shifted symbol (e.g. US Alt+Shift+1 = `!`), so we do not fall back
/// — keeping US-layout behavior identical.
fn number_row_digit(keyval: Key, keycode: u32, mods: ModifierType) -> Option<u32> {
    if let Some(d) = keyval.to_unicode().and_then(|c| c.to_digit(10)) {
        return Some(d);
    }
    if mods.contains(ModifierType::SHIFT_MASK) {
        return None;
    }
    let entries = Display::default()?.map_keycode(keycode)?;
    entries
        .into_iter()
        .find_map(|(_, kv)| kv.to_unicode().and_then(|c| c.to_digit(10)))
}
