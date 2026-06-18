//! A single terminal pane: a VTE terminal wrapped in an Overlay (so we can show
//! an "[exited]" message), spawning the user's shell over a PTY.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::gdk::RGBA;
use gtk4::glib::SpawnFlags;
use gtk4::pango::FontDescription;
use gtk4::prelude::*;
use gtk4::{gio, Align, Label, Overlay};
use tessera_core::config::Config;
use vte4::prelude::*;
use vte4::{PtyFlags, Terminal};

use crate::theme::rgba;

pub struct Pane {
    /// Styled root that goes into the grid (carries the `.pane` CSS class).
    pub root: Overlay,
    pub terminal: Terminal,
    /// Set when the child exits; gates `respawn` and is reset on `spawn`.
    exited: Rc<Cell<bool>>,
}

impl Pane {
    pub fn new(cfg: &Config) -> Pane {
        let terminal = Terminal::new();

        // Cell colors come from the VTE API, not CSS.
        let fg = rgba(&cfg.theme.foreground);
        let bg = rgba(&cfg.theme.background);
        let palette: Vec<RGBA> = cfg.theme.palette.iter().map(|c| rgba(c)).collect();
        let palette_refs: Vec<&RGBA> = palette.iter().collect();
        terminal.set_colors(Some(&fg), Some(&bg), &palette_refs);

        let fd = FontDescription::from_string(&format!("{} {}", cfg.font, cfg.font_size));
        terminal.set_font(Some(&fd));

        let root = Overlay::new();
        root.add_css_class("pane");
        root.set_child(Some(&terminal));

        let exited = Rc::new(Cell::new(false));

        // Connect child-exited ONCE (not per spawn) so restarts don't stack handlers.
        // Capture the overlay WEAKLY (plus the exited flag) so the terminal's signal
        // closure doesn't pin Overlay -> Terminal -> PTY and leak the child shell on
        // every re-grid.
        let overlay_weak = root.downgrade();
        let exited_c = exited.clone();
        terminal.connect_child_exited(move |_t, _status| {
            exited_c.set(true);
            if let Some(overlay) = overlay_weak.upgrade() {
                let label = Label::new(Some("[exited — Alt+r to restart]"));
                label.add_css_class("exited");
                label.set_halign(Align::Center);
                label.set_valign(Align::Center);
                overlay.add_overlay(&label);
            }
        });

        let pane = Pane {
            root,
            terminal,
            exited,
        };
        pane.spawn(cfg);
        pane
    }

    /// Spawn the shell (+ optional startup command) over a PTY.
    fn spawn(&self, cfg: &Config) {
        self.exited.set(false);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(str::to_string))
            .unwrap_or_else(|| "/".into());

        let startup = cfg.startup_command.trim().to_string();
        let argv: Vec<String> = if startup.is_empty() {
            vec![shell.clone(), "-l".into()]
        } else {
            vec![shell.clone(), "-c".into(), format!("{startup}; exec {shell}")]
        };
        let argv_ref: Vec<&str> = argv.iter().map(String::as_str).collect();

        self.terminal.spawn_async(
            PtyFlags::DEFAULT,
            Some(cwd.as_str()),
            &argv_ref,
            &[],
            SpawnFlags::DEFAULT,
            || {},
            -1,
            gio::Cancellable::NONE,
            move |res| {
                if let Err(err) = res {
                    eprintln!("tessera: spawn failed: {err}");
                }
            },
        );
    }

    /// Remove any "[exited]" overlay labels and spawn a fresh shell.
    /// No-op while the pane's shell is still running (Alt+r only restarts exited panes).
    pub fn respawn(&self, cfg: &Config) {
        if !self.exited.get() {
            return;
        }
        let mut child = self.root.first_child();
        while let Some(w) = child {
            child = w.next_sibling();
            if let Ok(label) = w.downcast::<Label>() {
                self.root.remove_overlay(&label);
            }
        }
        self.spawn(cfg);
    }

    pub fn grab_focus(&self) {
        self.terminal.grab_focus();
    }

    /// Type text into the terminal's child as if entered at the keyboard.
    pub fn feed_text(&self, text: &str) {
        self.terminal.feed_child(text.as_bytes());
    }

    pub fn set_active(&self, active: bool) {
        if active {
            self.root.add_css_class("active-pane");
        } else {
            self.root.remove_css_class("active-pane");
        }
    }
}
